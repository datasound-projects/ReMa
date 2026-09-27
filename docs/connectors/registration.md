# Connectors — app registration (for whoever builds and publishes ReMa)

ReMa signs users in with **its own** Google, Microsoft and LinkedIn app registrations.
Users never create OAuth clients, never see a client ID or secret, and never
open a cloud console. This page is for the publisher of a ReMa build.

> These steps follow Google's and Microsoft's documentation for installed
> apps. The documentation hosts (`developers.google.com`,
> `learn.microsoft.com`) were not reachable from the environment this was
> written in; check the current console labels against the official pages
> linked below before registering.

## Build configuration

The registrations are compiled into the app (`option_env!`, read in
`src-tauri/src/connectors/google/mod.rs` and `microsoft/mod.rs`):

| Variable | Required | Meaning |
|---|---|---|
| `REMA_GOOGLE_CLIENT_ID` | for Gmail / Google Calendar | Google OAuth client of type **Desktop app** |
| `REMA_GOOGLE_CLIENT_SECRET` | with the Google client | the Desktop client's secret (Google does not treat it as confidential for installed apps; it is still never shown or logged) |
| `REMA_MICROSOFT_CLIENT_ID` | for Outlook Mail / Calendar | Microsoft Entra application (client) ID |
| `REMA_MICROSOFT_TENANT` | no (default `common`) | `common` (work, school and personal accounts), `consumers`, `organizations` or a tenant ID |
| `REMA_LINKEDIN_CLIENT_ID` | for LinkedIn (Network Connect) | client ID of a LinkedIn app with native PKCE enabled; no secret exists or ships |
| `REMA_LINKEDIN_APPROVED_SCOPES` | no | restricted scopes LinkedIn approved for that app, space-separated; only `r_1st_connections` is recognised |

```sh
REMA_GOOGLE_CLIENT_ID=… REMA_GOOGLE_CLIENT_SECRET=… REMA_MICROSOFT_CLIENT_ID=… REMA_LINKEDIN_CLIENT_ID=… pnpm build:app
```

A build without a registration shows the provider's connectors as
"Unavailable"; it never asks users for credentials. `build.rs` rebuilds when
these variables change.

**Development only** (ignored by release builds): `REMA_DEV_GOOGLE_CLIENT_ID`,
`REMA_DEV_GOOGLE_CLIENT_SECRET`, `REMA_DEV_MICROSOFT_CLIENT_ID`,
`REMA_DEV_LINKEDIN_CLIENT_ID` and `REMA_DEV_LINKEDIN_APPROVED_SCOPES` override
the registration at run time; `REMA_GOOGLE_BASE_URL`, `REMA_MICROSOFT_BASE_URL`
and `REMA_LINKEDIN_BASE_URL` point the endpoints at a local mock (see
[validation.md](validation.md) and
[network-connect/validation.md](../network-connect/validation.md)).

## Google (Gmail, Google Calendar)

Official guides: [OAuth 2.0 for iOS & Desktop Apps](https://developers.google.com/identity/protocols/oauth2/native-app),
[Gmail API scopes](https://developers.google.com/workspace/gmail/api/auth/scopes),
[Calendar API scopes](https://developers.google.com/workspace/calendar/api/auth),
[OAuth app verification](https://support.google.com/cloud/answer/13463073).

1. Create (or choose) a Google Cloud project owned by the publisher.
2. **APIs & Services → Library**: enable the **Gmail API** and the **Google Calendar API**.
3. **Google Auth Platform → Branding** (OAuth consent screen): app name "ReMa", support email, logo, home page, privacy policy and terms of service URLs, authorized domain of those pages, developer contact.
4. **Audience**: user type **External**. While the app is in *Testing*, only listed test users can sign in, and Google expires their refresh tokens after 7 days (users then see "Reconnect needed" in ReMa).
5. **Data access**: add exactly these scopes:
   - `openid`, `…/auth/userinfo.email` (`email`), `…/auth/userinfo.profile` (`profile`)
   - `https://www.googleapis.com/auth/gmail.readonly` (restricted)
   - `https://www.googleapis.com/auth/calendar.events` (sensitive)
   - `https://www.googleapis.com/auth/calendar.freebusy`
6. **Clients → Create client → Application type: Desktop app.** No redirect URI is configured: desktop clients accept loopback redirects (`http://127.0.0.1:<any port>`), which is what ReMa uses.
7. Put the client ID and secret into the build variables above.
8. Before publishing to everyone: complete **verification** (see [compliance.md](compliance.md)).

What ReMa sends: `response_type=code`, `code_challenge` (S256), a random
`state`, `access_type=offline` (refresh token), `prompt=consent
select_account`, and only the scopes of the connectors being enabled.

## Microsoft (Outlook Mail, Outlook Calendar)

Official guides: [Register an application](https://learn.microsoft.com/entra/identity-platform/quickstart-register-app),
[Redirect URI (reply URL) restrictions — localhost](https://learn.microsoft.com/entra/identity-platform/reply-url),
[OAuth 2.0 authorization code flow](https://learn.microsoft.com/entra/identity-platform/v2-oauth2-auth-code-flow),
[Microsoft Graph permissions reference](https://learn.microsoft.com/graph/permissions-reference),
[Publisher verification](https://learn.microsoft.com/entra/identity-platform/publisher-verification-overview).

1. **Microsoft Entra admin center → App registrations → New registration.** Name "ReMa". Supported account types: **Accounts in any organizational directory and personal Microsoft accounts** (matches the `common` tenant).
2. **Authentication → Add a platform → Mobile and desktop applications.** Add the redirect URI **`http://localhost`** (no port: Microsoft matches any port on `localhost` for native clients; ReMa picks a free one per sign-in and listens on IPv4 and IPv6 loopback).
3. Do **not** create a client secret or certificate. ReMa is a public client and redeems codes with PKCE only.
4. **API permissions → Microsoft Graph → Delegated**: `openid`, `profile`, `email`, `offline_access`, `User.Read`, `Mail.Read`, `Calendars.ReadWrite`. None needs admin consent by default; organizations can still require it.
5. **Branding & properties**: publisher domain, home page, terms and privacy URLs, logo. Complete **publisher verification** (Microsoft AI Cloud Partner Program ID) so users see a verified publisher; some tenants block consent to unverified multi-tenant apps.
6. Put the Application (client) ID into `REMA_MICROSOFT_CLIENT_ID`.

What ReMa sends: `response_type=code`, `response_mode=query`,
`code_challenge` (S256), a random `state`, `prompt=select_account`, and the
scopes of the connectors being enabled (plus `offline_access`). Refresh
tokens rotate: ReMa stores the new one after every refresh. Microsoft offers
no token revocation for public clients; users remove ReMa at
<https://account.microsoft.com/privacy/app-access> (linked from the connector
details).

## LinkedIn (Network Connect)

Official guides: [Sign In with LinkedIn using OpenID Connect](https://learn.microsoft.com/linkedin/consumer/integrations/self-serve/sign-in-with-linkedin-v2),
[Authorization Code Flow (Native PKCE)](https://learn.microsoft.com/linkedin/shared/authentication/authorization-code-flow-native),
[Getting access to LinkedIn APIs](https://learn.microsoft.com/linkedin/shared/authentication/getting-access),
[Connections API](https://learn.microsoft.com/linkedin/shared/integrations/people/connections-api).
These pages were read through search excerpts only (the host was
unreachable here); check them before registering.

1. In the LinkedIn Developer Portal, create an app owned by the publisher's
   LinkedIn Page, with privacy policy URL and logo.
2. **Products**: add **Sign In with LinkedIn using OpenID Connect** (scopes
   `openid`, `profile`, `email`, self-service).
3. **Native PKCE**: LinkedIn enables the native (secret-less) PKCE flow per
   app on request, through the developer's LinkedIn contact. Without it the
   token request fails. ReMa never ships a client secret.
4. **Redirect**: the native-PKCE guide says LinkedIn communicates only with
   loopback IPs and the app listens on a random loopback port, which is what
   ReMa does (`http://127.0.0.1:<random port>` per sign-in). Configure the
   app's auth settings as that guide says; a live sign-in against LinkedIn
   was not possible here, so confirm it before release.
5. Put the client ID into `REMA_LINKEDIN_CLIENT_ID`.
6. Only if LinkedIn approves the app for **first-degree connections**
   (`r_1st_connections`, restricted to approved developers): add
   `REMA_LINKEDIN_APPROVED_SCOPES=r_1st_connections`. ReMa then asks for it
   and uses it only when the token actually grants it; otherwise the
   Network Connect card says "Not available". Sales enrichment would need
   the Sales Navigator Application Platform; ReMa does not use LinkedIn data
   in Business at all.

What ReMa sends: `response_type=code`, `code_challenge` (S256), a random
`state`, `openid profile email` (plus an approved scope), no secret. A build
without `REMA_LINKEDIN_CLIENT_ID` shows LinkedIn as unavailable in this
build. Disconnecting deletes ReMa's tokens and drops any connection data
from memory; users remove ReMa from their account in LinkedIn's settings
(permitted services), linked from the connector details.

## XING

Nothing to register. XING's "Login with XING" is a website plugin bound to
a registered domain, and XING's official API clients state that new API
applications can no longer be registered. ReMa therefore shows XING as
unavailable and never asks for a XING password or cookies.
