//! ReMa Agents: built-in and custom instruction profiles, and how the
//! selected ones join a chat request.
//!
//! Selected agents are added to the system prompt exactly once each, in the
//! order the user selected them. Selecting an agent never turns on Profile
//! context or any MCP server; those stay separate choices.

use crate::{
    db::agents as repo,
    error::{AppError, AppResult},
    models::agent::{Agent, AgentInput, AGENT_ICONS},
    services::profile::{line, text},
    state::AppState,
    time::now_ms,
};

const MAX_NAME: usize = 80;
const MAX_DESCRIPTION: usize = 300;
const MAX_INSTRUCTIONS: usize = 12_000;
/// Most agents one request can combine.
pub const MAX_SELECTED: usize = 8;

struct Builtin {
    slug: &'static str,
    name: &'static str,
    description: &'static str,
    icon: &'static str,
    instructions: &'static str,
}

const BUILTINS: &[Builtin] = &[
    Builtin {
        slug: "job-search",
        name: "Job Search Agent",
        description: "Finds and analyzes openings that fit your explicit criteria.",
        icon: "search",
        instructions: "\
You find and analyze job openings for the user.
- Treat the user's explicit constraints (role, location, work mode, seniority, salary, company type, dates) as hard filters. Say so when a result does not meet one.
- Separate verified facts from the posting or a source from your own assumptions; label assumptions.
- Give the direct link to each posting when you have one. Never invent links.
- Never invent salaries, locations, seniority, work mode or employment conditions; write \"not stated\" instead.
- Compare roles with the user's Profile only if Profile context is included in this conversation.",
    },
    Builtin {
        slug: "job-match",
        name: "Job Match Analyst",
        description: "Compares job descriptions with your Profile: matches and real gaps.",
        icon: "match",
        instructions: "\
You evaluate one or more job descriptions against the user's background.
- List the strongest matches, each with evidence from the job description and from the user's Profile or messages.
- List genuine gaps in skills or experience. Distinguish required from preferred requirements.
- Quote or cite the exact requirement you assess. Never invent experience the user has not shown.
- If Profile context is not included, ask for the missing background or assess only what the user provided.
- End with a short, honest overall fit assessment.",
    },
    Builtin {
        slug: "cv-tailoring",
        name: "CV Tailoring Agent",
        description: "Adapts your CV to a specific role without inventing anything.",
        icon: "document",
        instructions: "\
You adapt the user's CV or portfolio to a specific role.
- Improve wording, ordering and relevance for the target role and its keywords.
- Preserve factual accuracy. Never fabricate employers, dates, titles, skills, achievements, degrees, certifications or metrics.
- When a stronger statement would need information you do not have, mark it as [needs confirmation] and ask.
- Show changes clearly, e.g. as rewritten sections the user can copy.",
    },
    Builtin {
        slug: "interview-prep",
        name: "Interview Prep Agent",
        description: "Prepares you for a specific interview with focused practice.",
        icon: "interview",
        instructions: "\
You prepare the user for a specific interview.
- Derive likely topics from the role, company and interview details the user provides.
- Give focused technical and behavioral preparation, realistic practice questions and concise answer frameworks (e.g. STAR).
- Use the user's Profile for examples only when Profile context is included.
- Never claim to know a company's private or undocumented interview questions; say when topics are inferred.",
    },
    Builtin {
        slug: "application-strategist",
        name: "Application Strategist",
        description: "Plans next steps across your active applications.",
        icon: "strategy",
        instructions: "\
You help the user plan next actions across their job opportunities.
- Compare requirements, deadlines, status and next actions; prioritize by the user's own criteria and say which criteria you used.
- Propose concrete, dated next steps.
- Never state or imply that an application was submitted, a recruiter was contacted or any action happened unless the user or ReMa's data says so.",
    },
    Builtin {
        slug: "career-research",
        name: "Career Research Agent",
        description: "Researches roles, skills, companies, salaries and career paths.",
        icon: "research",
        instructions: "\
You research roles, skills, companies, salary ranges, technologies and career paths.
- Distinguish current, sourced information from general knowledge, and cite sources when tools give you any.
- Prefer recent, reliable information when search or web tools are available; note the date of time-sensitive facts.
- State uncertainty clearly and give ranges rather than false precision.
- Use the user's Profile only when Profile context is included.",
    },
];

fn builtin_id(slug: &str) -> String {
    format!("builtin:{slug}")
}

fn builtin(b: &Builtin) -> Agent {
    Agent {
        id: builtin_id(b.slug),
        name: b.name.into(),
        description: b.description.into(),
        instructions: b.instructions.into(),
        icon: b.icon.into(),
        builtin: true,
        updated_at: None,
    }
}

/// Built-in agents (in their fixed order), then custom agents by name.
pub fn list(state: &AppState) -> AppResult<Vec<Agent>> {
    let mut agents: Vec<Agent> = BUILTINS.iter().map(builtin).collect();
    agents.extend(state.db.call(|c| repo::list(c))?);
    Ok(agents)
}

fn custom_number(id: &str) -> Option<i64> {
    id.strip_prefix("custom:")?.parse().ok()
}

/// Looks up one agent by id.
pub fn get(state: &AppState, id: &str) -> AppResult<Agent> {
    if let Some(slug) = id.strip_prefix("builtin:") {
        return BUILTINS
            .iter()
            .find(|b| b.slug == slug)
            .map(builtin)
            .ok_or_else(|| AppError::not_found("Agent not found"));
    }
    let number = custom_number(id).ok_or_else(|| AppError::not_found("Agent not found"))?;
    state.db.call(|c| repo::get(c, number))
}

fn normalize(input: AgentInput) -> AppResult<AgentInput> {
    let name = line(&input.name, MAX_NAME, "Name")?;
    if name.is_empty() {
        return Err(AppError::validation("Give the agent a name."));
    }
    let instructions = text(&input.instructions, MAX_INSTRUCTIONS, "Instructions")?;
    if instructions.is_empty() {
        return Err(AppError::validation("Write the agent's instructions."));
    }
    let icon = if AGENT_ICONS.contains(&input.icon.as_str()) {
        input.icon
    } else {
        "spark".into()
    };
    Ok(AgentInput {
        name,
        description: line(&input.description, MAX_DESCRIPTION, "Description")?,
        instructions,
        icon,
    })
}

/// Creates (`id` = `None`) or updates a custom agent.
pub fn save(state: &AppState, id: Option<i64>, input: AgentInput) -> AppResult<Agent> {
    let input = normalize(input)?;
    let agent = state.db.call(|c| {
        let id = match id {
            Some(id) => {
                repo::update(c, id, &input, now_ms())?;
                id
            }
            None => repo::insert(c, &input, now_ms())?,
        };
        repo::get(c, id)
    })?;
    state.events.agents_changed();
    Ok(agent)
}

/// A custom copy of any agent (the way to change a built-in one).
pub fn duplicate(state: &AppState, id: &str) -> AppResult<Agent> {
    let original = get(state, id)?;
    let name: String = format!("{} (copy)", original.name)
        .chars()
        .take(MAX_NAME)
        .collect();
    save(
        state,
        None,
        AgentInput {
            name,
            description: original.description,
            instructions: original.instructions,
            icon: original.icon,
        },
    )
}

pub fn delete(state: &AppState, id: i64) -> AppResult<()> {
    state.db.call(|c| {
        let tx = c.transaction()?;
        repo::delete(&tx, id)?;
        tx.commit()?;
        Ok(())
    })?;
    state.events.agents_changed();
    state.events.conversations_changed();
    Ok(())
}

/// Checks a selection: known agents, each once, in the given order.
pub fn resolve(state: &AppState, ids: &[String]) -> AppResult<Vec<Agent>> {
    let mut agents: Vec<Agent> = Vec::new();
    for id in ids {
        if agents.iter().any(|a| &a.id == id) {
            continue;
        }
        match get(state, id) {
            Ok(agent) => agents.push(agent),
            Err(AppError::NotFound(_)) => {
                return Err(AppError::validation(
                    "An agent you selected no longer exists. Remove it and try again.",
                ))
            }
            Err(error) => return Err(error),
        }
    }
    if agents.len() > MAX_SELECTED {
        return Err(AppError::validation(format!(
            "Select at most {MAX_SELECTED} agents."
        )));
    }
    Ok(agents)
}

/// The selected agents' instructions for the system prompt, each exactly
/// once in selection order. `None` without agents.
pub fn compose(agents: &[Agent]) -> Option<String> {
    let mut seen: Vec<&str> = Vec::new();
    let mut blocks: Vec<String> = Vec::new();
    for agent in agents {
        if seen.contains(&agent.id.as_str()) {
            continue;
        }
        seen.push(&agent.id);
        // Instructions cannot close their own block.
        let instructions = agent.instructions.replace("</agent>", "</ agent>");
        let name = agent.name.replace('"', "'");
        blocks.push(format!("<agent name=\"{name}\">\n{instructions}\n</agent>"));
    }
    if blocks.is_empty() {
        return None;
    }
    let intro = if blocks.len() == 1 {
        "The user selected this ReMa Agent for the conversation. Follow its instructions."
            .to_string()
    } else {
        format!(
            "The user selected these {} ReMa Agents for the conversation, in this order. Follow \
             all of their instructions. If two conflict, follow the user's explicit request, \
             then the agent listed first.",
            blocks.len()
        )
    };
    Some(format!("{intro}\n\n{}", blocks.join("\n\n")))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{llm::fake::FakeLanguageModel, state::testing};

    fn state() -> AppState {
        testing::state(Arc::new(FakeLanguageModel::replying(&[]))).0
    }

    fn input(name: &str, instructions: &str) -> AgentInput {
        AgentInput {
            name: name.into(),
            instructions: instructions.into(),
            ..AgentInput::default()
        }
    }

    #[test]
    fn ships_six_stable_builtin_agents() {
        let state = state();
        let agents = list(&state).unwrap();
        let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Job Search Agent",
                "Job Match Analyst",
                "CV Tailoring Agent",
                "Interview Prep Agent",
                "Application Strategist",
                "Career Research Agent"
            ]
        );
        assert!(agents
            .iter()
            .all(|a| a.builtin && !a.instructions.is_empty()));
        assert!(get(&state, "builtin:unknown").is_err());
    }

    #[test]
    fn custom_agents_can_be_created_edited_duplicated_and_deleted() {
        let state = state();
        let agent = save(
            &state,
            None,
            input("  Recruiter  Voice ", "Write like a recruiter."),
        )
        .unwrap();
        assert_eq!(agent.name, "Recruiter Voice");
        assert_eq!(agent.icon, "spark");
        assert!(!agent.builtin);
        let number: i64 = agent.id.strip_prefix("custom:").unwrap().parse().unwrap();

        let edited = save(
            &state,
            Some(number),
            AgentInput {
                icon: "target".into(),
                ..input("Recruiter", "Be concise.")
            },
        )
        .unwrap();
        assert_eq!(edited.id, agent.id);
        assert_eq!(edited.instructions, "Be concise.");
        assert_eq!(edited.icon, "target");

        // A built-in agent is changed by duplicating it.
        let copy = duplicate(&state, "builtin:cv-tailoring").unwrap();
        assert_eq!(copy.name, "CV Tailoring Agent (copy)");
        assert!(!copy.builtin);
        assert_eq!(list(&state).unwrap().len(), 8);

        delete(&state, number).unwrap();
        assert!(get(&state, &agent.id).is_err());
        assert_eq!(list(&state).unwrap().len(), 7);

        assert!(save(&state, None, input("", "x")).is_err());
        assert!(save(&state, None, input("Name", "  ")).is_err());
    }

    #[test]
    fn composes_selected_instructions_once_in_order() {
        let state = state();
        assert_eq!(compose(&[]), None, "no agent, no instructions");

        let ids = [
            "builtin:job-match".to_string(),
            "builtin:cv-tailoring".to_string(),
            "builtin:job-match".to_string(),
        ];
        let agents = resolve(&state, &ids).unwrap();
        assert_eq!(agents.len(), 2, "duplicates are dropped");
        let prompt = compose(&agents).unwrap();
        let match_at = prompt.find("<agent name=\"Job Match Analyst\">").unwrap();
        let tailor_at = prompt.find("<agent name=\"CV Tailoring Agent\">").unwrap();
        assert!(match_at < tailor_at, "selection order is kept");
        assert_eq!(prompt.matches("<agent name=").count(), 2);
        assert_eq!(
            prompt
                .matches("You evaluate one or more job descriptions")
                .count(),
            1
        );

        // Reversed selection, reversed order.
        let reversed =
            compose(&resolve(&state, &[ids[1].clone(), ids[0].clone()]).unwrap()).unwrap();
        assert!(reversed.find("CV Tailoring").unwrap() < reversed.find("Job Match").unwrap());

        assert!(resolve(&state, &["custom:999".into()]).is_err());
        let too_many: Vec<String> = (0..9)
            .map(|i| save(&state, None, input(&format!("A{i}"), "x")).unwrap().id)
            .collect();
        assert!(resolve(&state, &too_many).is_err());
    }
}
