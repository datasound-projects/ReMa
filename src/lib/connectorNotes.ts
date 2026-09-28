/**
 * While ReMa's Google app is in Testing, Google ends every sign-in 7 days
 * after it was made: a setting of the app, said before it happens.
 */
export function testingNote(endsAt: number): string {
  const when = new Date(endsAt).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
  return `Google ends this sign-in on ${when}: ReMa's Google app is in Testing, where Google limits sign-ins to 7 days. Reconnect then; publishing the app removes the limit.`;
}
