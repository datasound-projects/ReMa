//! General chat: conversations, messages and streaming generation.
//!
//! Generation runs in the background: `send_message` stores the user message
//! and an empty assistant message, then returns immediately. Text streams to
//! the UI as `ChatEvent::Delta`; the final message is persisted and announced
//! with `ChatEvent::Finished`.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use tokio_util::sync::CancellationToken;

use crate::{
    db::{
        agents as agents_repo,
        conversations::{self as repo, NewMessage},
        mcp as mcp_repo,
    },
    error::{AppError, AppResult},
    llm::{ChatRequest, Endpoint, Finish, Turn},
    models::{
        chat::{
            ApprovalDecision, ChatEvent, Conversation, ConversationDetail, Message, MessageRole,
            MessageStatus, SendMessageInput, SendMessageResult, ToolActivity,
        },
        provider::ModelRef,
    },
    services::{agents, chat_tools::ChatTools, mcp, profile_context, providers},
    state::AppState,
    time::now_ms,
};

const MAX_MESSAGE_CHARS: usize = 100_000;
const TITLE_CHARS: usize = 60;

/// Responses currently streaming, keyed by assistant message id.
#[derive(Clone, Default)]
pub struct Generations(Arc<Mutex<HashMap<i64, Active>>>);

struct Active {
    conversation_id: i64,
    cancel: CancellationToken,
    content: String,
    /// Tool calls so far, by id (latest state).
    activity: Vec<ToolActivity>,
}

impl Generations {
    fn start(&self, message_id: i64, conversation_id: i64) -> CancellationToken {
        let cancel = CancellationToken::new();
        self.0.lock().unwrap().insert(
            message_id,
            Active {
                conversation_id,
                cancel: cancel.clone(),
                content: String::new(),
                activity: Vec::new(),
            },
        );
        cancel
    }

    fn append(&self, message_id: i64, text: &str) {
        if let Some(active) = self.0.lock().unwrap().get_mut(&message_id) {
            active.content.push_str(text);
        }
    }

    /// Records the latest state of a tool call.
    pub fn record_activity(&self, message_id: i64, activity: ToolActivity) {
        if let Some(active) = self.0.lock().unwrap().get_mut(&message_id) {
            match active.activity.iter_mut().find(|a| a.id == activity.id) {
                Some(existing) => *existing = activity,
                None => active.activity.push(activity),
            }
        }
    }

    fn finish(&self, message_id: i64) -> (String, Vec<ToolActivity>) {
        self.0
            .lock()
            .unwrap()
            .remove(&message_id)
            .map(|active| (active.content, active.activity))
            .unwrap_or_default()
    }

    /// Partial text and tool activity of a streaming message.
    fn snapshot(&self, message_id: i64) -> Option<(String, Vec<ToolActivity>)> {
        self.0
            .lock()
            .unwrap()
            .get(&message_id)
            .map(|active| (active.content.clone(), active.activity.clone()))
    }

    fn is_active(&self, message_id: i64) -> bool {
        self.0.lock().unwrap().contains_key(&message_id)
    }

    fn cancel(&self, message_id: i64) -> bool {
        match self.0.lock().unwrap().get(&message_id) {
            Some(active) => {
                active.cancel.cancel();
                true
            }
            None => false,
        }
    }

    fn cancel_conversation(&self, conversation_id: i64) {
        for active in self.0.lock().unwrap().values() {
            if active.conversation_id == conversation_id {
                active.cancel.cancel();
            }
        }
    }

    /// Stops everything (application shutdown).
    pub fn cancel_all(&self) {
        for active in self.0.lock().unwrap().values() {
            active.cancel.cancel();
        }
    }
}

pub fn list_conversations(state: &AppState) -> AppResult<Vec<Conversation>> {
    state.db.call(|c| repo::list(c, 500))
}

pub fn get_conversation(state: &AppState, id: i64) -> AppResult<ConversationDetail> {
    let (conversation, mut messages) = state
        .db
        .call(|c| Ok((repo::get(c, id)?, repo::list_messages(c, id)?)))?;
    // Show text streamed so far for messages still generating.
    for message in messages
        .iter_mut()
        .filter(|m| m.status == MessageStatus::Streaming)
    {
        if let Some((partial, activity)) = state.generations.snapshot(message.id) {
            message.content = partial;
            message.activity = activity;
        }
    }
    Ok(ConversationDetail {
        conversation,
        messages,
    })
}

pub async fn send_message(
    state: &AppState,
    input: SendMessageInput,
) -> AppResult<SendMessageResult> {
    let content = input.content.trim();
    if content.is_empty() {
        return Err(AppError::validation("Type a message first."));
    }
    if content.chars().count() > MAX_MESSAGE_CHARS {
        return Err(AppError::validation("The message is too long."));
    }
    // Fail fast (before storing anything) if the model cannot be reached.
    let endpoint = providers::resolve_endpoint(state, &input.model.provider_id).await?;
    // Selected agents must exist; MCP servers that are turned off are dropped
    // (never exposed to the model).
    let agent_ids: Vec<String> = agents::resolve(state, &input.agent_ids)?
        .into_iter()
        .map(|a| a.id)
        .collect();
    let mcp_server_ids: Vec<i64> = mcp::enabled_selection(state, &input.mcp_server_ids)?
        .into_iter()
        .map(|s| s.id)
        .collect();

    let now = now_ms();
    let model = input.model.clone();
    let (conversation, user_message, assistant_message) = state.db.call(|conn| {
        let tx = conn.transaction()?;
        let conversation = match input.conversation_id {
            Some(id) => {
                if let Some(last) = repo::last_message(&tx, id)? {
                    if last.status == MessageStatus::Streaming {
                        return Err(AppError::validation(
                            "Wait for the current response to finish, or stop it.",
                        ));
                    }
                }
                repo::touch(&tx, id, &model, now)?;
                repo::get(&tx, id)?
            }
            None => repo::create(&tx, &make_title(content), &model, now)?,
        };
        repo::set_profile_context(&tx, conversation.id, input.use_profile)?;
        agents_repo::set_conversation_agents(&tx, conversation.id, &agent_ids)?;
        mcp_repo::set_conversation_servers(&tx, conversation.id, &mcp_server_ids)?;
        let conversation = repo::get(&tx, conversation.id)?;
        let user_message = repo::insert_message(
            &tx,
            NewMessage {
                conversation_id: conversation.id,
                role: MessageRole::User,
                content,
                status: MessageStatus::Complete,
                model: None,
                created_at: now,
            },
        )?;
        let assistant_message = repo::insert_message(
            &tx,
            NewMessage {
                conversation_id: conversation.id,
                role: MessageRole::Assistant,
                content: "",
                status: MessageStatus::Streaming,
                model: Some(&model),
                created_at: now,
            },
        )?;
        tx.commit()?;
        Ok((conversation, user_message, assistant_message))
    })?;

    spawn_generation(state, &assistant_message, model, endpoint);
    state.events.conversations_changed();
    Ok(SendMessageResult {
        conversation,
        user_message,
        assistant_message,
    })
}

/// Stores the agents and MCP servers selected in a conversation's + menu,
/// so they are still selected when the user comes back to it.
pub fn set_selections(
    state: &AppState,
    conversation_id: i64,
    agent_ids: &[String],
    mcp_server_ids: &[i64],
) -> AppResult<Conversation> {
    let agent_ids: Vec<String> = agents::resolve(state, agent_ids)?
        .into_iter()
        .map(|a| a.id)
        .collect();
    let mcp_server_ids: Vec<i64> = mcp::enabled_selection(state, mcp_server_ids)?
        .into_iter()
        .map(|s| s.id)
        .collect();
    let conversation = state.db.call(|conn| {
        let tx = conn.transaction()?;
        repo::get(&tx, conversation_id)?;
        agents_repo::set_conversation_agents(&tx, conversation_id, &agent_ids)?;
        mcp_repo::set_conversation_servers(&tx, conversation_id, &mcp_server_ids)?;
        let conversation = repo::get(&tx, conversation_id)?;
        tx.commit()?;
        Ok(conversation)
    })?;
    state.events.conversations_changed();
    Ok(conversation)
}

/// Generates the last assistant message of a conversation again.
pub async fn retry(state: &AppState, message_id: i64, model: ModelRef) -> AppResult<Message> {
    let message = state.db.call(|c| repo::get_message(c, message_id))?;
    if message.role != MessageRole::Assistant {
        return Err(AppError::validation("Only responses can be retried."));
    }
    if message.status == MessageStatus::Streaming || state.generations.is_active(message_id) {
        return Err(AppError::validation(
            "This response is still being generated.",
        ));
    }
    let is_last = state
        .db
        .call(|c| repo::last_message(c, message.conversation_id))?
        .is_some_and(|last| last.id == message_id);
    if !is_last {
        return Err(AppError::validation(
            "Only the latest response can be retried.",
        ));
    }
    let endpoint = providers::resolve_endpoint(state, &model.provider_id).await?;
    let message = state.db.call(|c| {
        let message = repo::restart_message(c, message_id, &model)?;
        repo::touch(c, message.conversation_id, &model, now_ms())?;
        Ok(message)
    })?;
    spawn_generation(state, &message, model, endpoint);
    state.events.conversations_changed();
    Ok(message)
}

/// Stops a streaming response; the partial text is kept.
pub fn stop(state: &AppState, message_id: i64) {
    state.generations.cancel(message_id);
}

/// The user's answer to a tool approval request.
pub fn respond_to_tool(
    state: &AppState,
    message_id: i64,
    call_id: &str,
    decision: ApprovalDecision,
) -> AppResult<()> {
    state.approvals.respond(message_id, call_id, decision)
}

pub fn delete_conversation(state: &AppState, id: i64) -> AppResult<()> {
    state.generations.cancel_conversation(id);
    state.db.call(|c| repo::delete(c, id))?;
    state.events.conversations_changed();
    Ok(())
}

/// A short title from the first message: first line, whitespace collapsed,
/// cut at a word boundary.
pub fn make_title(content: &str) -> String {
    let line = content
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("New chat");
    let words: Vec<&str> = line.split_whitespace().collect();
    let mut title = String::new();
    for word in words {
        let extra = if title.is_empty() { 0 } else { 1 } + word.chars().count();
        if title.chars().count() + extra > TITLE_CHARS {
            if title.is_empty() {
                title = word.chars().take(TITLE_CHARS).collect();
            }
            title.push('…');
            return title;
        }
        if !title.is_empty() {
            title.push(' ');
        }
        title.push_str(word);
    }
    title
}

/// The selected agents' instructions, after the base system prompt.
pub fn with_agents(
    state: &AppState,
    mut system: String,
    agent_ids: &[String],
) -> AppResult<String> {
    if let Some(instructions) = agents::compose(&agents::resolve(state, agent_ids)?) {
        system.push_str("\n\n");
        system.push_str(&instructions);
    }
    Ok(system)
}

/// Appends the Profile Context to a system prompt when the user turned it
/// on. Every provider receives the same text.
pub fn with_profile(state: &AppState, mut system: String, use_profile: bool) -> AppResult<String> {
    if use_profile {
        match profile_context::load(state)? {
            Some(context) => {
                system.push_str("\n\n");
                system.push_str(&context.prompt);
            }
            None => system.push_str(
                "\n\nThe user turned on their ReMa Profile, but it is empty. If personal \
                 details matter, suggest adding a CV or details on the Profile page.",
            ),
        }
    }
    Ok(system)
}

/// The system prompt: identity plus the current local date and time.
pub fn system_prompt(now: i64) -> String {
    let zoned = jiff::Timestamp::from_millisecond(now)
        .unwrap_or_else(|_| jiff::Timestamp::now())
        .to_zoned(jiff::tz::TimeZone::system());
    format!(
        "You are ReMa, a helpful assistant in the ReMa desktop app. \
         Current date and time: {} ({}). \
         When you list job openings, show them as a Markdown table with the columns \
         Company, Role, Location, Work mode, Salary, Posted, Key skills and Link (one row \
         per job); write \"—\" for anything the posting does not state and never estimate \
         salaries or dates.",
        zoned.strftime("%A, %e %B %Y %H:%M"),
        zoned.time_zone().iana_name().unwrap_or("local time"),
    )
}

fn spawn_generation(state: &AppState, message: &Message, model: ModelRef, endpoint: Endpoint) {
    let state = state.clone();
    let message_id = message.id;
    let conversation_id = message.conversation_id;
    let cancel = state.generations.start(message_id, conversation_id);
    tauri::async_runtime::spawn(async move {
        generate(&state, conversation_id, message_id, model, endpoint, cancel).await;
    });
}

async fn generate(
    state: &AppState,
    conversation_id: i64,
    message_id: i64,
    model: ModelRef,
    endpoint: Endpoint,
    cancel: CancellationToken,
) {
    let outcome = async {
        let (conversation, history) = state.db.call(|c| {
            Ok((
                repo::get(c, conversation_id)?,
                repo::list_messages(c, conversation_id)?,
            ))
        })?;
        // Base prompt, then agents (selection order), then Profile context.
        let system = with_agents(state, system_prompt(now_ms()), &conversation.agent_ids)?;
        let mut system = with_profile(state, system, conversation.profile_context)?;
        let (tools, notices) = if conversation.mcp_server_ids.is_empty() {
            (None, Vec::new())
        } else {
            ChatTools::prepare(
                state,
                conversation_id,
                message_id,
                &conversation.mcp_server_ids,
                cancel.clone(),
            )
            .await?
        };
        for notice in notices {
            state
                .generations
                .record_activity(message_id, notice.clone());
            state.events.chat(ChatEvent::Activity {
                conversation_id,
                message_id,
                activity: notice,
            });
        }
        if tools.is_some() {
            system.push_str(
                "\n\nTools from the user's MCP servers are available. Use them when they help \
                 answer; say which tool a fact came from. Calls that could change something \
                 wait for the user's approval; if one is declined, continue without it.",
            );
        }
        let request = ChatRequest {
            system: Some(system),
            tools,
            turns: history
                .into_iter()
                .filter(|m| m.id < message_id)
                .filter(|m| matches!(m.status, MessageStatus::Complete | MessageStatus::Stopped))
                .map(|m| Turn {
                    role: m.role,
                    content: m.content,
                })
                .collect(),
            max_output_tokens: providers::max_output_tokens(state, &model)?,
            rounds: Vec::new(),
        };
        let generations = state.generations.clone();
        let events = state.events.clone();
        let mut on_delta = |text: &str| {
            generations.append(message_id, text);
            events.chat(ChatEvent::Delta {
                conversation_id,
                message_id,
                text: text.to_string(),
            });
        };
        state
            .llm
            .stream_chat(&endpoint, &model.model_id, &request, cancel, &mut on_delta)
            .await
    }
    .await;

    let (content, activity) = state.generations.finish(message_id);
    let (status, error) = match outcome {
        Ok(Finish::Cancelled) => (MessageStatus::Stopped, None),
        Ok(Finish::Refused) if content.trim().is_empty() => (
            MessageStatus::Error,
            Some("The model declined to answer this request.".to_string()),
        ),
        Ok(_) if content.trim().is_empty() => (
            MessageStatus::Error,
            Some("The model returned an empty response.".to_string()),
        ),
        Ok(_) => (MessageStatus::Complete, None),
        Err(error) => (MessageStatus::Error, Some(error.to_string())),
    };

    let saved = state.db.call(|c| {
        let message =
            repo::finish_message(c, message_id, &content, status, error.as_deref(), &activity)?;
        repo::touch(c, conversation_id, &model, now_ms())?;
        Ok(message)
    });
    match saved {
        Ok(message) => {
            // Job listings in the answer become available to Analytics.
            if message.status == MessageStatus::Complete {
                crate::analytics::ingest::after_chat_answer(state, message.id);
            }
            state.events.chat(ChatEvent::Finished { message })
        }
        // The conversation was deleted while streaming; nothing to report.
        Err(AppError::NotFound(_)) => {}
        Err(error) => eprintln!("failed to save chat response {message_id}: {error}"),
    }
    state.events.conversations_changed();
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{
        llm::fake::FakeLanguageModel, models::provider::ProviderKind, services::providers,
        state::testing,
    };

    async fn setup(
        llm: FakeLanguageModel,
    ) -> (
        AppState,
        Arc<crate::events::RecordingEvents>,
        Arc<FakeLanguageModel>,
    ) {
        let llm = Arc::new(llm);
        let (state, events) = testing::state(llm.clone());
        providers::connect(&state, ProviderKind::Openai, "sk-test")
            .await
            .unwrap();
        (state, events, llm)
    }

    fn model() -> ModelRef {
        ModelRef {
            provider_id: "openai".into(),
            model_id: "model-a".into(),
        }
    }

    fn send(conversation_id: Option<i64>, content: &str) -> SendMessageInput {
        SendMessageInput {
            conversation_id,
            content: content.into(),
            model: model(),
            use_profile: false,
            agent_ids: Vec::new(),
            mcp_server_ids: Vec::new(),
        }
    }

    async fn wait_until_done(state: &AppState, message_id: i64) -> Message {
        for _ in 0..200 {
            let message = state.db.call(|c| repo::get_message(c, message_id)).unwrap();
            if message.status != MessageStatus::Streaming {
                return message;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("generation did not finish");
    }

    #[tokio::test]
    async fn profile_context_is_sent_only_when_turned_on() {
        let (state, _, llm) = setup(FakeLanguageModel::replying(&["ok"])).await;
        crate::services::profile::save(
            &state,
            crate::models::profile::Profile {
                first_name: "Ana".into(),
                title: "Data Engineer".into(),
                email: "ana@example.com".into(),
                skills: vec!["Kubernetes".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let system_of = |i: usize| {
            llm.requests.lock().unwrap()[i]
                .1
                .system
                .clone()
                .unwrap_or_default()
        };

        // OFF (default): nothing about the profile reaches the model.
        let off = send_message(&state, send(None, "Find AI jobs"))
            .await
            .unwrap();
        wait_until_done(&state, off.assistant_message.id).await;
        assert!(!off.conversation.profile_context);
        assert!(!system_of(0).contains("user_profile"));
        assert!(!system_of(0).contains("Kubernetes"));

        // ON: the structured profile is included, without contact details.
        let mut input = send(Some(off.conversation.id), "Use my profile");
        input.use_profile = true;
        let on = send_message(&state, input).await.unwrap();
        wait_until_done(&state, on.assistant_message.id).await;
        assert!(on.conversation.profile_context);
        let system = system_of(1);
        assert!(system.contains("<user_profile>"), "{system}");
        assert!(system.contains("Skills: Kubernetes"));
        assert!(!system.contains("ana@example.com"));

        // Retrying keeps the conversation's choice; turning it off stops it.
        retry(&state, on.assistant_message.id, model())
            .await
            .unwrap();
        wait_until_done(&state, on.assistant_message.id).await;
        assert!(system_of(2).contains("<user_profile>"));
        let again = send_message(&state, send(Some(off.conversation.id), "Thanks"))
            .await
            .unwrap();
        wait_until_done(&state, again.assistant_message.id).await;
        assert!(!system_of(3).contains("user_profile"));
    }

    #[tokio::test]
    async fn streams_and_persists_a_response() {
        let (state, events, llm) = setup(FakeLanguageModel::replying(&["Hello", " there"])).await;
        let sent = send_message(&state, send(None, "  Hi ReMa\nsecond line "))
            .await
            .unwrap();
        assert_eq!(sent.conversation.title, "Hi ReMa");
        assert_eq!(sent.user_message.content, "Hi ReMa\nsecond line");
        assert_eq!(sent.assistant_message.status, MessageStatus::Streaming);

        let reply = wait_until_done(&state, sent.assistant_message.id).await;
        assert_eq!(reply.status, MessageStatus::Complete);
        assert_eq!(reply.content, "Hello there");
        assert_eq!(reply.model, Some(model()));

        let chat_events = events.chat.lock().unwrap();
        let deltas = chat_events
            .iter()
            .filter(|e| matches!(e, ChatEvent::Delta { .. }))
            .count();
        assert_eq!(deltas, 2);
        assert!(matches!(
            chat_events.last(),
            Some(ChatEvent::Finished { .. })
        ));

        let requests = llm.requests.lock().unwrap();
        let (model_id, request) = &requests[0];
        assert_eq!(model_id, "model-a");
        assert!(request.system.as_deref().unwrap().contains("ReMa"));
        assert_eq!(request.turns.len(), 1);
    }

    #[tokio::test]
    async fn sends_the_conversation_history() {
        let (state, _, llm) = setup(FakeLanguageModel::replying(&["ok"])).await;
        let first = send_message(&state, send(None, "one")).await.unwrap();
        wait_until_done(&state, first.assistant_message.id).await;
        let second = send_message(&state, send(Some(first.conversation.id), "two"))
            .await
            .unwrap();
        wait_until_done(&state, second.assistant_message.id).await;

        let requests = llm.requests.lock().unwrap();
        let turns: Vec<_> = requests[1]
            .1
            .turns
            .iter()
            .map(|t| t.content.as_str())
            .collect();
        assert_eq!(turns, ["one", "ok", "two"]);
        assert_eq!(list_conversations(&state).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn stop_keeps_the_partial_response() {
        let mut llm = FakeLanguageModel::replying(&["a", "b", "c", "d", "e"]);
        llm.delay = Duration::from_millis(50);
        let (state, _, _) = setup(llm).await;
        let sent = send_message(&state, send(None, "long")).await.unwrap();

        tokio::time::sleep(Duration::from_millis(120)).await;
        let live = get_conversation(&state, sent.conversation.id).unwrap();
        assert!(
            !live.messages[1].content.is_empty(),
            "partial text is visible while streaming"
        );

        stop(&state, sent.assistant_message.id);
        let reply = wait_until_done(&state, sent.assistant_message.id).await;
        assert_eq!(reply.status, MessageStatus::Stopped);
        assert!(!reply.content.is_empty() && reply.content.len() < 5);
    }

    #[tokio::test]
    async fn failures_are_stored_and_can_be_retried() {
        let mut llm = FakeLanguageModel::replying(&[]);
        llm.fail_with = Some("Anthropic is unavailable right now (529)".into());
        let (state, _, _) = setup(llm).await;
        let sent = send_message(&state, send(None, "hello")).await.unwrap();
        let failed = wait_until_done(&state, sent.assistant_message.id).await;
        assert_eq!(failed.status, MessageStatus::Error);
        assert!(failed.error.unwrap().contains("529"));

        let retried = retry(&state, failed.id, model()).await.unwrap();
        assert_eq!(retried.status, MessageStatus::Streaming);
        assert_eq!(
            wait_until_done(&state, failed.id).await.status,
            MessageStatus::Error
        );
    }

    #[tokio::test]
    async fn rejects_empty_messages_and_unknown_models() {
        let (state, _, _) = setup(FakeLanguageModel::replying(&["x"])).await;
        assert!(matches!(
            send_message(&state, send(None, "  ")).await,
            Err(AppError::Validation(_))
        ));

        let unknown = SendMessageInput {
            conversation_id: None,
            content: "hi".into(),
            model: ModelRef {
                provider_id: "gemini".into(),
                model_id: "x".into(),
            },
            use_profile: false,
            agent_ids: Vec::new(),
            mcp_server_ids: Vec::new(),
        };
        assert!(matches!(
            send_message(&state, unknown).await,
            Err(AppError::Configuration(_))
        ));
        assert!(
            list_conversations(&state).unwrap().is_empty(),
            "nothing stored on failure"
        );
    }

    #[tokio::test]
    async fn deleting_a_conversation_removes_its_messages() {
        let (state, _, _) = setup(FakeLanguageModel::replying(&["x"])).await;
        let sent = send_message(&state, send(None, "hi")).await.unwrap();
        wait_until_done(&state, sent.assistant_message.id).await;
        delete_conversation(&state, sent.conversation.id).unwrap();
        assert!(matches!(
            get_conversation(&state, sent.conversation.id),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn makes_short_titles() {
        assert_eq!(
            make_title("\n  Find   AI jobs in Vienna \nmore"),
            "Find AI jobs in Vienna"
        );
        let long = "word ".repeat(40);
        let title = make_title(&long);
        assert!(title.ends_with('…') && title.chars().count() <= TITLE_CHARS + 1);
    }

    // ── Agents, Profile sources and MCP tools ───────────────────────

    #[tokio::test]
    async fn agent_instructions_are_sent_once_in_selection_order_and_only_when_selected() {
        let (state, _, llm) = setup(FakeLanguageModel::replying(&["ok"])).await;
        let system_of = |i: usize| {
            llm.requests.lock().unwrap()[i]
                .1
                .system
                .clone()
                .unwrap_or_default()
        };

        // No agent: no agent instructions.
        let plain = send_message(&state, send(None, "Hi")).await.unwrap();
        wait_until_done(&state, plain.assistant_message.id).await;
        assert!(!system_of(0).contains("<agent "));

        // Two agents (and a duplicate) in this order; Profile stays off.
        let custom = crate::services::agents::save(
            &state,
            None,
            crate::models::agent::AgentInput {
                name: "Recruiter Voice".into(),
                instructions: "Write like a friendly recruiter.".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let mut input = send(None, "Tailor my CV");
        input.agent_ids = vec![
            custom.id.clone(),
            "builtin:cv-tailoring".into(),
            custom.id.clone(),
        ];
        let sent = send_message(&state, input).await.unwrap();
        assert_eq!(
            sent.conversation.agent_ids,
            [custom.id.clone(), "builtin:cv-tailoring".to_string()]
        );
        assert!(
            !sent.conversation.profile_context,
            "agents never turn Profile on"
        );
        wait_until_done(&state, sent.assistant_message.id).await;
        let system = system_of(1);
        assert_eq!(
            system.matches("Write like a friendly recruiter.").count(),
            1
        );
        assert_eq!(system.matches("<agent name=").count(), 2);
        assert!(
            system.find("Recruiter Voice").unwrap() < system.find("CV Tailoring Agent").unwrap()
        );
        assert!(!system.contains("<user_profile>"));

        // The selection stays with the conversation (Retry uses it too).
        let detail = get_conversation(&state, sent.conversation.id).unwrap();
        assert_eq!(detail.conversation.agent_ids.len(), 2);

        // Unknown agents are refused before anything is stored.
        let mut bad = send(None, "x");
        bad.agent_ids = vec!["custom:999".into()];
        assert!(send_message(&state, bad).await.is_err());
    }

    #[tokio::test]
    async fn profile_on_includes_only_the_sources_that_exist() {
        let (state, _, llm) = setup(FakeLanguageModel::replying(&["ok"])).await;
        let source = state.data_dir.join("Ana CV.pdf");
        std::fs::write(
            &source,
            crate::services::documents::samples::pdf(&["Ana Tester", "Kubernetes platform lead"]),
        )
        .unwrap();
        crate::services::profile::add_document(&state, &source, None)
            .await
            .unwrap();

        let mut input = send(None, "What am I good at?");
        input.use_profile = true;
        let sent = send_message(&state, input).await.unwrap();
        wait_until_done(&state, sent.assistant_message.id).await;
        let system = llm.requests.lock().unwrap()[0].1.system.clone().unwrap();
        assert!(system.contains("<source type=\"cv\" name=\"Ana CV\" primary=\"true\">"));
        assert!(system.contains("Kubernetes platform lead"));
        assert!(
            !system.contains("custom_profile"),
            "no empty Custom Profile section"
        );
        assert!(!system.contains("type=\"credentials\""));

        // Off again: nothing from the Profile.
        let mut off = send(Some(sent.conversation.id), "And now?");
        off.use_profile = false;
        let sent = send_message(&state, off).await.unwrap();
        wait_until_done(&state, sent.assistant_message.id).await;
        let system = llm.requests.lock().unwrap()[1].1.system.clone().unwrap();
        assert!(!system.contains("Kubernetes platform lead"));
        assert!(!system.contains("<user_profile>"));
    }

    fn test_server() -> Option<crate::models::mcp::McpServerInput> {
        let node = std::process::Command::new("node")
            .arg("--version")
            .output()
            .ok()?;
        if !node.status.success() {
            return None;
        }
        Some(crate::models::mcp::McpServerInput {
            name: "Tests".into(),
            transport: crate::models::mcp::McpTransport::Stdio,
            command: "node".into(),
            args: vec![concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/mcp_test_server.mjs"
            )
            .into()],
            env: vec![crate::models::mcp::McpEnvVar {
                name: "FAKE_MCP_ERA".into(),
                value: Some("modern".into()),
            }],
            cwd: String::new(),
            url: String::new(),
            auth: crate::models::mcp::McpAuth::None,
            header_name: String::new(),
            secret: None,
        })
    }

    fn call(id: &str, name: &str, arguments: serde_json::Value) -> crate::llm::ToolCall {
        crate::llm::ToolCall {
            id: id.into(),
            name: name.into(),
            arguments,
            provider_data: None,
        }
    }

    async fn awaiting(events: &crate::events::RecordingEvents, message: i64, call_id: &str) {
        for _ in 0..500 {
            let waiting = events.chat.lock().unwrap().iter().any(|e| {
                matches!(e, ChatEvent::Activity { activity, message_id, .. }
                    if *message_id == message
                        && activity.id == call_id
                        && activity.status == crate::models::chat::ToolStatus::AwaitingApproval)
            });
            if waiting {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no approval request for {call_id}");
    }

    #[tokio::test]
    async fn selected_mcp_tools_run_with_approval_for_changes() {
        use crate::models::chat::ToolStatus;
        let Some(server_input) = test_server() else {
            eprintln!("node is not installed; skipping the MCP chat test");
            return;
        };
        let llm = FakeLanguageModel::replying(&["Done."]).calling(vec![
            call("c1", "mcp_tests_echo", serde_json::json!({ "text": "hi" })),
            call(
                "c2",
                "mcp_tests_add_note",
                serde_json::json!({ "note": "call Globex" }),
            ),
        ]);
        let (state, events, llm) = setup(llm).await;
        let server = crate::services::mcp::save(&state, None, server_input)
            .await
            .unwrap();

        // Configured but disabled: never exposed, even if selected.
        let mut input = send(None, "Use my tools");
        input.mcp_server_ids = vec![server.id];
        let sent = send_message(&state, input).await.unwrap();
        assert!(sent.conversation.mcp_server_ids.is_empty());
        wait_until_done(&state, sent.assistant_message.id).await;
        assert!(llm.requests.lock().unwrap()[0].1.tools.is_none());

        // Enabled and selected: the tools are offered for this chat.
        crate::services::mcp::set_enabled(&state, server.id, true).unwrap();
        let mut input = send(None, "Use my tools");
        input.mcp_server_ids = vec![server.id];
        let sent = send_message(&state, input).await.unwrap();
        assert_eq!(sent.conversation.mcp_server_ids, [server.id]);
        // The read-only tool runs at once; the other waits for approval.
        awaiting(&events, sent.assistant_message.id, "c2").await;
        state
            .approvals
            .respond(sent.assistant_message.id, "c2", ApprovalDecision::Allow)
            .unwrap();
        let message = wait_until_done(&state, sent.assistant_message.id).await;
        assert_eq!(message.status, MessageStatus::Complete);
        assert_eq!(message.content, "Done.");
        let outputs: Vec<String> = llm
            .tool_outputs
            .lock()
            .unwrap()
            .iter()
            .map(|o| o.content.clone())
            .collect();
        assert_eq!(outputs, ["echo: hi", "saved: call Globex"]);
        let names: Vec<String> = llm.requests.lock().unwrap()[1]
            .1
            .tool_specs()
            .iter()
            .map(|t| t.name.clone())
            .collect();
        assert_eq!(
            names,
            ["mcp_tests_echo", "mcp_tests_add_note", "mcp_tests_env"]
        );
        // The activity is stored with the answer.
        assert_eq!(message.activity.len(), 2);
        assert!(message.activity[0].read_only);
        assert_eq!(message.activity[0].status, ToolStatus::Completed);
        assert_eq!(message.activity[1].status, ToolStatus::Completed);
        assert!(message.activity[1].arguments.contains("call Globex"));

        // Denied: the tool does not run and the model is told.
        let mut again = send(Some(sent.conversation.id), "Once more");
        again.mcp_server_ids = vec![server.id];
        let sent = send_message(&state, again).await.unwrap();
        awaiting(&events, sent.assistant_message.id, "c2").await;
        state
            .approvals
            .respond(sent.assistant_message.id, "c2", ApprovalDecision::Deny)
            .unwrap();
        let message = wait_until_done(&state, sent.assistant_message.id).await;
        assert_eq!(message.activity[1].status, ToolStatus::Denied);
        let last = llm.tool_outputs.lock().unwrap().last().cloned().unwrap();
        assert!(last.is_error);
        assert!(!last.content.contains("saved"));

        // Allowed for the chat: no second question in this conversation.
        let mut third = send(Some(sent.conversation.id), "And again");
        third.mcp_server_ids = vec![server.id];
        let sent3 = send_message(&state, third).await.unwrap();
        awaiting(&events, sent3.assistant_message.id, "c2").await;
        let pending_id = sent3.assistant_message.id;
        state
            .approvals
            .respond(pending_id, "c2", ApprovalDecision::AllowForChat)
            .unwrap();
        wait_until_done(&state, pending_id).await;
        let mut fourth = send(Some(sent.conversation.id), "Last one");
        fourth.mcp_server_ids = vec![server.id];
        let sent4 = send_message(&state, fourth).await.unwrap();
        let message = wait_until_done(&state, sent4.assistant_message.id).await;
        assert_eq!(message.activity[1].status, ToolStatus::Completed);

        // Turned off in Settings: gone from the chat's tools.
        crate::services::mcp::set_enabled(&state, server.id, false).unwrap();
        let mut fifth = send(Some(sent.conversation.id), "Tools?");
        fifth.mcp_server_ids = vec![server.id];
        let sent5 = send_message(&state, fifth).await.unwrap();
        assert!(sent5.conversation.mcp_server_ids.is_empty());
        wait_until_done(&state, sent5.assistant_message.id).await;
        assert!(llm
            .requests
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .1
            .tools
            .is_none());
        state.mcp.shutdown();
    }
}
