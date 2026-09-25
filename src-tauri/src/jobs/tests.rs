//! Whole-run tests with fake Gmail, Calendar and model: repeated runs are
//! idempotent, reschedules and cancellations update the same event, unclear
//! interviews never reach Calendar, and conflicts are reported.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use tokio_util::sync::CancellationToken;

use super::*;
use crate::{
    db::jobs::{self as repo, InterviewState},
    integrations::google::{
        calendar::{CalendarEvent, EventDraft},
        gmail::{MessageContent, MessageRef},
    },
    llm::{BoxFuture, DeltaSink, FetchedModel, LanguageModel},
    models::{
        jobs::ApplicationStatus,
        provider::{ModelRef, ProviderKind},
    },
    state::testing,
};

const HOUR: i64 = 3_600_000;

fn at(text: &str) -> i64 {
    text.parse::<jiff::Timestamp>().unwrap().as_millisecond()
}

fn now() -> i64 {
    at("2026-09-20T08:00:00Z")
}

// ── Fakes ───────────────────────────────────────────────────────────

#[derive(Default)]
struct FakeGmail {
    messages: Mutex<Vec<MessageContent>>,
    full_reads: Mutex<Vec<String>>,
}

impl FakeGmail {
    fn add(&self, id: &str, thread: &str, from: &str, subject: &str, received_at: i64, body: &str) {
        self.messages.lock().unwrap().push(MessageContent {
            meta: MessageMeta {
                id: id.into(),
                thread_id: thread.into(),
                received_at,
                from: from.into(),
                subject: subject.into(),
                snippet: body.chars().take(40).collect(),
            },
            body: body.into(),
        });
    }

    fn find(&self, id: &str) -> AppResult<MessageContent> {
        self.messages
            .lock()
            .unwrap()
            .iter()
            .find(|m| m.meta.id == id)
            .cloned()
            .ok_or_else(|| AppError::not_found("no such message"))
    }
}

impl GmailApi for FakeGmail {
    fn search<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> BoxFuture<'a, AppResult<Vec<MessageRef>>> {
        Box::pin(async move {
            let after: i64 = query
                .split_whitespace()
                .find_map(|t| t.strip_prefix("after:"))
                .and_then(|s| s.parse().ok())
                .expect("the search is always bounded by date");
            let mut found: Vec<_> = self
                .messages
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.meta.received_at >= after * 1000)
                .map(|m| {
                    (
                        m.meta.received_at,
                        MessageRef {
                            id: m.meta.id.clone(),
                            thread_id: m.meta.thread_id.clone(),
                        },
                    )
                })
                .collect();
            found.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
            Ok(found.into_iter().map(|(_, r)| r).take(limit).collect())
        })
    }

    fn metadata<'a>(&'a self, id: &'a str) -> BoxFuture<'a, AppResult<MessageMeta>> {
        Box::pin(async move { self.find(id).map(|m| m.meta) })
    }

    fn message<'a>(&'a self, id: &'a str) -> BoxFuture<'a, AppResult<MessageContent>> {
        Box::pin(async move {
            self.full_reads.lock().unwrap().push(id.to_string());
            self.find(id)
        })
    }

    fn thread<'a>(&'a self, _thread_id: &'a str) -> BoxFuture<'a, AppResult<Vec<MessageMeta>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

#[derive(Default)]
struct FakeCalendar {
    events: Mutex<Vec<(CalendarEvent, Option<EventDraft>)>>,
    writes: Mutex<Vec<String>>,
}

impl FakeCalendar {
    fn add_busy(&self, id: &str, title: &str, start_at: i64, end_at: i64) {
        self.events.lock().unwrap().push((
            CalendarEvent {
                id: id.into(),
                title: title.into(),
                start_at: Some(start_at),
                end_at: Some(end_at),
                all_day: false,
                transparent: false,
                interview_id: None,
            },
            None,
        ));
    }

    fn interview_events(&self) -> Vec<(CalendarEvent, EventDraft)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(e, d)| d.clone().map(|d| (e.clone(), d)))
            .collect()
    }

    fn event(draft: &EventDraft, id: String) -> CalendarEvent {
        CalendarEvent {
            id,
            title: draft.summary.clone(),
            start_at: Some(draft.start_at),
            end_at: Some(draft.end_at),
            all_day: false,
            transparent: draft.cancelled,
            interview_id: Some(draft.interview_id),
        }
    }
}

impl CalendarApi for FakeCalendar {
    fn list_events<'a>(
        &'a self,
        time_min: i64,
        time_max: i64,
    ) -> BoxFuture<'a, AppResult<Vec<CalendarEvent>>> {
        Box::pin(async move {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .map(|(e, _)| e.clone())
                .filter(|e| e.start_at.unwrap() < time_max && time_min < e.end_at.unwrap())
                .collect())
        })
    }

    fn get_event<'a>(&'a self, id: &'a str) -> BoxFuture<'a, AppResult<Option<CalendarEvent>>> {
        Box::pin(async move {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .find(|(e, _)| e.id == id)
                .map(|(e, _)| e.clone()))
        })
    }

    fn find_by_interview<'a>(
        &'a self,
        interview_id: i64,
    ) -> BoxFuture<'a, AppResult<Option<CalendarEvent>>> {
        Box::pin(async move {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .find(|(e, _)| e.interview_id == Some(interview_id))
                .map(|(e, _)| e.clone()))
        })
    }

    fn create_event<'a>(&'a self, draft: &'a EventDraft) -> BoxFuture<'a, AppResult<String>> {
        Box::pin(async move {
            let mut events = self.events.lock().unwrap();
            let id = format!("evt-{}", events.len() + 1);
            events.push((Self::event(draft, id.clone()), Some(draft.clone())));
            self.writes.lock().unwrap().push(format!("create {id}"));
            Ok(id)
        })
    }

    fn update_event<'a>(
        &'a self,
        id: &'a str,
        draft: &'a EventDraft,
    ) -> BoxFuture<'a, AppResult<()>> {
        Box::pin(async move {
            let mut events = self.events.lock().unwrap();
            let slot = events
                .iter_mut()
                .find(|(e, _)| e.id == id)
                .ok_or_else(|| AppError::not_found("no such event"))?;
            *slot = (Self::event(draft, id.to_string()), Some(draft.clone()));
            self.writes.lock().unwrap().push(format!("update {id}"));
            Ok(())
        })
    }
}

/// Answers relevance requests from a set of ids and extraction requests by
/// email subject, and records what it was shown.
#[derive(Default)]
struct ScriptedModel {
    relevant: Mutex<HashSet<String>>,
    extractions: Mutex<HashMap<String, String>>,
    fail_triage: Mutex<bool>,
    requests: Mutex<Vec<String>>,
}

impl ScriptedModel {
    fn relevant(&self, id: &str) {
        self.relevant.lock().unwrap().insert(id.into());
    }

    fn extract(&self, subject: &str, json: serde_json::Value) {
        self.extractions
            .lock()
            .unwrap()
            .insert(subject.into(), json.to_string());
    }

    fn seen(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    fn answer(&self, request: &ChatRequest) -> String {
        let content = &request.turns[0].content;
        if content.contains("Emails:\n") {
            if *self.fail_triage.lock().unwrap() {
                return "I cannot help with that.".into();
            }
            let list = &content[content.find("Emails:\n").unwrap() + 8..];
            let emails: Vec<serde_json::Value> = serde_json::from_str(list).unwrap();
            let relevant = self.relevant.lock().unwrap();
            let ids: Vec<_> = emails
                .iter()
                .filter_map(|e| e["id"].as_str())
                .filter(|id| relevant.contains(*id))
                .collect();
            return serde_json::json!({ "relevant": ids }).to_string();
        }
        let subject = content
            .lines()
            .find_map(|l| l.strip_prefix("Subject: "))
            .unwrap_or_default();
        self.extractions
            .lock()
            .unwrap()
            .get(subject)
            .cloned()
            .unwrap_or_else(|| r#"{"job_related": false}"#.into())
    }
}

impl LanguageModel for ScriptedModel {
    fn list_models<'a>(
        &'a self,
        _endpoint: &'a Endpoint,
    ) -> BoxFuture<'a, AppResult<Vec<FetchedModel>>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn stream_chat<'a>(
        &'a self,
        _endpoint: &'a Endpoint,
        _model_id: &'a str,
        request: &'a ChatRequest,
        _cancel: CancellationToken,
        on_delta: DeltaSink<'a>,
    ) -> BoxFuture<'a, AppResult<Finish>> {
        Box::pin(async move {
            let mut shown = request.system.clone().unwrap_or_default();
            for turn in &request.turns {
                shown.push('\n');
                shown.push_str(&turn.content);
            }
            self.requests.lock().unwrap().push(shown);
            on_delta(&self.answer(request));
            Ok(Finish::Complete)
        })
    }
}

// ── Harness ─────────────────────────────────────────────────────────

struct World {
    state: AppState,
    model: Arc<ScriptedModel>,
    gmail: FakeGmail,
    calendar: FakeCalendar,
}

const CONFIG: JobRunConfig = JobRunConfig {
    lookback_days: 7,
    sync_calendar: true,
    detect_conflicts: true,
};

impl World {
    fn new() -> Self {
        let model = Arc::new(ScriptedModel::default());
        let (state, _) = testing::state(model.clone());
        Self {
            state,
            model,
            gmail: FakeGmail::default(),
            calendar: FakeCalendar::default(),
        }
    }

    async fn run(&self, now: i64) -> JobRunReport {
        let endpoint = Endpoint {
            kind: ProviderKind::Anthropic,
            name: "Anthropic".into(),
            base_url: "http://localhost".into(),
            credential: None,
        };
        let model = ModelRef {
            provider_id: "anthropic".into(),
            model_id: "model-a".into(),
        };
        run(
            &self.state,
            1,
            "Track my applications.",
            RunModel {
                endpoint: &endpoint,
                model: &model,
                max_output_tokens: None,
            },
            CONFIG,
            Tools {
                gmail: &self.gmail,
                calendar: Ok(&self.calendar),
            },
            &CancellationToken::new(),
            now,
        )
        .await
        .unwrap()
    }

    fn count(&self, table: &'static str) -> i64 {
        self.state.db.call(|c| repo::count_rows(c, table)).unwrap()
    }

    fn status_of(&self, report: &JobRunReport, company: &str) -> ApplicationStatus {
        report
            .applications
            .iter()
            .find(|a| a.company == company)
            .unwrap_or_else(|| panic!("{company} is in the overview"))
            .status
    }
}

fn application(company: &str, role: &str, status: &str) -> serde_json::Value {
    serde_json::json!({
        "job_related": true, "company": company, "role": role, "reference": null,
        "status": status, "requires_action": false, "next_action": null,
        "summary": "Update.", "existing_application_id": null, "interview": null
    })
}

fn with_interview(mut value: serde_json::Value, interview: serde_json::Value) -> serde_json::Value {
    value["interview"] = interview;
    value
}

const GLOBEX: &str = "Globex Recruiting <talent@globex.com>";
const CONFIRM_SUBJECT: &str = "Interview confirmation - Data Engineer";
const NEWSLETTER_BODY: &str =
    "New jobs matching your profile this week. PRIVATE-NEWSLETTER-CONTENT that must never reach the model";
const GLOBEX_BODY: &str = "Hi Ana, we are happy to confirm your technical interview on \
    Thursday, 24 September 2026 from 14:00 to 15:00 (Europe/Vienna time). \
    Join here: https://meet.example.com/globex-1 . Kind regards, Globex Talent Team";

/// A mailbox with one of each kind of email.
fn seed(world: &World) {
    let g = &world.gmail;
    let m = &world.model;
    let n = now();
    g.add(
        "m1",
        "t1",
        "Acme Careers <jobs@acme.com>",
        "Your application: Backend Engineer",
        n - 50 * HOUR,
        "Thank you for applying for Backend Engineer. We received your application.",
    );
    g.add(
        "m2",
        "t2",
        "Job Alerts <alerts@jobs.example>",
        "12 new jobs for you",
        n - 40 * HOUR,
        NEWSLETTER_BODY,
    );
    g.add(
        "m3",
        "t3",
        GLOBEX,
        CONFIRM_SUBJECT,
        n - 30 * HOUR,
        GLOBEX_BODY,
    );
    g.add(
        "m4",
        "t4",
        "Initech HR <hr@initech.com>",
        "Your application at Initech",
        n - 20 * HOUR,
        "Unfortunately we decided to move forward with other candidates.",
    );
    g.add(
        "m5",
        "t5",
        "Umbrella <people@umbrella.com>",
        "Interview slots",
        n - 10 * HOUR,
        "Could you do Tuesday or Wednesday afternoon next week? Let us know.",
    );
    g.add(
        "m6",
        "t6",
        "Hooli <jobs@hooli.com>",
        "Interview scheduled",
        n - 5 * HOUR,
        "Your interview is confirmed for 25 September 2026 at 09:00 - 10:00.",
    );
    for id in ["m1", "m3", "m4", "m5", "m6"] {
        m.relevant(id);
    }
    m.extract(
        "Your application: Backend Engineer",
        application("Acme", "Backend Engineer", "confirmed"),
    );
    m.extract(
        CONFIRM_SUBJECT,
        with_interview(
            application("Globex", "Data Engineer", "upcoming_interview"),
            serde_json::json!({
                "state": "confirmed", "date": "2026-09-24", "start_time": "14:00", "end_time": "15:00",
                "duration_minutes": null, "timezone": "Europe/Vienna",
                "datetime_quote": "Thursday, 24 September 2026 from 14:00 to 15:00",
                "timezone_quote": "Europe/Vienna time", "type": "Technical interview",
                "location": null, "meeting_url": "https://meet.example.com/globex-1",
                "participants": [], "interviewer": null, "unclear": null
            }),
        ),
    );
    m.extract(
        "Your application at Initech",
        application("Initech", "QA Engineer", "rejected"),
    );
    m.extract(
        "Interview slots",
        with_interview(
            application("Umbrella", "Platform Engineer", "needs_action"),
            serde_json::json!({ "state": "proposed", "date": null, "start_time": null, "end_time": null,
                "timezone": null, "datetime_quote": null, "timezone_quote": null, "participants": [] }),
        ),
    );
    // The model claims a time zone the email never states.
    m.extract(
        "Interview scheduled",
        with_interview(
            application("Hooli", "SRE", "upcoming_interview"),
            serde_json::json!({
                "state": "confirmed", "date": "2026-09-25", "start_time": "09:00", "end_time": "10:00",
                "timezone": "America/Los_Angeles", "timezone_quote": "Pacific time",
                "datetime_quote": "25 September 2026 at 09:00 - 10:00", "participants": []
            }),
        ),
    );
    // Busy 14:30–15:30 Vienna (12:30–13:30 UTC), overlapping the Globex interview.
    world.calendar.add_busy(
        "dentist",
        "Dentist",
        at("2026-09-24T12:30:00Z"),
        at("2026-09-24T13:30:00Z"),
    );
}

// ── Tests ───────────────────────────────────────────────────────────

#[test]
fn window_covers_the_lookback_and_the_time_since_the_last_success() {
    let now = now();
    assert_eq!(window_start(now, 7, None), now - 7 * DAY_MS);
    // Last success within the lookback: the lookback wins.
    assert_eq!(window_start(now, 7, Some(now - HOUR)), now - 7 * DAY_MS);
    // ReMa was closed for 20 days: catch up from the last success.
    assert_eq!(
        window_start(now, 7, Some(now - 20 * DAY_MS)),
        now - 20 * DAY_MS
    );
    // …but never more than 90 days.
    assert_eq!(
        window_start(now, 7, Some(now - 400 * DAY_MS)),
        now - MAX_WINDOW_DAYS * DAY_MS
    );
}

#[tokio::test]
async fn first_run_classifies_tracks_and_syncs_confirmed_interviews_only() {
    let world = World::new();
    seed(&world);
    let report = world.run(now()).await;

    assert_eq!(report.emails_checked, 6);
    assert_eq!(report.new_emails, 6);
    assert_eq!(report.relevant_emails, 5);
    assert_eq!(report.new_applications, 5);
    assert_eq!(report.applications_updated, 5);
    assert_eq!(report.new_rejections, 1);
    assert_eq!(report.upcoming_interviews, 1);
    assert_eq!(
        report.needs_action, 2,
        "Umbrella (proposed) and Hooli (unclear)"
    );
    assert!(report.issues.is_empty(), "{:?}", report.issues);

    assert_eq!(
        world.status_of(&report, "Acme"),
        ApplicationStatus::Confirmed
    );
    assert_eq!(
        world.status_of(&report, "Globex"),
        ApplicationStatus::UpcomingInterview
    );
    assert_eq!(
        world.status_of(&report, "Initech"),
        ApplicationStatus::Rejected
    );
    assert_eq!(
        world.status_of(&report, "Umbrella"),
        ApplicationStatus::NeedsAction
    );
    assert_eq!(
        world.status_of(&report, "Hooli"),
        ApplicationStatus::NeedsAction
    );
    let globex = report
        .applications
        .iter()
        .find(|a| a.company == "Globex")
        .unwrap();
    assert_eq!(globex.interview_at, Some(at("2026-09-24T12:00:00Z")));
    let hooli = report
        .applications
        .iter()
        .find(|a| a.company == "Hooli")
        .unwrap();
    assert!(hooli.next_action.as_deref().unwrap().contains("time zone"));

    // Only the confirmed, fully stated interview became an event.
    let calendar = report.calendar.as_ref().unwrap();
    assert_eq!(
        (calendar.created, calendar.updated, calendar.unchanged),
        (1, 0, 0)
    );
    assert_eq!(calendar.needs_review, 1, "Hooli");
    assert_eq!(calendar.conflicts, 1);
    let events = world.calendar.interview_events();
    assert_eq!(events.len(), 1);
    let (event, draft) = &events[0];
    assert_eq!(event.title, "Data Engineer Interview — Globex");
    assert_eq!(draft.start_at, at("2026-09-24T12:00:00Z"));
    assert_eq!(draft.end_at, at("2026-09-24T13:00:00Z"));
    assert_eq!(draft.timezone, "Europe/Vienna");
    assert!(draft
        .description
        .contains("Meeting: https://meet.example.com/globex-1"));
    assert!(
        !draft.description.contains("Hi Ana"),
        "no email content in the event"
    );
    let created = calendar
        .items
        .iter()
        .find(|i| i.outcome == CalendarOutcome::Created)
        .unwrap();
    assert_eq!(created.conflicts[0].title, "Dentist");

    // Privacy: the newsletter was never read in full nor shown to the model.
    assert!(!world
        .gmail
        .full_reads
        .lock()
        .unwrap()
        .contains(&"m2".to_string()));
    let seen = world.model.seen();
    assert_eq!(seen.len(), 6, "one relevance request + five extractions");
    assert!(seen
        .iter()
        .all(|r| !r.contains("PRIVATE-NEWSLETTER-CONTENT")));
    // The relevance request carries snippets, not bodies.
    assert!(!seen[0].contains("meet.example.com/globex-1"));
}

#[tokio::test]
async fn repeated_runs_change_nothing_and_ask_the_model_nothing() {
    let world = World::new();
    seed(&world);
    world.run(now()).await;
    let requests = world.model.seen().len();
    let writes = world.calendar.writes.lock().unwrap().len();

    for hour in 1..=3 {
        let report = world.run(now() + hour * HOUR).await;
        assert_eq!(report.emails_checked, 6);
        assert_eq!(report.new_emails, 0);
        assert_eq!(report.relevant_emails, 0);
        assert_eq!(report.new_applications, 0);
        assert_eq!(report.new_rejections, 0);
        let calendar = report.calendar.unwrap();
        assert_eq!(
            (calendar.created, calendar.updated, calendar.unchanged),
            (0, 0, 1)
        );
        assert_eq!(report.applications.len(), 5, "still in the overview");
    }
    assert_eq!(
        world.model.seen().len(),
        requests,
        "no email was sent to the model again"
    );
    assert_eq!(
        world.calendar.writes.lock().unwrap().len(),
        writes,
        "no Calendar writes"
    );
    assert_eq!(world.calendar.interview_events().len(), 1);
    assert_eq!(world.count("job_applications"), 5);
    assert_eq!(
        world.count("interviews"),
        3,
        "Globex, Umbrella (proposed), Hooli (review)"
    );
    assert_eq!(world.count("mail_messages"), 6);
}

#[tokio::test]
async fn a_follow_up_in_the_thread_updates_the_same_application() {
    let world = World::new();
    seed(&world);
    world.run(now()).await;
    world.gmail.add(
        "m7",
        "t1",
        "Acme Careers <jobs@acme.com>",
        "Next steps",
        now() + HOUR,
        "Please complete the coding assessment by Friday.",
    );
    world.model.relevant("m7");
    // The company name differs in spelling; the thread links it.
    let mut update = application("ACME GmbH", "Backend Engineer", "needs_action");
    update["next_action"] = "Complete the coding assessment by Friday".into();
    world.model.extract("Next steps", update);

    let report = world.run(now() + 2 * HOUR).await;
    assert_eq!(report.new_applications, 0);
    assert_eq!(report.applications_updated, 1);
    assert_eq!(world.count("job_applications"), 5);
    let acme = report
        .applications
        .iter()
        .find(|a| a.company == "Acme")
        .unwrap();
    assert_eq!(acme.status, ApplicationStatus::NeedsAction);
    assert_eq!(
        acme.next_action.as_deref(),
        Some("Complete the coding assessment by Friday")
    );
}

#[tokio::test]
async fn reschedule_and_cancellation_update_the_same_event() {
    let world = World::new();
    seed(&world);
    world.run(now()).await;
    let event_id = world.calendar.interview_events()[0].0.id.clone();

    world.gmail.add(
        "m8",
        "t3",
        GLOBEX,
        "Re: Interview confirmation - Data Engineer",
        now() + HOUR,
        "We need to move your interview. New time: Friday, 25 September 2026 from 10:00 to 11:00 \
         (Europe/Vienna time). Same link as before.",
    );
    world.model.relevant("m8");
    world.model.extract(
        "Re: Interview confirmation - Data Engineer",
        with_interview(
            application("Globex", "Data Engineer", "upcoming_interview"),
            serde_json::json!({
                "state": "rescheduled", "date": "2026-09-25", "start_time": "10:00", "end_time": "11:00",
                "timezone": "Europe/Vienna", "timezone_quote": "Europe/Vienna time",
                "datetime_quote": "Friday, 25 September 2026 from 10:00 to 11:00", "participants": []
            }),
        ),
    );
    let report = world.run(now() + 2 * HOUR).await;
    let calendar = report.calendar.unwrap();
    assert_eq!((calendar.created, calendar.updated), (0, 1));
    assert_eq!(calendar.conflicts, 0, "the new time is free");
    let events = world.calendar.interview_events();
    assert_eq!(events.len(), 1, "moved, not duplicated");
    let (event, draft) = &events[0];
    assert_eq!(event.id, event_id);
    assert_eq!(draft.start_at, at("2026-09-25T08:00:00Z"));
    assert!(
        draft.description.contains("meet.example.com/globex-1"),
        "link kept from the earlier email"
    );

    world.gmail.add(
        "m9",
        "t3",
        GLOBEX,
        "Interview cancelled",
        now() + 3 * HOUR,
        "Unfortunately we have to cancel your interview. We will get back to you.",
    );
    world.model.relevant("m9");
    world.model.extract(
        "Interview cancelled",
        with_interview(
            application("Globex", "Data Engineer", "in_process"),
            serde_json::json!({ "state": "cancelled", "participants": [] }),
        ),
    );
    let report = world.run(now() + 4 * HOUR).await;
    assert_eq!(report.calendar.as_ref().unwrap().cancelled, 1);
    assert_eq!(
        world.status_of(&report, "Globex"),
        ApplicationStatus::InProcess
    );
    let events = world.calendar.interview_events();
    assert_eq!(events.len(), 1, "kept, not deleted");
    assert_eq!(events[0].0.id, event_id);
    assert!(events[0].0.title.starts_with("Cancelled: "));
    assert!(events[0].0.transparent, "no longer blocks the time");

    // History is preserved.
    let interview = world
        .state
        .db
        .call(|c| Ok(repo::interview_from_message(c, "m8")?.unwrap()))
        .unwrap();
    assert_eq!(interview.state, InterviewState::Cancelled);
    let history: Vec<String> = world
        .state
        .db
        .call(|c| repo::interview_history(c, interview.id))
        .unwrap()
        .into_iter()
        .map(|(change, _)| change)
        .collect();
    assert_eq!(
        history,
        [
            "confirmed",
            "calendar_created",
            "rescheduled",
            "calendar_updated",
            "cancelled",
            "calendar_cancelled"
        ]
    );

    // And once handled, the cancellation is quiet.
    let report = world.run(now() + 5 * HOUR).await;
    let calendar = report.calendar.unwrap();
    assert_eq!(
        (calendar.created, calendar.updated, calendar.cancelled),
        (0, 0, 0)
    );
    assert_eq!(world.calendar.interview_events().len(), 1);
}

#[tokio::test]
async fn a_lost_event_id_never_creates_a_duplicate() {
    let world = World::new();
    seed(&world);
    world.run(now()).await;
    // Simulate a crash between creating the event and saving its id.
    world
        .state
        .db
        .call(|c| {
            c.execute(
                "UPDATE interviews SET calendar_event_id = NULL, calendar_hash = NULL",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let report = world.run(now() + HOUR).await;
    assert_eq!(report.calendar.unwrap().created, 0);
    assert_eq!(world.calendar.interview_events().len(), 1);
}

#[tokio::test]
async fn events_deleted_by_the_user_are_not_recreated() {
    let world = World::new();
    seed(&world);
    world.run(now()).await;
    world
        .calendar
        .events
        .lock()
        .unwrap()
        .retain(|(_, draft)| draft.is_none());
    let report = world.run(now() + HOUR).await;
    let calendar = report.calendar.unwrap();
    assert_eq!(calendar.created, 0);
    assert!(calendar
        .items
        .iter()
        .any(|i| i.outcome == CalendarOutcome::Removed));
    assert!(world.calendar.interview_events().is_empty());
}

#[tokio::test]
async fn failed_relevance_checks_are_retried_next_run() {
    let world = World::new();
    seed(&world);
    *world.model.fail_triage.lock().unwrap() = true;
    let report = world.run(now()).await;
    assert_eq!(report.relevant_emails, 0);
    assert_eq!(report.issues.len(), 1);
    assert_eq!(
        world.count("mail_messages"),
        0,
        "nothing recorded as handled"
    );

    *world.model.fail_triage.lock().unwrap() = false;
    let report = world.run(now() + HOUR).await;
    assert_eq!(report.new_emails, 6);
    assert_eq!(report.new_applications, 5);
}

#[tokio::test]
async fn calendar_is_untouched_when_sync_is_off() {
    let world = World::new();
    seed(&world);
    let endpoint = Endpoint {
        kind: ProviderKind::Anthropic,
        name: "Anthropic".into(),
        base_url: "http://localhost".into(),
        credential: None,
    };
    let model = ModelRef {
        provider_id: "anthropic".into(),
        model_id: "model-a".into(),
    };
    let report = run(
        &world.state,
        1,
        "",
        RunModel {
            endpoint: &endpoint,
            model: &model,
            max_output_tokens: None,
        },
        JobRunConfig {
            sync_calendar: false,
            ..CONFIG
        },
        Tools {
            gmail: &world.gmail,
            calendar: Ok(&world.calendar),
        },
        &CancellationToken::new(),
        now(),
    )
    .await
    .unwrap();
    assert!(report.calendar.is_none());
    assert!(world.calendar.writes.lock().unwrap().is_empty());
    assert_eq!(
        world.status_of(&report, "Globex"),
        ApplicationStatus::UpcomingInterview
    );
}
