# Scheduled task run history — implementation record

Every scheduled task keeps its own persistent execution history: each run
(scheduled firing or **Run now**) is a record with a snapshot of the task as
it was, its progress, its outputs, its result or a safe error. Specification:
`7fbae61e-ReMa_Scheduled_Task_Run_History_Implementation_1.md` (kept outside
the repository). The plan and checklist for all three current
specifications are in [../implementation-plan.md](../implementation-plan.md);
validation is in [validation.md](validation.md).

## 1. Where things live

```text
db/migrations/0010_run_history.sql   task_executions rebuilt (queued/cancelled, snapshots,
                                     context, error_category); task_run_events; task_run_outputs
db/runs.rs                           the run record: insert (queued), mark_running, stages,
                                     outputs, finish (once), list (paged), get, mark_interrupted
services/runs.rs                     the lifecycle: create (snapshot), record_skipped, start,
                                     finish, Failure (category + safe message), safe_message,
                                     RunRecorder (stages, context, outputs, live events),
                                     Activity (how pipelines report stages)
services/scheduler.rs                the execution boundary: spawn_run → execute → run_task,
                                     stages for prompt tasks and job searches, outputs
jobs/mod.rs, services/mail_monitor.rs
                                     Job Mail & Interview Sync stages, through RunConfig.activity
commands/tasks.rs                    list_task_runs, get_task_run, cancel_task_run; run_task_now
                                     returns the run id
models/task.rs                       TaskRun, TaskRunSummary, TaskRunPage, RunContext,
                                     RunProgressEvent, RunOutput(Ref), RunErrorCategory, TaskRunChanged
src/components/tasks/TaskRunsView.tsx   "Scheduled / <Task>": run view, run list, inspector
src/hooks/useTaskRuns.ts             paged history and one run, reloaded on TaskRunChanged
src/lib/taskRuns.ts                  titles, statuses, tags, failure guidance, web search text
src/components/ui/Collapsible.tsx    the inspector's sections
```

## 2. Lifecycle

```text
scheduler fires / Run now
   ↓  spawn_run: one run per firing or click (the task must not be running)
create run (queued) — snapshot: name, kind, prompt, model, schedule, Profile setting
   ↓  execute
running (started_at; the task's last_run_at)
   ↓  run_task: stages recorded as they happen, context as it is used
final output (result / report) → succeeded     safe error + category → failed
                                                stopped → cancelled
   ↓
outputs (references), stages closed, TaskRunChanged → the open view updates
```

- The run exists before the task executes, so a crash or failure still
  leaves a record. At start-up, runs a previous session left queued or
  running become **failed — "Execution interrupted before completion."**
  (category `interrupted`); no output is made up for them.
- A scheduled firing while the previous run is still going is recorded as
  one run, **Skipped** (status `cancelled`, category `skipped`). Run now on a
  running task is refused and records nothing.
- The scheduler has no run-level retries; a job search's second search
  attempt appears in its stage label ("Searching the web (second attempt)").
- Stopping a run (**Stop run**) cancels it: status `cancelled`. When ReMa is
  shutting down, a stopped run is recorded as interrupted.
- A finished run is never rewritten (`finish` only changes queued or running
  runs).

## 3. What a run keeps

| Part | Where | Notes |
|---|---|---|
| Trigger, status, `scheduled_for`, `queued_at`, `started_at`, `finished_at` | `task_executions` | Epoch milliseconds (UTC); shown in local time. A run that never started keeps `started_at` empty. Duration = finished − started. |
| Snapshot | `task_name`, `kind`, `schedule` (schedule, timezone, local start), `use_profile`, `provider_id`/`model_id`, `prompt` | Taken when the run is created; edits, renames, model or schedule changes never touch it. Runs recorded before 0010 kept their prompt and model; the other fields are empty ("Not recorded"). |
| Context | `context` (JSON) | Profile given to the model, connected services used (by name), web search available, searches and engines. Names and flags only. |
| Final output | `result` (Markdown) and `report` (Job Mail & Interview Sync) | The authoritative result, written once when the run ends. |
| Progress | `task_run_events` | One row per stage (pending → running → completed / failed / skipped), in the order first recorded; labels are concise activity ("Searched with ChatGPT web search · 3 searches"), never model reasoning. |
| Outputs | `task_run_outputs` | References, never copies: `run_report` (the run's own report), `job_search` (an Analytics job search), `application` (an application). |
| Error | `error` (safe message), `error_category` | Categories: timeout, interrupted, cancelled, skipped, model, model_access, provider, billing, connector, network, search, task, internal. |

Stages by task type:

| Task | Stages |
|---|---|
| Prompt | Load your Profile (when on) → [Searching the web · N searches, when the model searched] → Get the answer |
| Job search (prompt detected as one) | Load your Profile → Search for jobs → Check the postings → Assess the listings |
| Job Mail & Interview Sync | Read new mail (per mailbox) → Find job-related messages → Read job-related messages → Update applications → Check calendars |

Outputs: a job search's listings become an Analytics job search when the
result is ingested ("Job Search Results · N jobs"); a Job Mail & Interview
Sync run has its report ("Application Watch") and one output per interview
it added to or moved in a calendar ("Interview · Globex — added to your
calendar", opening the application). Migration 0010 gives earlier runs the
outputs they already had (reports, Analytics searches made from them).

## 4. Secrets

A run never stores tokens, keys, authorization headers, PKCE data or
passwords: the snapshot and context hold names and flags only, and every
error passes `services::runs::safe_message`, which removes credential-shaped
text (authorization values, `key=`/`token=` pairs, `sk-…`, Google `ya29.`
and `1//` tokens, JWTs, GitHub tokens) after the providers' own scrubbing.
Tests check the stored rows of a full Job Mail & Interview Sync run and a
failed run whose provider error carried a key.

## 5. Interface

- **Scheduled Tasks** keeps its overview (built-in card, task table, `…`
  actions). Clicking a task row (or **History** on the built-in card) opens
  its runs; **Run now** anywhere creates a manual run and opens it.
- **Scheduled / <Task>**: the breadcrumb returns to the overview; the task's
  schedule, model, next run and status; Run now and the `…` menu.
- The selected run (the newest by default): "Run — Today at 21:30", status,
  Manual/Scheduled, model, started / finished (or failed / ended), duration
  or live elapsed time; then the result (safe Markdown, or the Job
  Application Update report), or the live progress and "The result appears
  here when the run finishes." with **Stop run**, or the error with guidance
  (Open Connectors, Open Settings or Edit task where that is the fix).
- The panel on the right: the runs, newest first, 30 at a time (more load
  when the end of the list comes into view, or with **Load older runs**),
  Manual / Failed / Running / Skipped tags; under it, collapsible
  **Progress**, **Context**, **Outputs** (open an Analytics search, an
  application, or the run's report) and **Task configuration** (name then,
  prompt, mail settings, model, Profile, schedule and time zone).
- Live updates: every run change emits `TaskRunChanged`; the open list and
  run reload (coalesced), no polling.
- The panels scroll independently; below 860 px the history sits under the
  run. All colours come from the theme tokens (light and dark).

## 6. Acceptance checklist (specification §54)

| # | Criterion | Status | Evidence |
|---|---|---|---|
| 1 | Every task opens into a detail view | Verified | `TaskRunsView`; Vitest "opens a task into its runs…"; E2E |
| 2 | Every automatic execution creates a persistent run | Verified | `spawn_run`; `every_firing_and_every_run_now_is_one_run_with_its_own_result`; E2E scheduled firing |
| 3 | Every Run now creates a persistent run | Verified | same test; E2E |
| 4 | Each run has its own id | Verified | AUTOINCREMENT kept across the rebuild (`runs_keep_their_ids_results_and_links_when_run_history_arrives`) |
| 5 | Newest first | Verified | `db::runs::list`; tests; E2E |
| 6 | Manual runs identified | Verified | "Manual" tag and header; tests; E2E |
| 7 | Running runs appear immediately | Verified | queued before execution; `a_run_is_visible_while_running_and_records_its_stages`; Vitest live update |
| 8 | Successful runs keep their results | Verified | tests; E2E |
| 9 | Failed runs keep their failure | Verified | `a_failed_run_keeps_a_safe_error_and_no_secret`; Vitest failure view |
| 10 | A historical run shows its exact result | Verified | tests; Vitest; E2E |
| 11 | New runs never overwrite older ones | Verified | `finish` updates only unfinished runs; tests |
| 12 | Edits keep historical snapshots | Verified | `runs_keep_the_task_as_it_was_through_edits_pause_and_resume`; E2E (rename + prompt) |
| 13 | Model changes keep the historical model | Verified | same test |
| 14 | Pausing keeps history | Verified | same test |
| 15 | Restarting keeps history | Verified | `runs_their_outputs_and_progress_survive_a_restart`; E2E restart |
| 16 | Progress persisted | Verified | `task_run_events`; tests |
| 17 | Outputs persisted by reference | Verified | `task_run_outputs`; analytics and Job Mail tests |
| 18 | Context metadata of safe capabilities | Verified | `RunContext`; tests; E2E |
| 19 | No OAuth/API secrets in run history | Verified | `safe_messages_carry_no_credentials`; Job Mail run scan; failed-run test |
| 20 | Built-in automations use the same system | Verified | `job_mail_sync_records_a_normal_run_with_progress_outputs_and_no_tokens`; E2E |
| 21 | User tasks use the same system | Verified | scheduler tests; E2E |
| 22 | Run now uses the same pipeline | Verified | `run_now` → `spawn_run` → `execute` |
| 23 | Stale running runs reconciled | Verified | `unfinished_runs_are_marked_interrupted_without_an_output`; restart test |
| 24 | Long histories loaded efficiently | Verified | 30 per page, cursor by id; Vitest "loads older runs only when asked" |
| 25 | Light and dark | Verified | theme tokens; E2E screenshots in both themes |
| 26 | Create / edit / pause / delete / Run now still work | Verified | existing task tests; Vitest; E2E |
| 27 | Existing functionality unaffected | Verified | full Rust and Vitest suites |
| 28 | Tests pass | Verified | see validation.md |
