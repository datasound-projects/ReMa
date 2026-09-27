# Profile context, Applications, Job Mail & Interview Sync and the calendar — implementation record

What this change does, the rules it follows, where each part lives, and the
acceptance checklist. The connectors it builds on are described in
[../connectors/implementation.md](../connectors/implementation.md); how the
work was validated is in [validation.md](validation.md).

## 1. Where things live

```text
Navigation         src/app/pages.ts, navigation.ts, App.tsx
                   Chat · Agents · Scheduled Tasks · Profile · Portfolio Studio · Applications (Settings at the bottom)
Profile            src/pages/ProfilePage.tsx                 two tabs: Documents & Credentials, Custom Profile
Portfolio Studio   src/pages/PortfolioStudioPage.tsx         its own page (was a Profile tab)
Applications       src/pages/ApplicationsPage.tsx            five section cards
Built-in task      src/pages/ScheduledTasksPage.tsx          "Built-in" card; setup/settings in TaskDialog.tsx
Calendar           src/components/layout/CalendarPanel.tsx   title-bar button + panel
        │  typed IPC (tauri-specta)
services/profile_context/   mod.rs (ProfileContext, precedence, rendering), cv.rs (CV text → facts),
                            labels.rs (field names → fields), excerpts.rs (document detail on demand)
services/tasks.rs           the built-in task: one per install, needs a mailbox, run-now guard
services/mail_monitor.rs    the task's run: reading plan per mailbox, then jobs::run
jobs/                       mod.rs (reading plan, sync, classification loop), extract.rs (categories,
                            confidence, rejection reasons), applications.rs (identity, chronology,
                            latest-update text), calendar_sync.rs (conflicts in every calendar, events),
                            tracker.rs (rows and sections)
services/calendar_view.rs   the in-app calendar: provider events + ReMa interviews, merged
services/connector_tools.rs chat reads the same rows (applications_find_match, by section)
db/migrations/0009_job_mail_sync.sql
```

## 2. Profile context

One deterministic pipeline (`services/profile_context/`) builds the context
for Chat (Profile switch on) and scheduled prompt tasks ("Include my
Profile"):

```text
Custom Profile, primary CV, other CVs, credentials     raw sources
   ↓ normalize      CV text → sections → entries and facts; profile fields → facts
   ↓ deduplicate    the same skill, job, degree or language once
   ↓ precedence     1 Custom Profile  2 primary CV  3 other CVs  4 credentials
ProfileContext      17 fields (identity, summary, target_roles, experience, skills, technologies,
                    education, certifications, languages, locations, work_preferences,
                    salary_preferences, industries, projects, achievements,
                    portfolio_information, additional_relevant_context) + source references
   ↓
<user_profile> … </user_profile>  (+ <profile_excerpts> only when the request needs them)
```

- **Precedence.** For preference-like fields (summary, target roles, locations, work preferences, salary preferences) the highest source that has the field wins and lower sources' values are left out, so a CV's "Preferred location: Vienna" never contradicts the Custom Profile. Accumulating fields (experience, skills, education, …) are merged in precedence order without duplicates; the CV stays evidence of past experience. Overridden values are listed as such in the source references.
- **Credentials** supplement the context. A credential is an objective record: it replaces a less precise CV mention of the same certification, and next to a Custom Profile fact it is added as "(credential record: …)" instead of overwriting it.
- **Excerpts.** The normalized fields carry the stable facts. A document's own text is added only when the message needs it: the whole CV (up to 10,000 characters) when the user asks to work on the CV or a cover letter or names the document, otherwise at most 3 passages (1,500 characters each) that match the request and add something the fields do not.
- **Budget and privacy.** The fields are limited to 14,000 characters. Email addresses, phone numbers and personal details (birth date, nationality, address, …) are removed. Other files and Portfolio Studio CVs are named, never their text. Uploaded files are never changed.
- **On/off.** On: the context is rebuilt for every message, so an edit applies to the next message. Off: nothing from the Profile is sent. Text the user writes or attaches in the chat is not affected.
- **UI.** Profile explains this under its title ("When Profile is enabled in Chat, ReMa combines your Custom Profile, CVs and credentials into one career context. …"), and each tab says what it is for.

## 3. Applications

**One row per application, in exactly one section**, derived from its status
(`ApplicationStatus::section()`):

| Section | Statuses | Last column |
|---|---|---|
| Interviews Confirmed | confirmed interview | Interview ("Interview confirmed for Sep 29 at 10:00 CEST."; a conflict adds "Calendar conflict: not added to your calendar.") |
| Applications Confirmed | application confirmed | Latest update ("Application received successfully. No action required.") |
| Needs Your Action | action required, interview requested (proposed times), assessment, offer | Requested action (from the email, e.g. "Choose an interview slot from the proposed times.") |
| Applications In Progress | in progress (under review, recruiter messages, a cancelled interview) | Latest update (from the email) |
| Rejected | rejected | Rejection reason (only what the email says, else "No reason provided.") |

Columns: Date (latest message), Company, Role, Status, and the section's last
column. Rows are ordered by the latest message, newest first. Each section is
a rounded card that collapses (remembered per device) and scrolls inside.

**Chronology.** Emails are applied oldest first across all mailboxes. The
status is set only by an email at least as new as the one that set the
current status (`status_at`); an older email read later (a backfill, a
delayed mailbox) adds to the timeline but never moves the application back.
Interview details come from the newest email about the interview.

**Identity.** An email belongs to an application by thread, then job ID /
reference, then company + role. Same company with different roles, or the
same role with different job IDs, are different applications. A new
application confirmation after a rejection, or after 180 days without
news, starts a new application. When the match is unclear the email is
listed for review and nothing changes. The same email in Gmail and Outlook
(same sender and subject, within 10 minutes) is processed once.

**Classification.** 13 categories (application_confirmed, application_update,
needs_action, recruiter_message, interview_request, interview_confirmed,
interview_rescheduled, interview_cancelled, assessment_request, rejection,
offer, other_job_related, not_job_related) with a confidence. Below 0.5
nothing changes; interview confirmations, reschedules, cancellations,
rejections and offers need 0.8. A rejection reason is kept only if the email
contains it (quoted, at least three words). Dates, times and time zones must
be quoted from the email.

**Chat** uses the same rows: `applications_find_match` returns section,
status, latest update, rejection reason and meeting link, and can filter by
section.

## 4. Job Mail & Interview Sync (the automation boundary)

- **Only this task reads mail automatically.** Connecting Gmail or Outlook reads nothing and records no sync; the per-connector background worker and "Sync now" are gone. Settings → Connectors says so and shows whether the task is on. With the task off (or paused) nothing reads mail: the scheduler skips it, and "Run now" (card, menu, task page, tray) is refused in Rust. No other task can read mail.
- **One built-in task** (`scheduled_tasks.builtin = 'job_mail_sync'`, unique). Scheduled Tasks shows it in a "Built-in" card with the mail + calendar icon and the description "Tracks job-application emails, updates Applications, detects actions and confirmed interviews, and syncs confirmed interviews with your calendar." Before it is set up the card says "Off. ReMa does not read your mail until you set this up." Turning it on needs a connected mailbox.
- **Settings**: the mail sources (every connected mailbox), "Initial lookback: last [N] days" (default 30, 1–365), whether confirmed interviews go to the calendar, optional instructions, the model, and the usual scheduler controls (default every hour).
- **Lookback** (`jobs::reading_plan`, per mailbox):
  - first run: the last N days;
  - later runs: only changes since the stored cursor (Gmail `history.list`, Graph delta); an expired cursor resumes from the last successful run, never before the lookback;
  - a larger N: the newly included days are read once (Gmail dated search, Graph `receivedDateTime` range);
  - a smaller N: nothing old is read and nothing is deleted.
- **Progress is saved only after success**: the messages, the cursor and how far back the mailbox was read (`mail:covered_from`) are written in one transaction.
- **Privacy**: the prefilter and a headers-only relevance check keep unrelated mail from the model; only job-related mail is read in full. Email text is fenced, untrusted data; pipeline requests carry no tools; answers are validated in Rust. Tokens never leave the connector layer.

## 5. Calendar

**Adding interviews.** Only confirmed interviews (not proposals) whose date
and time the email states, with confidence ≥ 0.8. ReMa first checks **every
connected calendar** (Google and Outlook) for overlapping events; if the time
is free it creates the event in the calendar of the mailbox the email came
from (or the other one), otherwise it flags the conflict ("Calendar conflict"
in Applications and in the calendar panel), sends a notification naming the
conflicting events, and creates nothing. Adding a conflicting interview by
hand asks first.

**Events.** Title "Interview — Company — Role"; the description lists
company, role, time with time zone, interview type, interviewers, location,
meeting link and "Source: ReMa Applications (application #id)"; the meeting
link is also the location. A missing end time reserves 60 minutes (and says
so). Each event carries a private `remaInterviewId`, so it is never created
twice; a reschedule updates the same event (a new time that conflicts is
reported, not moved); a cancellation marks the event cancelled and free;
events the user deleted are not recreated.

**In-app calendar.** The calendar button in the title bar opens a panel with
seven days at a time (previous / today / next). Events are read live from
the connected calendars for the range shown (nothing is stored; at most 62
days per request); ReMa's interviews are highlighted with company and role, a
link to the application and a **Join** button for the meeting link. An
interview that is in no calendar (a conflict, or no calendar connected) is
shown from Applications with its reason. The same interview in two calendars
is listed once. With no calendar connected the panel says "No calendar
connected. Connect Google Calendar or Outlook Calendar in Settings →
Connectors." with a button that opens it.

## 6. Upgrade (migration 0009)

- The existing mail task (the oldest, if there are several) becomes the built-in "Job Mail & Interview Sync"; its settings and state stay.
- Any other mail task is paused and never runs again (Rust refuses to resume or run it; Scheduled Tasks says "No longer runs: only Job Mail & Interview Sync reads mail."). It stays listed, with its history, until the user deletes it.
- Background mail sync is switched off for every connector; nothing reads mail until the built-in task is on.
- Stored categories are renamed (`application_received` → `application_confirmed`, `action_required` → `needs_action`); applications get `latest_update`, `rejection_reason`, `status_category`, `status_at` and their mailbox.

## 7. Acceptance checklist

| # | Criterion | Status | Where / how verified |
|---|---|---|---|
| 1 | Portfolio Studio is no longer a Profile tab | done | `ProfilePage.tsx`; `ProfilePage.test.tsx`; E2E |
| 2 | Portfolio Studio has its own route directly under Profile | done | `pages.ts`; `navigation.test.tsx`; E2E |
| 3 | Applications is its own route | done | `pages.ts`; E2E |
| 4 | Profile has only Documents & Credentials and Custom Profile | done | `ProfilePage.tsx`; tests; E2E |
| 5 | One deterministic context assembly for Profile ON | done | `services/profile_context/`; `profile_context/tests.rs` |
| 6 | Custom Profile overrides conflicting CV facts | done | precedence; test "custom profile overrides" |
| 7 | CVs and credentials still contribute | done | accumulating fields, credentials; tests |
| 8 | Profile OFF injects nothing | done | `chat::with_profile`; `profile_context_is_sent_only_when_turned_on` |
| 9 | Exactly the five sections | done | `ApplicationSection`; `ApplicationsPage.test.tsx`; E2E |
| 10 | Sections collapse and scroll independently | done | `SectionCard`; tests; E2E |
| 11 | Required columns | done | `ApplicationsPage.tsx`; tests; E2E |
| 12 | One application in one section | done | `section()`; `each_category_maps_to_the_right_status_and_section` |
| 13 | History reconstructs the current state | done | `history_is_replayed_into_one_current_row`, `an_older_email_read_later_never_overrides_the_newer_state` |
| 14 | Mail monitored only while the task is on | done | no worker; run-now guard; `turning_job_mail_sync_on_needs_a_connected_mailbox`; E2E (connect reads nothing, paused disables Run now) |
| 15 | Built-in "Job Mail & Interview Sync" | done | `BuiltinTask`; `job_mail_sync_is_a_single_built_in_task_that_needs_a_mailbox`; E2E |
| 16 | Initial lookback in days | done | TaskDialog; `taskForm.ts`; E2E |
| 17 | Frequency through the scheduler | done | existing scheduler controls; E2E |
| 18 | First run reads the lookback | done | `reading_plan`; `the_lookback_reads_n_days_first_then_only_changes`; E2E (30 days, older mail not read) |
| 19 | Later runs are incremental | done | `the_lookback_bootstraps_then_syncs_incrementally_and_backfills_once`; E2E (history / delta only) |
| 20 | Gmail and Outlook through the connector abstraction | done | `MailProvider`; tests for both; E2E |
| 21 | Only job-related mail reaches the model | done | prefilter + triage; tests; E2E (personal mail and newsletter never sent) |
| 22 | Confirmed interviews distinguished from requests | done | categories; tests |
| 23 | Only confirmed interviews are added automatically | done | `calendar_sync`; tests |
| 24 | Conflicts detected first | done | all calendars; `a_meeting_in_the_other_calendar_blocks_the_interview_too`; E2E |
| 25 | No duplicate events | done | `remaInterviewId`, fingerprints; tests; E2E |
| 26 | Reschedules update the event | done | tests; E2E (PATCH, no new event) |
| 27 | Calendar button top right | done | `AppShell.tsx`; E2E |
| 28 | It opens the calendar in ReMa | done | `CalendarPanel.tsx`; tests; E2E |
| 29 | Actual connected calendar events | done | `calendar_view.rs` (live reads); E2E |
| 30 | ReMa interviews appear | done | `merge`; `shows_both_calendars_and_marks_interviews_once`; E2E |
| 31 | Meeting links one click away | done | Join button; tests; E2E (opened in the browser) |
| 32 | Google and Outlook through one architecture | done | `CalendarProvider`; tests; E2E |
| 33 | No tokens reach the model | done | connector layer; existing token tests |
| 34 | Mail cannot override instructions | done | fenced data, no tools, schema validation; existing injection tests |
| 35 | Automated tests pass | done | [validation.md](validation.md) |
| 36 | Existing functionality builds and works | done | full test suites, build; [validation.md](validation.md) |

Not verified here: real Google and Microsoft sign-ins and APIs (needs the
publisher's app registrations; see
[../connectors/registration.md](../connectors/registration.md)), and native
notifications and the tray outside Linux/Xvfb.
