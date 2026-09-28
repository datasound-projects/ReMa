/**
 * While ReMa's Google app is in Testing, Google ends every sign-in about 7
 * days after it was made: a setting of the app, said before it happens.
 * Google gives no exact moment, so the date is an estimate.
 */
export function testingNote(endsAt: number): string {
  const when = new Date(endsAt).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
  return `Google is expected to end this sign-in around ${when} (estimated): ReMa's Google app is in Testing, where Google ends sign-ins about 7 days after they are made. Reconnect then; publishing the app removes the limit.`;
}
