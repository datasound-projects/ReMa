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
// report, and so is every email the model is asked about (subject only).
// Release builds ignore all REMA_DEV_* and *_BASE_URL variables.
//
// The mailboxes cover each Applications section, a confirmed interview on
// a free slot and one that conflicts, an email older than the default
// 30-day lookback in each mailbox (read once the lookback is raised), the
// same email in Gmail and Outlook, and personal mail that must never reach
// the model. `POST /__e2e/next` delivers two more emails: the Globex
// interview moves to a new (free) time, and a Vandelay interview lands on
// a time that is free in Google Calendar but busy in Outlook Calendar.

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
const HOUR = 3_600_000;
const b64url = (text) => Buffer.from(text).toString('base64url');

// Interviews (Europe/Vienna). Globex lands on a free slot; Soylent overlaps
// the "Team offsite" below; the reschedule moves Globex two days later.
const globexDay = inDays(5);
const soylentDay = inDays(6);
const movedDay = inDays(7);
// 09:00–10:00 Vienna (CEST) is 07:00–08:00 UTC: the Outlook "Dentist".
const vandelayDay = inDays(2);

// ── Mailboxes ─────────────────────────────────────────────────────────
let historyId = 1000;
const gmail = [];
function addGmail(id, thread, from, subject, body, hoursAgo, labels = ['INBOX']) {
  historyId += 1;
  gmail.push({ id, thread, from, subject, body, at: Date.now() - hoursAgo * HOUR, labels, history: historyId });
}
const globexBody = `Hi Ana, we are happy to confirm your technical interview on ${longDate(globexDay)} from 14:00 to 15:00 (Europe/Vienna time). Join here: https://meet.example.com/globex-1 . Kind regards, Globex Talent Team`;
const soylentBody = `Hi Ana, your interview for the ML Engineer role is confirmed for ${longDate(soylentDay)} from 10:00 to 11:00 (Europe/Vienna time). Meeting link: https://meet.example.com/soylent-7 . Best, Soylent People Team`;
const starkBody = 'Thank you for your interest in the Site Reliability Engineer role. Unfortunately, we have decided to proceed with candidates who have more hands-on Kubernetes experience. We wish you all the best.';
addGmail('g9', 'gt9', 'Wayne Careers <careers@wayne.example>', 'Application received - Security Analyst', 'Thank you for applying for Security Analyst. Your application was received.', 45 * 24);
addGmail('g1', 'gt1', 'Acme Careers <jobs@acme.com>', 'Your application: Backend Engineer', 'Thank you for applying for Backend Engineer. We received your application.', 50);
addGmail('g2', 'gt2', 'Job Alerts <alerts@jobs.example>', '12 new jobs for you', 'Recommended jobs this week. PRIVATE-NEWSLETTER-CONTENT', 40);
addGmail('g3', 'gt3', 'Globex Recruiting <talent@globex.com>', 'Interview confirmation - Data Engineer', globexBody, 30);
addGmail('g4', 'gt4', 'Initech HR <hr@initech.com>', 'Your application at Initech', 'Unfortunately we decided to move forward with other candidates.', 20);
addGmail('g6', 'gt6', 'Umbrella Hiring <hiring@umbrella.example>', 'Coding assessment for your application - Frontend Engineer', 'Please complete the online coding assessment by 3 October. The link is valid for 7 days.', 15);
addGmail('g7', 'gt7', 'Hooli Recruiting <recruiting@hooli.example>', 'Application update - Platform Engineer', 'Good news: your application has moved to the hiring manager review. We will be in touch.', 12);
addGmail('g5', 'gt5', 'Mom <mom@family.net>', 'Dinner on Sunday', 'PRIVATE-FAMILY-CONTENT see you', 10);
addGmail('g8', 'gt8', 'Stark Talent <talent@stark.example>', 'Update on your application - Site Reliability Engineer', starkBody, 8);
addGmail('g10', 'gt10', 'Soylent People <people@soylent.example>', 'Interview confirmed - ML Engineer', soylentBody, 6);

let deltaToken = 1;
const outlook = [];
function addOutlook(id, conversationId, name, address, subject, body, hoursAgo) {
  outlook.push({ id, conversationId, from: { name, address }, subject, body, at: Date.now() - hoursAgo * HOUR, token: deltaToken });
}
addOutlook('o3', 'oc3', 'Initrode Careers', 'careers@initrode.example', 'Application received - Product Designer', 'We received your application for Product Designer.', 40 * 24);
// The Acme confirmation also reached Outlook (e.g. a forwarding alias).
addOutlook('o2', 'oc2', 'Acme Careers', 'jobs@acme.com', 'Your application: Backend Engineer', 'Thank you for applying for Backend Engineer. We received your application.', 50 - 2 / 60);
addOutlook('o1', 'oc1', 'Contoso Talent', 'talent@contoso.com', 'Offer letter - Cloud Engineer', 'We are delighted to offer you the Cloud Engineer position. Please reply by Friday.', 5);

const at = (day, time) => ({ dateTime: `${isoDate(day)}T${time}:00Z` });
const events = [
  // Busy all day when Soylent's interview is: a conflict.
  { id: 'busy1', summary: 'Team offsite', start: at(soylentDay, '07:00'), end: at(soylentDay, '16:00'), transparency: 'opaque', status: 'confirmed', htmlLink: 'https://calendar.google.com/event?eid=busy1' },
  { id: 'sync1', summary: 'Weekly sync', start: at(inDays(1), '09:00'), end: at(inDays(1), '09:30'), transparency: 'opaque', status: 'confirmed', hangoutLink: 'https://meet.google.com/abc-defg-hij', htmlLink: 'https://calendar.google.com/event?eid=sync1' },
];
let eventSeq = 1;
let replyDelay = 0;
// Event ids stay unique across mock sessions, as real calendars' ids do.
const session = Date.now().toString(36);
const outlookEvents = [
  { id: 'ol-dentist', subject: 'Dentist', start: { dateTime: `${isoDate(inDays(2))}T07:00:00`, timeZone: 'UTC' }, end: { dateTime: `${isoDate(inDays(2))}T08:00:00`, timeZone: 'UTC' }, showAs: 'busy', isAllDay: false, isCancelled: false, location: { displayName: 'Dental clinic' }, webLink: 'https://outlook.live.com/calendar/item/ol-dentist' },
];

// ── Model answers (OpenAI-compatible) ─────────────────────────────────
const plain = { reference: null, stage: null, action_required: false, next_action: null, rejection_reason: null, rejection_quote: null, existing_application_id: null, contacts: [], interview: null };
const interviewAt = (day, start, end, url) => ({
  state: 'confirmed', date: isoDate(day), start_time: start, end_time: end, duration_minutes: null, timezone: 'Europe/Vienna',
  datetime_quote: `${longDate(day)} from ${start} to ${end}`, timezone_quote: 'Europe/Vienna time',
  type: 'Technical interview', location: null, meeting_url: url, participants: [], interviewer: null, proposed_slots: [], unclear: null,
});
const answers = {
  'Application received - Security Analyst': { ...plain, category: 'application_confirmed', confidence: 0.95, company: 'Wayne Enterprises', role: 'Security Analyst', latest_update: 'Application received.', summary: 'Application received.' },
  'Application received - Product Designer': { ...plain, category: 'application_confirmed', confidence: 0.95, company: 'Initrode', role: 'Product Designer', latest_update: 'Application received.', summary: 'Application received.' },
  'Your application: Backend Engineer': { ...plain, category: 'application_confirmed', confidence: 0.95, company: 'Acme', role: 'Backend Engineer', latest_update: 'Application received.', summary: 'Application received.' },
  'Interview confirmation - Data Engineer': {
    ...plain, category: 'interview_confirmed', confidence: 0.97, company: 'Globex', role: 'Data Engineer', stage: 'Technical interview',
    latest_update: 'Technical interview confirmed.', summary: 'Technical interview confirmed.', contacts: ['Globex Talent Team'],
    interview: interviewAt(globexDay, '14:00', '15:00', 'https://meet.example.com/globex-1'),
  },
  'Re: Interview confirmation - Data Engineer': {
    ...plain, category: 'interview_rescheduled', confidence: 0.96, company: 'Globex', role: 'Data Engineer', stage: 'Technical interview',
    latest_update: 'Technical interview moved to a new time.', summary: 'Interview rescheduled.', contacts: ['Globex Talent Team'],
    interview: { ...interviewAt(movedDay, '11:00', '12:00', 'https://meet.example.com/globex-1'), state: 'rescheduled' },
  },
  'Interview confirmation - Import Export Analyst': {
    ...plain, category: 'interview_confirmed', confidence: 0.95, company: 'Vandelay Industries', role: 'Import Export Analyst', stage: 'First interview',
    latest_update: 'First interview confirmed.', summary: 'Interview confirmed.', interview: interviewAt(vandelayDay, '09:00', '10:00', 'https://meet.example.com/vandelay-3'),
  },
  'Interview confirmed - ML Engineer': {
    ...plain, category: 'interview_confirmed', confidence: 0.96, company: 'Soylent', role: 'ML Engineer', stage: 'Technical interview',
    latest_update: 'Interview confirmed.', summary: 'Interview confirmed.', interview: interviewAt(soylentDay, '10:00', '11:00', 'https://meet.example.com/soylent-7'),
  },
  'Your application at Initech': { ...plain, category: 'rejection', confidence: 0.97, company: 'Initech', role: 'QA Engineer', latest_update: 'Application rejected.', summary: 'Rejected.' },
  'Update on your application - Site Reliability Engineer': {
    ...plain, category: 'rejection', confidence: 0.97, company: 'Stark Industries', role: 'Site Reliability Engineer', latest_update: 'Application rejected.', summary: 'Rejected.',
    rejection_reason: 'They chose candidates with more hands-on Kubernetes experience.',
    rejection_quote: 'we have decided to proceed with candidates who have more hands-on Kubernetes experience',
  },
  'Coding assessment for your application - Frontend Engineer': {
    ...plain, category: 'assessment_request', confidence: 0.94, company: 'Umbrella', role: 'Frontend Engineer', action_required: true,
    next_action: 'Complete the online coding assessment by 3 October.', latest_update: 'Coding assessment sent.', summary: 'Assessment requested.',
  },
  'Application update - Platform Engineer': {
    ...plain, category: 'application_update', confidence: 0.9, company: 'Hooli', role: 'Platform Engineer',
    latest_update: 'Application moved to hiring manager review.', summary: 'In review.',
  },
  'Offer letter - Cloud Engineer': {
    ...plain, category: 'offer', confidence: 0.99, company: 'Contoso', role: 'Cloud Engineer', action_required: true,
    next_action: 'Reply to the offer by Friday.', latest_update: 'Offer received.', summary: 'Offer received.',
  },
};

function modelReply(body) {
  const user = [...(body.messages ?? [])].reverse().find((m) => m.role === 'user');
  const content = typeof user?.content === 'string' ? user.content : JSON.stringify(user?.content ?? '');
  // Personal mail and newsletters must never reach the model.
  const leaked = JSON.stringify(body).includes('PRIVATE-');
  const marker = content.indexOf('Emails:\n');
  if (marker >= 0) {
    const emails = JSON.parse(content.slice(marker + 8));
    const relevant = emails.filter((e) => !/new jobs|dinner/i.test(e.subject)).map((e) => e.id);
    log({ model: 'triage', subjects: emails.map((e) => e.subject), leaked });
    return JSON.stringify({ relevant });
  }
  const subject = content.split('\n').find((l) => l.startsWith('Subject: '))?.slice(9);
  if (subject !== undefined) {
    log({ model: 'classify', subject, leaked });
    return JSON.stringify(answers[subject] ?? { category: 'not_job_related', confidence: 0.9 });
  }
  log({ model: 'chat', leaked });
  return 'Mock answer from the local model.';
}

/** The next emails (POST /__e2e/next). */
function deliverNext() {
  const moved = `Hi Ana, we need to move your technical interview. The new time is ${longDate(movedDay)} from 11:00 to 12:00 (Europe/Vienna time), same link: https://meet.example.com/globex-1 . Globex Talent Team`;
  addGmail('g11', 'gt3', 'Globex Recruiting <talent@globex.com>', 'Re: Interview confirmation - Data Engineer', moved, 0.2);
  const vandelay = `Hi Ana, your first interview for the Import Export Analyst role is confirmed for ${longDate(vandelayDay)} from 09:00 to 10:00 (Europe/Vienna time). Link: https://meet.example.com/vandelay-3 . Vandelay Industries`;
  addGmail('g12', 'gt12', 'Vandelay Hiring <hiring@vandelay.example>', 'Interview confirmation - Import Export Analyst', vandelay, 0.1);
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
    const finish = () => {
      res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: { content: text } }] })}\n\n`);
      res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] })}\n\n`);
      res.end('data: [DONE]\n\n');
    };
    // POST /__e2e/delay {ms}: answers take that long (to catch a run mid-way).
    if (replyDelay > 0) setTimeout(finish, replyDelay);
    else finish();
    return;
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
    const before = Number((q.get('q') ?? '').match(/before:(\d+)/)?.[1] ?? Infinity) * 1000;
    const found = gmail.filter((m) => m.at >= after && m.at < before).sort((a, b) => b.at - a.at);
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
    const event = { ...JSON.parse(body), id: `evt-${session}-${eventSeq++}`, status: 'confirmed' };
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
  // `$filter=receivedDateTime ge <iso> [and receivedDateTime lt <iso>]`
  const received = (filter) => ({
    from: Date.parse(filter?.match(/receivedDateTime ge (\S+)/)?.[1] ?? '') || 0,
    to: Date.parse(filter?.match(/receivedDateTime lt (\S+)/)?.[1] ?? '') || Infinity,
  });
  if (p === '/graph/v1.0/me/mailFolders/inbox/messages/delta') {
    // The first round honours the lookback filter; later rounds return
    // what arrived since the delta token.
    const token = q.get('$deltatoken');
    const { from } = received(q.get('$filter'));
    const value = outlook
      .filter((m) => (token === null ? m.at >= from : m.token > Number(token)))
      .map((m) => graphMessage(m, false));
    deltaToken = Math.max(deltaToken, ...outlook.map((m) => m.token));
    return send(res, 200, { value, '@odata.deltaLink': `${base}/graph/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken=${deltaToken}` });
  }
  if (p === '/graph/v1.0/me/mailFolders/inbox/messages') {
    const { from, to } = received(q.get('$filter'));
    const value = outlook.filter((m) => m.at >= from && m.at < to).sort((a, b) => a.at - b.at);
    return send(res, 200, { value: value.map((m) => graphMessage(m, false)) });
  }
  match = p.match(/^\/graph\/v1\.0\/me\/messages\/([^/]+)$/);
  if (match) {
    const m = outlook.find((x) => x.id === match[1]);
    return m ? send(res, 200, graphMessage(m, true)) : send(res, 404, { error: { code: 'ErrorItemNotFound' } });
  }
  if (p === '/graph/v1.0/me/messages') return send(res, 200, { value: outlook.map((m) => graphMessage(m, false)) });
  const graphMs = (t) => Date.parse(`${t.dateTime}Z`);
  if (p === '/graph/v1.0/me/calendarView') {
    const min = Date.parse(q.get('startDateTime'));
    const max = Date.parse(q.get('endDateTime'));
    return send(res, 200, { value: outlookEvents.filter((e) => graphMs(e.start) < max && min < graphMs(e.end)) });
  }
  if (p === '/graph/v1.0/me/events') return send(res, 200, { value: [] });
  if (p === '/graph/v1.0/me/calendar/getSchedule') {
    const request = JSON.parse(body);
    const min = graphMs(request.startTime);
    const max = graphMs(request.endTime);
    const scheduleItems = outlookEvents
      .filter((e) => graphMs(e.start) < max && min < graphMs(e.end))
      .map((e) => ({ status: e.showAs, start: e.start, end: e.end }));
    return send(res, 200, { value: [{ scheduleId: 'ana@outlook.com', scheduleItems }] });
  }

  // Test control
  if (p === '/__e2e/delay' && req.method === 'POST') {
    replyDelay = Number(JSON.parse(body).ms) || 0;
    return send(res, 200, { ok: true, replyDelay });
  }
  if (p === '/__e2e/next' && req.method === 'POST') {
    deliverNext();
    return send(res, 200, { ok: true });
  }
  // One more Gmail message, with the classification the model gives it:
  // {id, from, subject, body, answer?}. Its history id is past any cursor
  // an earlier mock session handed out.
  if (p === '/__e2e/mail' && req.method === 'POST') {
    const m = JSON.parse(body);
    historyId += 100;
    addGmail(m.id, m.thread ?? m.id, m.from, m.subject, m.body, m.hoursAgo ?? 0);
    if (m.answer) answers[m.subject] = { ...plain, ...m.answer };
    return send(res, 200, { ok: true, historyId });
  }

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
