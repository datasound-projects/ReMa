//! End-to-end tests of the analytics engine at the service level: the user
//! scenarios A–J plus automatic ingestion from chat, scheduled tasks and
//! history, and learning research.

use std::{sync::Arc, time::Duration};

use tokio_util::sync::CancellationToken;

use super::*;
use crate::{
    analytics::ingest::{ingest, RawJob, RawJobSearch},
    db::{analytics as repo, conversations, tasks as task_repo},
    llm::fake::FakeLanguageModel,
    models::{
        analytics::{
            CellState, DataScope, FilterCondition, FilterField, FilterOp, MatchState,
            PriorityLevel, RunSource, ScopeKind,
        },
        chat::{MessageRole, MessageStatus, SendMessageInput},
        profile::{Experience, Language, Profile},
        provider::{ModelRef, ProviderKind},
        task::{EndCondition, ExecutionTrigger, IntervalUnit, Schedule, TaskInput, TaskKind},
    },
    services::{chat, profile as profile_service, scheduler, tasks},
    state::testing,
};

const DAY: i64 = 86_400_000;
/// 2025-09-25T00:00:00Z
const SEP_25: i64 = 1_758_758_400_000;

#[allow(clippy::too_many_arguments)]
fn job(
    title: &str,
    company: &str,
    location: &str,
    mode: &str,
    salary: &str,
    posted: &str,
    url: &str,
    skills: &[&str],
) -> RawJob {
    let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
    RawJob {
        title: title.into(),
        company: opt(company),
        location: opt(location),
        work_mode: opt(mode),
        salary: opt(salary),
        posted: opt(posted),
        url: opt(url),
        requirements: skills.iter().map(|s| s.to_string()).collect(),
        ..RawJob::default()
    }
}

fn run(origin: &str, title: &str, created_at: i64, jobs: Vec<RawJob>) -> RawJobSearch {
    RawJobSearch {
        origin: origin.into(),
        title: title.into(),
        query: title.into(),
        source: RunSource::Tool,
        task_id: None,
        conversation_id: None,
        message_id: None,
        execution_id: None,
        created_at,
        jobs,
    }
}

/// Three searches; "AI Engineer at Company A" and "Data Scientist at
/// Globex" are each found twice.
fn seed(state: &AppState) -> (i64, i64, i64) {
    let j1 = || {
        job(
            "AI Engineer",
            "Company A GmbH",
            "Vienna, Austria",
            "Remote",
            "€90k–110k",
            "2025-09-20",
            "https://boards.greenhouse.io/companya/jobs/1000001",
            &["Python", "Kubernetes", "AWS", "Terraform"],
        )
    };
    let j3 = || {
        job(
            "Data Scientist",
            "Globex",
            "Vienna, Austria",
            "On-site",
            "Not stated",
            "n/a",
            "",
            &["Python", "SQL", "German C1"],
        )
    };
    let r1 = ingest(
        state,
        run(
            "t:1",
            "Austria AI Jobs",
            SEP_25,
            vec![
                j1(),
                job(
                    "AI Platform Engineer",
                    "Initech",
                    "Graz, Austria",
                    "Hybrid",
                    "€120k–140k",
                    "2025-09-23",
                    "https://initech.example/jobs/ai-platform-20001",
                    &["Python", "Kubernetes", "Terraform", "Helm", "Ray serve"],
                ),
                j3(),
            ],
        ),
    )
    .unwrap()
    .unwrap();
    let r2 = ingest(
        state,
        run(
            "t:2",
            "Remote EU AI Jobs",
            SEP_25 - DAY,
            vec![
                RawJob {
                    url: Some(
                        "https://boards.greenhouse.io/companya/jobs/1000001?gh_src=li".into(),
                    ),
                    ..j1()
                },
                job(
                    "ML Engineer",
                    "Umbrella",
                    "Berlin, Germany",
                    "Remote",
                    "$150k",
                    "2025-09-10",
                    "https://umbrella.example/careers/ml-30003",
                    &["PyTorch", "Python", "AWS"],
                ),
                job(
                    "Senior AI Engineer",
                    "Hooli",
                    "Remote (EU)",
                    "Remote",
                    "€105k",
                    "",
                    "https://hooli.example/jobs/40004",
                    &["Python", "LangGraph", "AWS", "Terraform"],
                ),
            ],
        ),
    )
    .unwrap()
    .unwrap();
    let r3 = ingest(
        state,
        run(
            "t:3",
            "Vienna Data",
            SEP_25 - 5 * DAY,
            vec![
                j3(),
                job(
                    "AI Platform Engineer",
                    "Vandelay",
                    "Vienna, Austria",
                    "Remote",
                    "€100k–130k",
                    "",
                    "https://vandelay.example/jobs/50005",
                    &["Kubernetes", "Terraform", "GCP"],
                ),
                job(
                    "Junior Data Analyst",
                    "Stark",
                    "Linz, Austria",
                    "Hybrid",
                    "€45k",
                    "",
                    "https://stark.example/jobs/60006",
                    &["SQL", "Excel"],
                ),
            ],
        ),
    )
    .unwrap()
    .unwrap();
    (r1.run_id, r2.run_id, r3.run_id)
}

fn with_profile(state: &AppState) {
    profile_service::save(
        state,
        Profile {
            title: "Data Engineer".into(),
            skills: vec![
                "Python".into(),
                "Kubernetes".into(),
                "Azure".into(),
                "SQL".into(),
            ],
            experience: vec![Experience {
                title: "Data Engineer".into(),
                company: "Globex".into(),
                start: "2018".into(),
                current: true,
                ..Default::default()
            }],
            languages: vec![Language {
                name: "German".into(),
                level: "Native".into(),
            }],
            ..Default::default()
        },
    )
    .unwrap();
}

fn new_state(reply: &[&str]) -> AppState {
    testing::state(Arc::new(FakeLanguageModel::replying(reply))).0
}

fn query(
    scope: DataScope,
    filters: Vec<FilterCondition>,
    ranking: Vec<SortCriterion>,
) -> AnalyticsQuery {
    AnalyticsQuery {
        scope,
        filters,
        ranking,
        limit: None,
    }
}

fn searches(ids: &[i64]) -> DataScope {
    DataScope {
        kind: ScopeKind::Searches,
        run_ids: ids.to_vec(),
        job_ids: Vec::new(),
    }
}

fn cond(field: FilterField, op: FilterOp, values: &[&str], number: Option<f64>) -> FilterCondition {
    FilterCondition {
        field,
        op,
        values: values.iter().map(|v| v.to_string()).collect(),
        number,
        currency: None,
    }
}

fn titles(o: &AnalyticsOverview) -> Vec<String> {
    o.jobs
        .iter()
        .map(|j| format!("{} @ {}", j.title, j.company.clone().unwrap_or_default()))
        .collect()
}

fn job_id(state: &AppState, company: &str) -> i64 {
    state
        .db
        .call(|c| {
            Ok(c.query_row(
                "SELECT id FROM jobs WHERE company LIKE ?1",
                [format!("{company}%")],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

#[test]
fn scenario_a_everything_is_analyzed_once() {
    let state = new_state(&[]);
    seed(&state);
    let o = overview(&state, &default_preferences().query).unwrap();
    assert_eq!(o.summary.analyzed, 7, "{:?}", titles(&o));
    assert_eq!(o.summary.runs, 3);
    assert_eq!(o.summary.duplicates_merged, 2, "two jobs were found twice");
    let a = o
        .jobs
        .iter()
        .find(|j| j.company.as_deref() == Some("Company A GmbH"))
        .unwrap();
    assert_eq!(a.appearances, 2);
    // Default ranking: newest posting first, unknown posting dates last.
    assert_eq!(o.jobs[0].title, "AI Platform Engineer");
    assert!(o.jobs[4..].iter().all(|j| j.date_posted.is_none()));
    assert_eq!(runs(&state).unwrap().len(), 3);
}

#[test]
fn scenarios_b_c_d_select_searches_and_jobs() {
    let state = new_state(&[]);
    let (r1, r2, _) = seed(&state);
    let one = overview(&state, &query(searches(&[r1]), vec![], vec![])).unwrap();
    assert_eq!(one.summary.analyzed, 3);
    assert_eq!(one.summary.label, "Austria AI Jobs (25 Sep)");
    let two = overview(&state, &query(searches(&[r1, r2]), vec![], vec![])).unwrap();
    assert_eq!(
        two.summary.analyzed, 5,
        "the job found by both searches counts once"
    );
    assert_eq!(two.summary.duplicates_merged, 1);
    let ids = vec![job_id(&state, "Initech"), job_id(&state, "Vandelay")];
    let picked = overview(
        &state,
        &query(
            DataScope {
                kind: ScopeKind::Jobs,
                run_ids: vec![],
                job_ids: ids,
            },
            vec![],
            vec![],
        ),
    )
    .unwrap();
    assert_eq!(picked.summary.analyzed, 2);
    assert_eq!(picked.summary.label, "2 selected jobs");
}

#[test]
fn scenario_e_high_salary_ranking_and_gap() {
    let state = new_state(&[]);
    seed(&state);
    with_profile(&state);
    let mut salary = cond(FilterField::Salary, FilterOp::AtLeast, &[], Some(100_000.0));
    salary.currency = Some("EUR".into());
    let q = query(
        DataScope::default(),
        vec![salary],
        vec![SortCriterion {
            key: SortKey::Salary,
            direction: SortDirection::Desc,
            value: None,
        }],
    );
    let o = overview(&state, &q).unwrap();
    assert_eq!(
        titles(&o),
        [
            "AI Platform Engineer @ Initech",
            "AI Platform Engineer @ Vandelay",
            "Senior AI Engineer @ Hooli",
            "AI Engineer @ Company A GmbH"
        ],
        "EUR salaries ≥ 100k (range midpoints), highest first; USD and missing salaries excluded"
    );
    let gap = skill_gap(&state, &q, &SkillGapOptions::default()).unwrap();
    assert_eq!(gap.jobs, 4);
    let terraform = gap.demand.iter().find(|d| d.name == "Terraform").unwrap();
    assert_eq!((terraform.jobs, terraform.percent), (4, 100.0));
    assert_eq!(terraform.state, Some(MatchState::Missing));
    let top = &gap.priorities[0];
    assert_eq!(
        (top.name.as_str(), top.level),
        ("Terraform", PriorityLevel::High)
    );
    assert!(
        top.reasons[0].starts_with("Required by 4 of 4 jobs (100%)"),
        "{:?}",
        top.reasons
    );
    let aws = gap.priorities.iter().find(|p| p.name == "AWS").unwrap();
    assert_eq!(aws.state, MatchState::Partial, "Azure is a related cloud");
    assert!(
        gap.summary
            .iter()
            .any(|s| s == "Highest-impact gaps: Terraform, AWS, Helm"),
        "{:?}",
        gap.summary
    );
}

#[test]
fn scenario_f_remote_austria_feeds_every_view() {
    let state = new_state(&[]);
    seed(&state);
    with_profile(&state);
    let q = query(
        DataScope::default(),
        vec![
            cond(FilterField::Country, FilterOp::Is, &["Austria"], None),
            cond(FilterField::WorkMode, FilterOp::Is, &["remote"], None),
        ],
        vec![],
    );
    let o = overview(&state, &q).unwrap();
    let mut t = titles(&o);
    t.sort();
    assert_eq!(
        t,
        [
            "AI Engineer @ Company A GmbH",
            "AI Platform Engineer @ Vandelay"
        ]
    );
    assert_eq!(
        o.summary.label,
        "All job searches · Country is Austria · Work mode is Remote"
    );
    let reqs = requirements(&state, &q, &RequirementTableQuery::default()).unwrap();
    assert_eq!(reqs.jobs, 2);
    let k8s = reqs.rows.iter().find(|r| r.name == "Kubernetes").unwrap();
    assert_eq!((k8s.jobs, k8s.percent), (2, 100.0));
    let learn = learning(
        &state,
        &q,
        &LearningCriteria::default(),
        &SkillGapOptions::default(),
    )
    .unwrap();
    assert_eq!(learn.jobs, 2);
    assert_eq!(learn.gaps[0].name, "Terraform");
    assert!(
        learn.gaps.iter().all(|g| g.jobs >= 2),
        "single mentions are ignored by default"
    );
}

#[test]
fn scenario_g_and_h_role_and_single_job() {
    let state = new_state(&[]);
    seed(&state);
    with_profile(&state);
    let role = query(
        DataScope::default(),
        vec![cond(
            FilterField::Role,
            FilterOp::Is,
            &["AI Platform Engineer"],
            None,
        )],
        vec![],
    );
    let reqs = requirements(&state, &role, &RequirementTableQuery::default()).unwrap();
    assert_eq!(reqs.jobs, 2);
    assert!(
        reqs.summary
            .iter()
            .any(|s| s.starts_with("Most common: Kubernetes, Terraform")),
        "{:?}",
        reqs.summary
    );
    let gap = skill_gap(&state, &role, &SkillGapOptions::default()).unwrap();
    assert_eq!(gap.priorities[0].name, "Terraform");

    let one = query(
        DataScope {
            kind: ScopeKind::Jobs,
            run_ids: vec![],
            job_ids: vec![job_id(&state, "Initech")],
        },
        vec![],
        vec![],
    );
    let gap = skill_gap(&state, &one, &SkillGapOptions::default()).unwrap();
    let job = gap.job.expect("a single job shows its own gap");
    let missing: Vec<&str> = job
        .requirements
        .iter()
        .filter(|r| r.state == MatchState::Missing)
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(missing, ["Helm", "Ray", "Terraform"]);
    assert_eq!(job.counts.matched, 2);
    // (2 matched) / (2 + 3 missing) = 40 %.
    assert_eq!(job.coverage, Some(40.0));
    let learn = learning(
        &state,
        &one,
        &LearningCriteria::default(),
        &SkillGapOptions::default(),
    )
    .unwrap();
    assert_eq!(
        learn.gaps.len(),
        3,
        "a single job keeps its single mentions"
    );
}

#[test]
fn scenario_i_analytics_work_without_a_profile() {
    let state = new_state(&[]);
    seed(&state);
    let q = default_preferences().query;
    let o = overview(&state, &q).unwrap();
    assert_eq!(o.summary.analyzed, 7);
    assert!(o.jobs.iter().all(|j| j.match_percent.is_none()));
    let gap = skill_gap(&state, &q, &SkillGapOptions::default()).unwrap();
    assert!(!gap.profile_available);
    assert!(gap
        .summary
        .contains(&"Profile required for personal skill-gap comparison.".to_string()));
    assert!(!gap.demand.is_empty(), "market demand is still shown");
    assert!(gap.priorities.is_empty());
    assert!(gap
        .matrix
        .rows
        .iter()
        .flat_map(|r| &r.cells)
        .all(|c| !matches!(c, CellState::Missing | CellState::Matched)));
    let learn = learning(
        &state,
        &q,
        &LearningCriteria::default(),
        &SkillGapOptions::default(),
    )
    .unwrap();
    assert!(learn.gaps.is_empty() && !learn.profile_available);
    assert!(research(
        &state,
        &q,
        &LearningCriteria::default(),
        &SkillGapOptions::default()
    )
    .is_err());
}

#[test]
fn scenario_j_missing_salaries_are_reported_not_invented() {
    let state = new_state(&[]);
    seed(&state);
    let o = overview(&state, &default_preferences().query).unwrap();
    assert_eq!(o.summary.salary.known, 6);
    assert_eq!(
        o.summary.salary.comparable, 5,
        "the USD salary is not compared with EUR"
    );
    assert_eq!(o.summary.salary.currency.as_deref(), Some("EUR"));
    assert!(o
        .summary
        .notes
        .contains(&"Salary available for 6 of 7 jobs.".to_string()));
    let globex = o
        .jobs
        .iter()
        .find(|j| j.company.as_deref() == Some("Globex"))
        .unwrap();
    assert_eq!(globex.salary, None);
    assert_eq!(globex.date_posted, None);
    // Ranking by salary lists unknown salaries last, in both directions.
    for direction in [SortDirection::Asc, SortDirection::Desc] {
        let q = query(
            DataScope::default(),
            vec![],
            vec![SortCriterion {
                key: SortKey::Salary,
                direction,
                value: None,
            }],
        );
        let o = overview(&state, &q).unwrap();
        assert!(o.jobs.last().unwrap().salary.is_none());
        assert!(!o.jobs[o.jobs.len() - 2].salary_comparable, "USD after EUR");
    }
}

#[test]
fn composes_multiple_conditions() {
    let state = new_state(&[]);
    seed(&state);
    let mut salary = cond(FilterField::Salary, FilterOp::AtLeast, &[], Some(100_000.0));
    salary.currency = Some("EUR".into());
    let q = query(
        DataScope::default(),
        vec![
            cond(FilterField::Country, FilterOp::Is, &["Austria"], None),
            cond(
                FilterField::WorkMode,
                FilterOp::Is,
                &["remote", "hybrid"],
                None,
            ),
            salary,
            cond(FilterField::Seniority, FilterOp::IsNot, &["entry"], None),
            cond(FilterField::Skill, FilterOp::HasAny, &["k8s"], None),
            cond(FilterField::Company, FilterOp::IsNot, &["Vandelay"], None),
        ],
        vec![],
    );
    let mut t = titles(&overview(&state, &q).unwrap());
    t.sort();
    assert_eq!(
        t,
        [
            "AI Engineer @ Company A GmbH",
            "AI Platform Engineer @ Initech"
        ]
    );
    let recent = query(
        DataScope::default(),
        vec![cond(
            FilterField::DatePosted,
            FilterOp::OnOrAfter,
            &["2025-09-15"],
            None,
        )],
        vec![],
    );
    assert_eq!(overview(&state, &recent).unwrap().summary.analyzed, 2);
    let mut limited = default_preferences().query;
    limited.limit = Some(3);
    let o = overview(&state, &limited).unwrap();
    assert_eq!((o.summary.matching, o.summary.analyzed), (7, 3));
}

#[test]
fn deleting_a_search_keeps_jobs_other_searches_found() {
    let state = new_state(&[]);
    let (r1, _, _) = seed(&state);
    delete_run(&state, r1).unwrap();
    let o = overview(&state, &default_preferences().query).unwrap();
    // Initech was only in search 1; Company A and Globex were found again.
    assert_eq!(o.summary.analyzed, 6);
    assert!(o
        .jobs
        .iter()
        .all(|j| j.company.as_deref() != Some("Initech")));
}

#[test]
fn preferences_round_trip_and_validate() {
    let state = new_state(&[]);
    assert_eq!(preferences(&state).unwrap(), default_preferences());
    let mut prefs = default_preferences();
    prefs
        .query
        .filters
        .push(cond(FilterField::Country, FilterOp::Is, &["Austria"], None));
    prefs.read_pages = false;
    save_preferences(&state, prefs.clone()).unwrap();
    assert_eq!(preferences(&state).unwrap(), prefs);
    assert!(!state.analytics.options(&state).unwrap().read_pages);
    prefs
        .query
        .filters
        .push(cond(FilterField::Salary, FilterOp::Contains, &["x"], None));
    assert!(save_preferences(&state, prefs).is_err());
}

const TABLE_ANSWER: &str = "Here are matching roles:\n\n| Company | Role | Location | Work mode | Salary | Posted | Key skills | Link |\n|---|---|---|---|---|---|---|---|\n| Company A | AI Engineer | Vienna, Austria | Remote | €100k | 2 days ago | Python, K8s | https://boards.greenhouse.io/companya/jobs/777001 |\n| Globex | ML Engineer | Berlin | Hybrid | — | — | PyTorch | https://globex.example/careers/ml-888002 |\n";

async fn connected(reply: &[&str]) -> AppState {
    let state = new_state(reply);
    providers::connect(&state, ProviderKind::Anthropic, "k")
        .await
        .unwrap();
    providers::set_default_model(
        &state,
        &ModelRef {
            provider_id: "anthropic".into(),
            model_id: "model-a".into(),
        },
    )
    .unwrap();
    state
}

#[tokio::test]
async fn chat_answers_with_job_tables_are_ingested_automatically() {
    let state = connected(&[TABLE_ANSWER]).await;
    let sent = chat::send_message(
        &state,
        SendMessageInput {
            conversation_id: None,
            content: "Find AI jobs in Austria".into(),
            model: ModelRef {
                provider_id: "anthropic".into(),
                model_id: "model-a".into(),
            },
            use_profile: false,
            agent_ids: Vec::new(),
            mcp_server_ids: Vec::new(),
        },
    )
    .await
    .unwrap();
    for _ in 0..200 {
        if !runs(&state).unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let runs = runs(&state).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!((runs[0].source, runs[0].result_count), (RunSource::Chat, 2));
    assert_eq!(runs[0].title, "Find AI jobs in Austria");
    assert_eq!(runs[0].conversation_id, Some(sent.conversation.id));
    let linked = state
        .db
        .call(|c| repo::runs_for_conversation(c, sent.conversation.id))
        .unwrap();
    assert_eq!(linked, vec![(sent.assistant_message.id, runs[0].id, 2)]);
}

#[tokio::test]
async fn scheduled_task_results_are_ingested_automatically() {
    let state = connected(&[TABLE_ANSWER]).await;
    let task = tasks::create(
        &state,
        TaskInput {
            name: "Weekly AI jobs".into(),
            kind: TaskKind::Prompt,
            use_profile: false,
            prompt: "Weekly job table of remote AI roles".into(),
            model: ModelRef {
                provider_id: "anthropic".into(),
                model_id: "model-a".into(),
            },
            timezone: "UTC".into(),
            start_date: jiff::Zoned::now().date().tomorrow().unwrap().to_string(),
            start_time: "08:00".into(),
            schedule: Schedule::Interval {
                every: 24,
                unit: IntervalUnit::Hours,
            },
            end: EndCondition::Never,
        },
    )
    .await
    .unwrap();
    let row = state.db.call(|c| task_repo::get(c, task.id)).unwrap();
    for _ in 0..2 {
        scheduler::execute(
            &state,
            &row,
            ExecutionTrigger::Manual,
            None,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    }
    let runs = runs(&state).unwrap();
    assert_eq!(runs.len(), 2, "every run is kept as history");
    assert!(runs
        .iter()
        .all(|r| r.source == RunSource::Task && r.task_id == Some(task.id)));
    let o = overview(
        &state,
        &query(searches(&[runs[0].id, runs[1].id]), vec![], vec![]),
    )
    .unwrap();
    assert_eq!(
        o.summary.analyzed, 2,
        "the same jobs found twice are stored once"
    );
    assert_eq!(o.summary.duplicates_merged, 2);
}

#[test]
fn earlier_searches_are_ingested_once() {
    let state = new_state(&[]);
    let model = ModelRef {
        provider_id: "anthropic".into(),
        model_id: "model-a".into(),
    };
    state
        .db
        .call(|c| {
            let conv = conversations::create(c, "Old search", &model, 1)?;
            for (role, content) in [
                (MessageRole::User, "AI jobs please"),
                (MessageRole::Assistant, TABLE_ANSWER),
            ] {
                conversations::insert_message(
                    c,
                    conversations::NewMessage {
                        conversation_id: conv.id,
                        role,
                        content,
                        status: MessageStatus::Complete,
                        model: Some(&model),
                        created_at: SEP_25,
                    },
                )?;
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(ingest::ingest_history(&state).unwrap(), 1);
    assert_eq!(ingest::ingest_history(&state).unwrap(), 0, "only once");
    let runs = runs(&state).unwrap();
    assert_eq!(
        (runs.len(), runs[0].title.as_str(), runs[0].created_at),
        (1, "AI jobs please", SEP_25)
    );
    // "2 days ago" counts back from when the search ran, not from today.
    let o = overview(&state, &default_preferences().query).unwrap();
    assert_eq!(o.jobs[0].date_posted, Some(SEP_25 - 2 * DAY));
}

const RESEARCH_ANSWER: &str = r#"{"recommendations": [
 {"skill": "Terraform", "path": ["Terraform fundamentals", "Terraform with AWS", "Modules, state and CI"],
  "resources": [{"title": "HashiCorp Terraform Associate", "provider": "HashiCorp", "url": "https://terraform.example.invalid/certification", "type": "certification", "level": "intermediate", "cost": "paid", "note": "Recognised certification."}]}
]}"#;

#[tokio::test]
async fn researches_learning_resources_for_prioritized_gaps() {
    let state = connected(&[RESEARCH_ANSWER]).await;
    seed(&state);
    with_profile(&state);
    let q = default_preferences().query;
    let criteria = LearningCriteria::default();
    let options = SkillGapOptions::default();
    let before = learning(&state, &q, &criteria, &options).unwrap();
    assert_eq!(before.gaps[0].name, "Terraform");
    assert!(
        before.research.is_none(),
        "nothing is researched automatically"
    );
    let started = research(&state, &q, &criteria, &options).unwrap();
    assert_eq!(
        started.research.as_ref().unwrap().status,
        ResearchStatus::Running
    );
    let mut done = None;
    for _ in 0..300 {
        let view = learning(&state, &q, &criteria, &options).unwrap();
        if view
            .research
            .as_ref()
            .is_some_and(|r| r.status != ResearchStatus::Running)
        {
            done = view.research;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let research = done.expect("research finished");
    assert_eq!(
        research.status,
        ResearchStatus::Done,
        "{:?}",
        research.error
    );
    assert!(research.current && !research.stale);
    let rec = &research.recommendations[0];
    assert_eq!(rec.skill, "Terraform");
    assert!(
        rec.why[0].starts_with("Required by 4 of 7 jobs (57%)"),
        "{:?}",
        rec.why
    );
    assert!(
        rec.why
            .iter()
            .any(|w| w.contains("3 of the 3 jobs paying €105k / year or more")),
        "{:?}",
        rec.why
    );
    assert_eq!(
        rec.resources[0].reachable,
        Some(false),
        "the link was checked"
    );
    assert_eq!(rec.path.len(), 3);
}
