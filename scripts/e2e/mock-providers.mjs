#!/usr/bin/env node
// Local stand-ins for Google (OAuth, Gmail, Calendar), Microsoft (identity
// platform, Graph mail and calendar) and an OpenAI-compatible model, for
// end-to-end runs of a *debug* build of ReMa:
//
//   node scripts/e2e/mock-providers.mjs 8777
//   REMA_DEV_GOOGLE_CLIENT_ID=e2e-google REMA_DEV_MICROSOFT_CLIENT_ID=e2e-microsoft \
//   REMA_GOOGLE_BASE_URL=http://127.0.0.1:8777 REMA_MICROSOFT_BASE_URL=http://127.0.0.1:8777 \
//   PATH="$PWD/scripts/e2e/bin:$PATH" pnpm tauri dev
//
// The authorization endpoints answer like a user who signs in and allows
// access: they redirect to ReMa's loopback address with a code and the
// request's state. `scripts/e2e/bin/xdg-open` plays the system browser.
// Every request is logged to stdout (one JSON line) for the validation
// report. Release builds ignore all REMA_DEV_* and *_BASE_URL variables.

import http from 'node:http';

const port = Number(process.argv[2] ?? 8777);
const base = `http://127.0.0.1:${port}`;
const DAY = 86_400_000;

// ── Dates relative to now (interviews must lie in the future) ─────────
const WEEKDAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
const inDays = (n) => new Date(Date.now() + n * DAY);
const longDate = (d) => `${WEEKDAYS[d.getUTCDay()]}, ${d.getUTCDate()} ${MONTHS[d.getUTCMonth()]} ${d.getUTCFullYear()}`;
const isoDate = (d) => d.toISOString().slice(0, 10);
const interviewDay = inDays(5);
const b64url = (text) => Buffer.from(text).toString('base64url');

// ── Mailboxes ─────────────────────────────────────────────────────────
let historyId = 1000;
const gmail = [];
function addGmail(id, thread, from, subject, body, hoursAgo, labels = ['INBOX']) {
  historyId += 1;
  gmail.push({ id, thread, from, subject, body, at: Date.now() - hoursAgo * 3_600_000, labels, history: historyId });
}
const globexBody = `Hi Ana, we are happy to confirm your technical interview on ${longDate(interviewDay)} from 14:00 to 15:00 (Europe/Vienna time). Join here: https://meet.example.com/globex-1 . Kind regards, Globex Talent Team`;
addGmail('g1', 'gt1', 'Acme Careers <jobs@acme.com>', 'Your application: Backend Engineer', 'Thank you for applying for Backend Engineer. We received your application.', 50);
addGmail('g2', 'gt2', 'Job Alerts <alerts@jobs.example>', '12 new jobs for you', 'Recommended jobs this week. PRIVATE-NEWSLETTER-CONTENT', 40);
addGmail('g3', 'gt3', 'Globex Recruiting <talent@globex.com>', 'Interview confirmation - Data Engineer', globexBody, 30);
addGmail('g4', 'gt4', 'Initech HR <hr@initech.com>', 'Your application at Initech', 'Unfortunately we decided to move forward with other candidates.', 20);
addGmail('g5', 'gt5', 'Mom <mom@family.net>', 'Dinner on Sunday', 'PRIVATE-FAMILY-CONTENT see you', 10);

let deltaToken = 1;
const outlook = [
  {
    id: 'o1',
    conversationId: 'oc1',
    from: { name: 'Contoso Talent', address: 'talent@contoso.com' },
    subject: 'Offer letter - Cloud Engineer',
    body: 'We are delighted to offer you the Cloud Engineer position. Please reply by Friday.',
    at: Date.now() - 5 * 3_600_000,
    token: 1,
  },
];

const events = [
  // Busy the day after the interview; not a conflict.
  {
    id: 'busy1',
    summary: 'Team offsite',
    start: { dateTime: `${isoDate(inDays(6))}T08:00:00Z` },
    end: { dateTime: `${isoDate(inDays(6))}T16:00:00Z` },
    transparency: 'opaque',
    status: 'confirmed',
  },
];
let eventSeq = 1;

// ── Model answers (OpenAI-compatible) ─────────────────────────────────
const answers = {
  'Your application: Backend Engineer': {
    category: 'application_received', confidence: 0.95, company: 'Acme', role: 'Backend Engineer',
    reference: null, stage: null, action_required: false, next_action: null,
    summary: 'Application received.', existing_application_id: null, contacts: [], interview: null,
  },
  'Interview confirmation - Data Engineer': {
    category: 'interview_confirmed', confidence: 0.97, company: 'Globex', role: 'Data Engineer',
    reference: null, stage: 'Technical interview', action_required: false, next_action: null,
    summary: 'Technical interview confirmed.', existing_application_id: null, contacts: ['Globex Talent Team'],
    interview: {
      state: 'confirmed', date: isoDate(interviewDay), start_time: '14:00', end_time: '15:00',
      duration_minutes: null, timezone: 'Europe/Vienna',
      datetime_quote: `${longDate(interviewDay)} from 14:00 to 15:00`, timezone_quote: 'Europe/Vienna time',
      type: 'Technical interview', location: null, meeting_url: 'https://meet.example.com/globex-1',
      participants: [], interviewer: null, proposed_slots: [], unclear: null,
    },
  },
  'Your application at Initech': {
    category: 'rejection', confidence: 0.97, company: 'Initech', role: 'QA Engineer',
    reference: null, stage: null, action_required: false, next_action: null,
    summary: 'Rejected.', existing_application_id: null, contacts: [], interview: null,
  },
  'Offer letter - Cloud Engineer': {
    category: 'offer', confidence: 0.99, company: 'Contoso', role: 'Cloud Engineer',
    reference: null, stage: null, action_required: true, next_action: 'Reply by Friday',
    summary: 'Offer received.', existing_application_id: null, contacts: [], interview: null,
  },
};

function modelReply(body) {
  const user = [...(body.messages ?? [])].reverse().find((m) => m.role === 'user');
  const content = typeof user?.content === 'string' ? user.content : JSON.stringify(user?.content ?? '');
  const marker = content.indexOf('Emails:\n');
  if (marker >= 0) {
    const emails = JSON.parse(content.slice(marker + 8));
    const relevant = emails.filter((e) => !/new jobs|dinner/i.test(e.subject)).map((e) => e.id);
    return JSON.stringify({ relevant });
  }
  const subject = content.split('\n').find((l) => l.startsWith('Subject: '))?.slice(9);
  if (subject !== undefined) {
    return JSON.stringify(answers[subject] ?? { category: 'not_job_related', confidence: 0.9 });
  }
  return 'Mock answer from the local model.';
}

// ── HTTP helpers ──────────────────────────────────────────────────────
const idToken = (claims) => `h.${b64url(JSON.stringify(claims))}.s`;
const codes = new Map();
const log = (entry) => process.stdout.write(`${JSON.stringify({ at: new Date().toISOString(), ...entry })}\n`);

function send(res, status, body, headers = {}) {
  const text = typeof body === 'string' ? body : JSON.stringify(body);
  res.writeHead(status, { 'content-type': 'application/json', ...headers });
  res.end(text);
}

function gmailResource(m, format) {
  const headers = [
    { name: 'From', value: m.from },
    { name: 'To', value: 'ana@gmail.com' },
    { name: 'Subject', value: m.subject },
  ];
  const payload = { mimeType: 'text/plain', headers };
  if (format === 'full') payload.body = { data: b64url(m.body) };
  return { id: m.id, threadId: m.thread, labelIds: m.labels, snippet: m.body.slice(0, 60), internalDate: String(m.at), payload };
}

function graphMessage(m, withBody) {
  const message = {
    id: m.id,
    conversationId: m.conversationId,
    receivedDateTime: new Date(m.at).toISOString(),
    subject: m.subject,
    bodyPreview: m.body.slice(0, 60),
    from: { emailAddress: m.from },
    toRecipients: [{ emailAddress: { name: 'Ana', address: 'ana@outlook.com' } }],
    webLink: `https://outlook.live.com/owa/?ItemID=${m.id}`,
    isDraft: false,
    categories: [],
  };
  if (withBody) message.body = { contentType: 'text', content: m.body };
  return message;
}

function overlaps(e, min, max) {
  const s = Date.parse(e.start.dateTime);
  const en = Date.parse(e.end.dateTime);
  return s < max && min < en;
}

// ── Routes ────────────────────────────────────────────────────────────
function route(req, url, body, res) {
  const p = url.pathname;
  const q = url.searchParams;
  const form = new URLSearchParams(body);

  // Model
  if (p === '/v1/models') return send(res, 200, { data: [{ id: 'mock-classifier', object: 'model' }] });
  if (p === '/v1/chat/completions') {
    const text = modelReply(JSON.parse(body || '{}'));
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: { content: text } }] })}\n\n`);
    res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] })}\n\n`);
    return res.end('data: [DONE]\n\n');
  }

  // Authorization: the user signs in and allows access.
  if (p === '/o/oauth2/v2/auth' || p === '/common/oauth2/v2.0/authorize') {
    const code = `code-${codes.size + 1}`;
    codes.set(code, { scope: q.get('scope'), challenge: q.get('code_challenge'), redirect: q.get('redirect_uri') });
    const target = `${q.get('redirect_uri')}/?code=${code}&state=${encodeURIComponent(q.get('state'))}`;
    res.writeHead(302, { location: target });
    return res.end();
  }
  if (p === '/token' || p === '/common/oauth2/v2.0/token') {
    const google = p === '/token';
    if (form.get('grant_type') === 'authorization_code') {
      const grant = codes.get(form.get('code'));
      if (!grant || grant.redirect !== form.get('redirect_uri') || !form.get('code_verifier')) {
        return send(res, 400, { error: 'invalid_grant' });
      }
      return send(res, 200, {
        access_token: `${google ? 'g' : 'm'}-at-${Date.now()}`,
        refresh_token: `${google ? 'g' : 'm'}-rt-1`,
        expires_in: 3599,
        token_type: 'Bearer',
        scope: grant.scope,
        id_token: google
          ? idToken({ sub: 'g-123', email: 'ana@gmail.com', name: 'Ana Example' })
          : idToken({ oid: 'm-oid', preferred_username: 'ana@outlook.com', name: 'Ana Example' }),
      });
    }
    return send(res, 200, { access_token: `${google ? 'g' : 'm'}-at-${Date.now()}`, expires_in: 3599, ...(google ? {} : { refresh_token: `m-rt-${Date.now()}` }) });
  }
  if (p === '/revoke') return send(res, 200, {});

  // Gmail
  if (p === '/gmail/v1/users/me/profile') return send(res, 200, { emailAddress: 'ana@gmail.com', historyId: String(historyId) });
  if (p === '/gmail/v1/users/me/messages') {
    const after = Number((q.get('q') ?? '').match(/after:(\d+)/)?.[1] ?? 0) * 1000;
    const found = gmail.filter((m) => m.at >= after).sort((a, b) => b.at - a.at);
    return send(res, 200, { messages: found.map((m) => ({ id: m.id, threadId: m.thread })), resultSizeEstimate: found.length });
  }
  let match = p.match(/^\/gmail\/v1\/users\/me\/messages\/([^/]+)$/);
  if (match) {
    const m = gmail.find((x) => x.id === match[1]);
    return m ? send(res, 200, gmailResource(m, q.get('format'))) : send(res, 404, { error: { code: 404 } });
  }
  if (p === '/gmail/v1/users/me/history') {
    const start = Number(q.get('startHistoryId'));
    const added = gmail.filter((m) => m.history > start);
    return send(res, 200, {
      history: added.map((m) => ({ id: String(m.history), messagesAdded: [{ message: { id: m.id, threadId: m.thread, labelIds: m.labels } }] })),
      historyId: String(historyId),
    });
  }
  match = p.match(/^\/gmail\/v1\/users\/me\/threads\/([^/]+)$/);
  if (match) return send(res, 200, { messages: gmail.filter((m) => m.thread === match[1]).map((m) => gmailResource(m, 'metadata')) });

  // Google Calendar
  if (p === '/calendar/v3/calendars/primary/events' && req.method === 'GET') {
    const property = q.get('privateExtendedProperty');
    let items = events.filter((e) => e.status !== 'cancelled');
    if (property) {
      const [key, value] = property.split('=');
      items = items.filter((e) => e.extendedProperties?.private?.[key] === value);
    } else {
      const min = Date.parse(q.get('timeMin'));
      const max = Date.parse(q.get('timeMax'));
      items = items.filter((e) => overlaps(e, min, max));
    }
    return send(res, 200, { items });
  }
  if (p === '/calendar/v3/calendars/primary/events' && req.method === 'POST') {
    const event = { ...JSON.parse(body), id: `evt${eventSeq++}`, status: 'confirmed' };
    events.push(event);
    return send(res, 200, event);
  }
  match = p.match(/^\/calendar\/v3\/calendars\/primary\/events\/([^/]+)$/);
  if (match) {
    const event = events.find((e) => e.id === match[1]);
    if (!event) return send(res, 404, { error: { code: 404 } });
    if (req.method === 'PATCH') Object.assign(event, JSON.parse(body));
    return send(res, 200, event);
  }
  if (p === '/calendar/v3/freeBusy') {
    const request = JSON.parse(body);
    const min = Date.parse(request.timeMin);
    const max = Date.parse(request.timeMax);
    const busy = events.filter((e) => e.transparency !== 'transparent' && overlaps(e, min, max)).map((e) => ({ start: e.start.dateTime, end: e.end.dateTime }));
    return send(res, 200, { calendars: { primary: { busy } } });
  }

  // Microsoft Graph
  if (p === '/graph/v1.0/me') return send(res, 200, { id: 'm-oid', displayName: 'Ana Example', mail: 'ana@outlook.com', userPrincipalName: 'ana@outlook.com' });
  if (p === '/graph/v1.0/me/mailFolders/inbox/messages/delta') {
    const since = Number(q.get('$deltatoken') ?? 0);
    const value = outlook.filter((m) => m.token > since).map((m) => graphMessage(m, false));
    deltaToken = Math.max(deltaToken, ...outlook.map((m) => m.token));
    return send(res, 200, { value, '@odata.deltaLink': `${base}/graph/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken=${deltaToken}` });
  }
  match = p.match(/^\/graph\/v1\.0\/me\/messages\/([^/]+)$/);
  if (match) {
    const m = outlook.find((x) => x.id === match[1]);
    return m ? send(res, 200, graphMessage(m, true)) : send(res, 404, { error: { code: 'ErrorItemNotFound' } });
  }
  if (p === '/graph/v1.0/me/messages') return send(res, 200, { value: outlook.map((m) => graphMessage(m, false)) });
  if (p === '/graph/v1.0/me/calendarView' || p === '/graph/v1.0/me/events') return send(res, 200, { value: [] });
  if (p === '/graph/v1.0/me/calendar/getSchedule') return send(res, 200, { value: [{ scheduleId: 'ana@outlook.com', scheduleItems: [] }] });

  return send(res, 404, { error: 'not mocked', path: p });
}

http
  .createServer((req, res) => {
    let body = '';
    req.on('data', (chunk) => (body += chunk));
    req.on('end', () => {
      const url = new URL(req.url, base);
      const auth = req.headers.authorization ?? '';
      log({
        method: req.method,
        path: url.pathname,
        query: url.search.length > 300 ? `${url.search.slice(0, 300)}…` : url.search,
        // Never the token itself, only whether one was sent.
        bearer: auth.startsWith('Bearer ') ? 'yes' : 'no',
        form: req.headers['content-type']?.includes('form') ? [...new URLSearchParams(body).keys()] : undefined,
      });
      try {
        route(req, url, body, res);
      } catch (error) {
        send(res, 500, { error: String(error) });
      }
    });
  })
  .listen(port, '127.0.0.1', () => log({ listening: base }));
