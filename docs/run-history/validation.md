# Scheduled task run history — validation report

What was run and what was observed. Environment: Linux container, Xvfb
display, D-Bus session with GNOME Keyring, Rust 1.98.1 (the CI's stable).
Providers were the local stand-ins in `scripts/e2e/mock-providers.mjs`
(Google and Microsoft mail and calendar, and an OpenAI-compatible model).

## 1. Automated checks

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --all-targets --locked -- -D warnings` | clean |
| `cargo test --locked` | 480 passed, 3 ignored (explicit-only tests, as before) |
| `pnpm lint`, `pnpm typecheck` | clean |
| `pnpm test` | 98 passed (20 files) |
| `pnpm build` | built |

The tests the specification asks for (§53):

| Area | Tests |
|---|---|
| Run creation | `every_firing_and_every_run_now_is_one_run_with_its_own_result` (a scheduler tick and Run now: one run each, unique ids, the task, the trigger, `scheduled_for`) |
| Lifecycle | `records_runs_with_snapshots_progress_and_outputs` (queued → running → succeeded, a finished run never rewritten); `a_failed_run_keeps_a_safe_error_and_no_secret` (→ failed); `a_run_is_visible_while_running_and_records_its_stages` (visible and running before it ends, stages as they happen, an event for every change); `a_stopped_run_is_recorded_as_cancelled`; `a_firing_while_the_last_run_is_busy_is_recorded_as_skipped` |
| Persistence | `runs_their_outputs_and_progress_survive_a_restart` (a database file reopened: result, stages and outputs kept; a run left running reconciled); `unfinished_runs_are_marked_interrupted_without_an_output`; the 0009 → 0010 migration test (ids, results and Analytics links kept, deleted ids never reused, deleting a task deletes its runs) |
| History | newest first and paging (`records_runs_with_snapshots_progress_and_outputs`); an old run keeps its own result (`every_firing_…`); Vitest "opens a task into its runs, newest first, each with its own result", "loads older runs only when asked" |
| Task edits | `runs_keep_the_task_as_it_was_through_edits_pause_and_resume` (prompt, model, schedule, Profile, name; pause and resume keep the history) |
| Manual runs | Manual trigger and label (Rust and Vitest), result kept |
| Failure | failed run kept with category and safe message; Vitest "shows a failed run with its error and the way to fix it" (Open Connectors) |
| Built-in automation | `job_mail_sync_records_a_normal_run_with_progress_outputs_and_no_tokens` (Gmail through a local stand-in: five stages, the Application Watch output, Gmail in the context) |
| Security | `safe_messages_carry_no_credentials` (authorization values, key and token pairs, `sk-`, `ya29.`, `1//`, JWTs); the Job Mail run's rows scanned for its access token, refresh token, client secret and `Bearer`; a provider error carrying a key is stored without it |
| Outputs | the Analytics job search of a prompt task's result (`scheduled_task_results_are_ingested_automatically`); a job search that could not search records a failed search stage and the `search` category |
| Interface | Vitest: breadcrumb and run list, selecting an older run, Task configuration and Context of an old run, the failure view, a running run followed live until it succeeds (backend event), Run now opens the new run, empty state, run labels and guidance (`taskRuns.test.ts`) |

## 2. In-app end-to-end (debug build)

The data directory was the one left by the Job Mail & Interview Sync run
(database version 9: the built-in task with seven runs, Gmail, Outlook Mail
and both calendars connected).

| Step | Observed |
|---|---|
| Upgrade 9 → 10 | Version 10; the seven runs kept their ids, times and results; each has its report as an "Application Watch" output; nothing else changed. |
| Scheduled Tasks → History on the built-in card | "Scheduled / Job Mail & Interview Sync"; seven runs newest first with Manual tags (the one scheduled run without); the newest selected with its Job Application Update; Progress 0 and Schedule "Not recorded" for runs from before snapshots (honest, nothing made up). |
| Turn on, new Gmail message (Pied Piper interview), Run now | The page switched to the new run: "Succeeded · Manual"; Progress: "Synchronized Gmail and Outlook Mail · 1 new message", "Found 1 job-related message", "Read 1 job-related message", "Updated 1 application", "Calendar checked · 2 conflicts"; Context: Gmail, Outlook Mail, Google Calendar, Outlook Calendar; web search not available. |
| New prompt task "Interview tips" (Profile on, daily 06:45) → open | "No runs yet" with the explanation and Run now. |
| Run now | Manual run: "Mock answer from the local model."; stages "Your Profile is empty; nothing was added", "Answer received"; Task configuration: prompt, model, Profile Yes, "Daily 06:45 AM (Etc/UTC)". |
| Edit: name "DE interview prep", new prompt, Profile off | The header shows the new name; the earlier run still shows "Name then: Interview tips", the old prompt and Profile Yes. |
| 06:45 scheduled firing (page left open) | "Run — Today at 06:45 AM" appeared in the list without a click; next run moved to tomorrow; the run: "Succeeded · Scheduled", the new prompt, Profile No. |
| Model slowed to 30 s, Run now | "Running · Manual", elapsed time counting, "Waiting for the model's answer" in progress, **Stop run**, list tag "Manual · Running", header "Running…". |
| ReMa killed during that run, started again | The run: Failed, "Execution interrupted before completion." with "ReMa was closed or stopped while this run was in progress…"; its running stage closed as failed; all earlier runs and results still there. |
| Dark and light themes | Run view, list, inspector and error box use the theme tokens in both. |

Found and fixed during the run:

- the task's meta line (schedule, model, next run, status) wrapped under
  the title with a separator at the start of the line: it now has its own
  full-width line;
- runs from before snapshots labelled the instructions of a Job Mail run
  "Prompt": the task's own kind is used when a run has none;
- an interrupted run showed a duration up to the moment ReMa noticed it:
  its end time and duration are no longer shown;
- the stand-in calendar reused event ids after a restart (a real calendar
  never does), which hit the unique event index: its ids are now unique per
  session. ReMa itself finds its own event by `remaInterviewId` on the next
  run, so a real collision would not duplicate events.

## 3. Not verified here

- Real providers (the model and the mailboxes were local stand-ins).
- Very long histories in the interface (paging is covered by tests; the
  run with the most runs had eight).
