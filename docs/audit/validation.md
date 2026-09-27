# Harness audit — validation

What was run after the repairs in [implementation.md](implementation.md),
and what was observed. Environment: Linux container, Xvfb display, D-Bus
session with GNOME Keyring, debug build (`pnpm tauri build --debug
--no-bundle`). No live provider accounts or mailboxes were available; the
in-app runs used the stand-ins in `scripts/e2e/mock-providers.mjs`, which
enforce the documented provider rules and log every model request (tools,
flags, and whether credentials, private mail or sign-in tokens were in it).

## 1. Automated checks

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --all-targets --locked -- -D warnings` | clean |
| `cargo test --locked` | 743 passed, 3 ignored (the explicit-only tests, as before); baseline was 715 |
| `pnpm lint`, `pnpm typecheck` | clean |
| `pnpm test` | 129 passed (26 files); baseline was 121 (23 files) |
| `pnpm tauri build --debug --no-bundle` | built |

Regression tests per finding (numbers from implementation.md):

| # | Tests |
|---|---|
| 1 | `gemini_3_searches_next_to_functions_and_sends_its_parts_back`, `gemini_2_gets_remas_search_tools_instead_of_search_next_to_functions`, `only_gemini_3_and_later_combine_search_with_functions`, `keeps_the_turn_as_sent_for_the_next_round`, `recognizes_providers_refusing_their_web_tools` |
| 2 | `leaves_room_for_thinking_on_models_that_think_by_default` |
| 3 | `long_histories_keep_their_newest_turns`, `long_chats_send_their_newest_turns_and_say_so` |
| 4 | `a_round_longer_than_one_run_continues_where_it_stopped` |
| 5 | `answers_cut_off_at_the_output_limit_say_so`, `a_scheduled_answer_cut_off_at_the_output_limit_says_so`, `a_reply_cut_off_at_the_output_limit_is_reported_and_retried` |
| 6 | `a_model_that_cannot_use_tools_answers_with_an_honest_prompt` |
| 7 | `a_local_model_without_tool_support_still_answers_its_scheduled_prompt` |
| 8 | `calls_without_provider_ids_get_ids_no_other_call_has` |
| 9 | `a_long_tool_loop_ends_with_an_answer_instead_of_an_error`, `the_last_round_of_a_long_tool_loop_asks_for_no_tools`, `a_fetched_page_is_bounded_and_the_last_round_uses_no_tools` |
| 10 | `an_overloaded_provider_is_asked_again_before_anything_streamed`, `an_empty_quota_is_not_asked_again` |
| 11 | `inline_reasoning_is_left_out_even_when_its_tags_are_split` |
| 12 | `a_first_sync_at_its_limit_says_how_far_back_it_read`, `a_range_larger_than_one_read_is_read_from_its_recent_end`, `a_first_sync_cut_at_its_limit_reads_the_older_mail_in_later_runs` |
| 13, 23 | `a_chat_that_read_private_data_keeps_the_web_off_afterwards` (cloud and local model) |
| 14, 22 | `a_rejected_key_shows_in_settings_until_a_request_succeeds` (including a restart); Vitest `CloudProviderRow`, `ProviderRows` |
| 16 | `openai_reasoning_goes_back_with_the_calls_it_led_to` |
| 17 | `a_fetched_page_is_bounded_and_the_last_round_uses_no_tools` |
| 18 | `each_claude_console_request_takes_the_current_token` |
| 20 | Vitest `useChat` (another conversation's stream; a new chat's early text) |
| 21 | `maps_statuses_without_leaking_secrets` (Google's `API key not valid` / `API key expired`) |

The tests for 13/23 and 14/22 were also run against the code with the fix
taken out: both fail there.

## 2. In-app runs

| Scenario | Observed |
|---|---|
| Gemini 3 Flash, ReMa MCP on, "try the ReMa job tools" | Request: `mcp_rema_*` functions + `google_search` + `includeServerSideToolInvocations: true`. The stand-in searched (`toolCall`/`toolResponse`) and called `mcp_rema_search_jobs`; round 2 sent that turn back with the search parts, the thought signature and the call id, and `functionResponse.id` matched. `search_jobs` Done, answer shown. |
| Same, stand-in with invalid tool arguments | ReMa MCP refused the unknown field with a clear error, which went back to the model; the answer still came. |
| Gemini 2.5 Flash, same question | Request: `rema_career_search`, `rema_read_page` and `mcp_rema_*`, no `google_search`: no 400. Answer shown. |
| Google rejects the key (400 `API_KEY_INVALID`) | Chat: "Gemini rejected the API key: API key not valid. Please pass a valid API key." (one request, no retry). Settings: "Reauthentication required", "Gemini no longer accepts this API key. Replace it to keep using Gemini.", **Replace key**. Still so after an app restart. |
| Replace key while Google still rejects it | The key check fails in the form with Google's reason; status unchanged. |
| Replace key, Google accepts | "Connected · API key". |
| Ollama-like endpoint (`gemma2:2b`), chat | Request 1 with ReMa's tools → 400 "does not support tools"; request 2 without tools, prompt without web claims; notice "This model cannot use tools, so ReMa answered without web or MCP tools."; answer shown. |
| Same model, scheduled prompt task, Run now after an app restart | Same fallback in the scheduler (request 1 with `rema_career_search`/`rema_read_page` → 400; request 2 without tools or web claims); run Succeeded with the answer. |
| Gmail + Google Calendar connected (mock OAuth, PKCE), local model asks about job emails and tomorrow 09:00 | `mail_search` and `calendar_check_availability` Done; answer lists 8 job emails and the conflicting "Weekly sync". Model context: no personal mail, no sign-in tokens. |
| Follow-ups in that chat | Requests carry no `rema_career_search`/`rema_read_page` (web off); a neutral question shows the notice "This chat contains your mail, calendar or application data, so the model's web access is off. …"; questions about the user's own data get the connector tools instead. |
| Job Mail & Interview Sync, 240 messages in the lookback | Run 1 read the newest 200 (two list pages), noted "Gmail: more job mail than one run reads; older mail is read in the next runs.", coverage recorded at the oldest message read (20.7 days of 30). Run 2 read history (new mail) and backfilled the older range: 39 messages, coverage now the full 30 days, nothing read twice. Classification continued its backlog across runs (98, then 37 left). |
| Secrets | No API key text in the app logs, the stand-in's log or any SQLite column; no model request flagged with credentials. |

The first job-mail attempt showed coverage stuck at the full lookback; the
cause was the stand-in (fractional `internalDate` values, which Gmail never
sends), fixed in the stand-in before the run above.

## 3. Not verified here

- Live provider behavior: no live OpenAI, Anthropic, Gemini, Google or
  Microsoft accounts. Request shapes follow current documentation (Google's
  host is blocked here; its rules were confirmed through public GitHub
  issues and pull requests) and are enforced by adapter tests and the
  stand-ins.
- Outlook's partial delta round in the app (covered by
  `a_round_longer_than_one_run_continues_where_it_stopped`; 2,000+ changes
  were not generated in the app run).
- The system tray (no tray host in this environment).
