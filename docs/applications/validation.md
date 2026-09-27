# Profile context, Applications, Job Mail & Interview Sync and the calendar — validation report

What was run and what was observed. Environment: Linux container, Xvfb
display, D-Bus session with GNOME Keyring as the credential store, Rust
1.98.1 (the CI's stable). No real Google or Microsoft registration was
available, so live sign-ins were **not** performed; the in-app run used local
stand-ins that follow the providers' documented request and response formats
(`scripts/e2e/mock-providers.mjs`).

## 1. Automated checks

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --all-targets --locked -- -D warnings` | clean |
| `cargo test --locked` | 468 passed, 3 ignored (pre-existing explicit-only tests), including the generated-bindings check |
| `pnpm lint`, `pnpm typecheck` | clean |
| `pnpm test` | 87 passed (19 files) |
| `pnpm build` | built |

The tests the specification asks for (§18), and where they are:

- **Profile context** (`services/profile_context/tests.rs`, `services/chat.rs`): Custom Profile only; CV only; both; the Custom Profile overriding a conflicting CV field; credentials supplementing (and replacing a vaguer CV mention); Profile on and off (`profile_context_is_sent_only_when_turned_on`); the Profile edited between two messages (`the_next_message_uses_the_edited_profile`); duplicates normalized; excerpts only when needed; the character budget.
- **Application state** (`jobs/tests.rs`): every category to its status and section; status transitions and a replayed history (`history_is_replayed_into_one_current_row`); an older email read later (`an_older_email_read_later_never_overrides_the_newer_state`); same company with different roles and same role with different job IDs (`applications_are_told_apart_by_role_and_job_id`); the same email in two mailboxes (`the_same_email_in_gmail_and_outlook_is_processed_once`); an unclear classification (`an_unclear_confirmation_changes_nothing`); rejection reasons only from the email; concise latest-update texts.
- **Scheduled tracking** (`jobs/tests.rs`, `services/tasks.rs`, `db/mod.rs`): the N-day bootstrap, incremental runs, a longer lookback read once, a shorter one reading nothing (`the_lookback_bootstraps_then_syncs_incrementally_and_backfills_once`, `the_lookback_reads_n_days_first_then_only_changes`); one built-in task that needs a mailbox; turning it on without a mailbox; a paused task that cannot be run; an old mail task that never runs; the migration (`one_mail_task_becomes_job_mail_sync_and_nothing_else_reads_mail`); connecting reads nothing and claims no sync (`connectors/tests.rs`).
- **Calendar** (`jobs/tests.rs`, `services/calendar_view.rs`, `connectors/calendar.rs`): a confirmed interview added to a free calendar; a request adding nothing; conflicts, including one only in the other calendar (`a_meeting_in_the_other_calendar_blocks_the_interview_too`); meeting links (https only); duplicates prevented; reschedule; cancellation; Google and Outlook (`outlook_calendar_gets_the_interview_like_google_calendar`); both calendars in the panel with each interview once.
- **Interface** (Vitest): navigation order; Profile tabs and copy; Portfolio Studio page; the five Applications sections (columns, collapse, empty states, tracking notice); the built-in task card (set up, on/off, no Run now while off, old mail tasks); the calendar panel (empty state, both calendars, Join, next week, renamed events); the scroll-to-section hook.

## 2. In-app end-to-end (debug build)

The debug build ran against the mock providers on `127.0.0.1:8777` (Google
OAuth, Gmail, Google Calendar; Microsoft identity platform, Graph mail and
calendar; an OpenAI-compatible model that logs the subject of every email it
is asked about and whether any request contained the private test markers).
It was driven with xdotool; screenshots were checked at each step.

Mailboxes: in Gmail, an application confirmation (Acme), a newsletter, a
confirmed interview in 5 days on a free slot (Globex), a rejection without a
reason (Initech), an assessment request (Umbrella), an application update
(Hooli), a personal email, a rejection with a reason (Stark), a confirmed
interview in 6 days that overlaps a Google Calendar event (Soylent), and a
45-day-old confirmation (Wayne). In Outlook, an offer (Contoso), the Acme
confirmation again (a forwarding alias), and a 40-day-old confirmation
(Initrode). Google Calendar: "Weekly sync" with a Meet link and a "Team
offsite"; Outlook Calendar: "Dentist" in 2 days.

| Step | Observed |
|---|---|
| Upgrade: a version-8 database from the earlier connector run (Outlook Mail connected with background sync) | Migrated to version 9; background sync off; **no mail request in 30 s**; the four existing applications appear in their sections; notice "ReMa is not reading your mail. Turn on Job Mail & Interview Sync …". |
| Navigation | Chat, Agents, Scheduled Tasks, Profile, Portfolio Studio, Applications; Settings at the bottom; calendar button top right. |
| Profile, Portfolio Studio | Two tabs with their descriptions and the context note; Portfolio Studio as its own page. |
| Calendar with no calendar connected | "No calendar connected." / "Connect Google Calendar or Outlook Calendar in Settings → Connectors." / **Open Connectors** scrolls to Connectors. |
| Connect Gmail, Google Calendar, Outlook Calendar | Only authorization and token requests; **no Gmail or Graph mail request**; no "Synced" time on the cards. Settings shows "Job mail tracking · Off". |
| Scheduled Tasks | "Built-in" card: icon, the specified description, "Off. ReMa does not read your mail until you set this up." **Set up** shows the mail sources (Gmail, Outlook Mail), "Initial lookback: last [30] days", the calendar option naming both calendars, instructions, model, schedule (every hour). **Turn on** → Active. |
| Run now (first run) | Gmail search `after:` = now − 30 days; Outlook delta with `receivedDateTime ge` now − 30 days; the 45- and 40-day-old emails not read. Newsletter and personal email filtered before any model request (`leaked=False` on every request). Acme classified **once** (the Outlook copy skipped). Each interview: Google **and** Outlook checked; Globex created in Google Calendar; Soylent not created (conflict). |
| Applications | Interviews Confirmed: Soylent ("… Calendar conflict: not added to your calendar.", conflict badge, meeting link), Globex ("Interview confirmed for Oct 2 at 14:00 CEST."). Applications Confirmed: Acme ("Application received successfully. No action required."). Needs Your Action: Contoso (Offer received, "Reply to the offer by Friday."), Umbrella (Assessment requested, "Complete the online coding assessment by 3 October."). In Progress: Hooli ("Application moved to hiring manager review."). Rejected: Stark ("They chose candidates with more hands-on Kubernetes experience."), Initech ("No reason provided."). Newest first; sections collapse. |
| Calendar panel | Weekly sync (Google, **Join**), Dentist (Outlook), Globex highlighted with **View application** and **Join**, Team offsite, Soylent "Calendar conflict: not added". **Join** opened `https://meet.example.com/globex-1` in the system browser; **View application** opened the Globex detail ("In Google Calendar", timeline, correspondence). |
| Next emails: Globex moved two days later; Vandelay interview at the time of the Outlook "Dentist" | Run now: Gmail `history.list` and Outlook delta only. Globex: free in both calendars → the **same** event updated (`PATCH`), status "Interview rescheduled", panel shows it in the next week. Vandelay: free in Google, busy in Outlook → flagged, nothing created, notification "Conflict detected with: Dentist 09:00–10:00". |
| Lookback 30 → 60 days | Saving read nothing. The next run read the added range once (Gmail dated search, Outlook `receivedDateTime` range 60–30 days) and added Wayne (Aug 13) and Initrode (Aug 18). The run after that: `history.list` and delta only. |
| Lookback 60 → 10 days | Incremental only; 11 applications and 15 mail records before and after. |
| Pause | Card "Paused"; **Run now** disabled on the card and in the menu; Applications shows the "not reading your mail" notice again. |
| Dark theme | Applications, calendar panel and notice render with the dark tokens. |
| Fresh install (final build) | Database version 9; Applications shows "Connect Gmail or Outlook Mail in Settings → Connectors, then turn on Job Mail & Interview Sync in Scheduled Tasks." and five empty sections. |

Found and fixed during the run:

- reconnecting (or adding a calendar to a connected account) recorded a successful sync ("Synced just now") although nothing was read, which would also have moved the point a recovery resumes from: it now only clears the error;
- the conflict check looked at one calendar: it now checks every connected calendar (also for manual adds and proposed times);
- "Run now" in the task menu, the task page and the backend still worked while the task was paused; a mail task left from before the built-in one could still run: both are refused in Rust and disabled in the interface, and the migration pauses such tasks;
- "Open Connectors" did nothing when Settings was already open (a scroll helper never scrolled a settled page);
- the "not reading your mail" notice had a column layout and a doubled gap; the interview link in the calendar repeated the event title; the built-in card repeated its name; Portfolio Studio said the same sentence twice.

## 3. Not verified here

- Real Google and Microsoft sign-ins, consent screens and API responses (needs the publisher's registrations; see [../connectors/registration.md](../connectors/registration.md)).
- A real model classifying real mail (the run used a scripted model; classification rules and validation are covered by the tests above).
- The system tray menu (Xvfb has no tray host) and native notifications on macOS and Windows.
