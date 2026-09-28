import { describe, expect, it } from 'vitest';
import { testingNote } from './connectorNotes';

describe('testingNote', () => {
  const endsAt = Date.UTC(2026, 9, 5, 12, 0, 0);

  it('says the date is an estimate and whose setting it is', () => {
    const note = testingNote(endsAt);
    expect(note).toContain('(estimated)');
    expect(note).toContain('about 7 days');
    expect(note).toContain('in Testing');
    expect(note).toContain('publishing the app removes the limit');
    // Never a promise of an exact moment.
    expect(note).not.toMatch(/\bon (Mon|Tue|Wed|Thu|Fri|Sat|Sun)\b/);
  });

  it('names the estimated day', () => {
    const when = new Date(endsAt).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
    expect(testingNote(endsAt)).toContain(`around ${when}`);
  });
});
