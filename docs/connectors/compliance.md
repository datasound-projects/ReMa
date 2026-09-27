# Connectors — compliance and data handling

What publishing ReMa with Gmail and Outlook access requires, and how the
implementation meets it. This is engineering guidance, not legal advice.

> The official policy pages could not be fetched from the environment this
> was written in. The requirements below reflect Google's and Microsoft's
> published policies as known at the time of writing; confirm them against
> the linked pages before submitting for verification.

## Google: restricted and sensitive scopes

| Scope | Google class | Why ReMa needs it |
|---|---|---|
| `gmail.readonly` | **restricted** | find job-application emails and read the relevant ones; no modify, compose or send |
| `calendar.events` | sensitive | read events for conflict checks; create and update the interview events ReMa manages |
| `calendar.freebusy` | — | availability without event details |

Restricted scopes require, before the app can leave *Testing* for all users
([OAuth app verification](https://support.google.com/cloud/answer/13463073),
[restricted scope verification](https://support.google.com/cloud/answer/13464325)):

1. **Brand and domain verification**: verified home page and privacy policy on a domain the publisher owns.
2. **Scope justification** for each sensitive and restricted scope, with a **demo video** showing the consent screen, the Settings → Connectors flow, and how each scope is used (tracking applications, conflict checks, adding an interview after the user confirms).
3. **Security assessment** (CASA, performed by an authorized assessor) for restricted scopes, renewed yearly. Google scopes the assessment by where restricted data goes; ReMa stores Gmail data only on the user's device, but sends selected email text to the model provider the user chose. Confirm the required tier with Google during verification.
4. **Compliance with the [Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy)**, including the **Limited Use** requirements.

### Limited Use: how ReMa complies

| Requirement | ReMa |
|---|---|
| Use Google user data only to provide or improve user-facing features prominent in the app | Gmail data is used only for the Applications tracker, notifications, interview calendar handling and the user's own chat questions about applications. |
| No transfer to third parties except as necessary for those features, with user consent, for security, or by law | There is no ReMa server. Only job-related email text is sent, from the user's computer, to the **model provider the user configured** to classify it or answer the user's question. Unrelated mail is filtered deterministically and never read in full or sent. |
| No use for advertising; no sale | None. |
| No human reading, except with consent, for security, legal reasons or on aggregated anonymized data | No ReMa staff can access user data (nothing leaves the device except to the user's chosen model provider). |
| No use to develop, improve or train generalized AI/ML models | ReMa does not train models. Provider requests are inference only; users choose the provider (its own data terms apply; API providers do not train on API data by default). |

**Privacy policy** must state: what Gmail and Calendar data is accessed
(metadata of recent mail, full text of job-related mail, calendar events and
free/busy), that it is processed and stored locally, that job-related email
text is sent to the user's chosen AI provider for classification, how to
disconnect and revoke access, and the Limited Use statement: *"ReMa's use and
transfer of information received from Google APIs will adhere to the Google
API Services User Data Policy, including the Limited Use requirements."*

## Microsoft

- Public client, delegated permissions only (`Mail.Read`, `Calendars.ReadWrite`, `User.Read`, `offline_access`, OpenID scopes); no application permissions, no client secret.
- **Publisher verification** is recommended (and effectively required by many tenants for multi-tenant apps).
- Honour the [Microsoft APIs Terms of Use](https://learn.microsoft.com/legal/microsoft-apis/terms-of-use) and describe Outlook data handling in the privacy policy the same way as for Gmail.

## Data handling in the implementation

| Data | Where | Retention |
|---|---|---|
| OAuth access/refresh tokens | OS credential store (`connector:google`, `connector:microsoft`) | until disconnect or a rejected refresh |
| Account (email, name, granted scopes, status) | SQLite `connector_accounts` | until the account's last connector is disconnected |
| Sync cursors (Gmail `historyId`, Graph `deltaLink`) | SQLite `sync_cursors` | until the mail connector is disconnected |
| Filtered (unrelated) mail | SQLite `mail_messages`: id, thread, date, sender **domain** only | kept to avoid re-reading |
| Job-related mail | SQLite: sender, subject, link, category, confidence, validated extraction; **no body** | with the application |
| Applications, timeline, interviews | SQLite | kept after disconnect (the user's own records) |

Never stored or sent: tokens, authorization codes, PKCE verifiers, client
secrets (in logs, SQLite, settings, the interface, a model's context);
unrelated mail (to a model); the mailbox as a whole.

Prompt injection is handled at the architecture level: pipeline requests
carry no tools and no web access; email text is fenced, untrusted data; the
model's answer is parsed into a strict schema and validated (dates, times and
time zones must be quoted from the email); in chat, connector data is marked
untrusted, answers that read it get no web access, every change needs the
user's approval, and MCP calls after private data need approval. No tool can
change connector configuration, request more access, read tokens or send
mail.
