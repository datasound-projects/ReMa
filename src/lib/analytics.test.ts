import { describe, expect, it } from 'vitest';

import { isSearchPage, mayListJobs } from './analytics';

describe('mayListJobs', () => {
  it('offers Analyze for job tables and answers linking to postings', () => {
    expect(mayListJobs('| Company | Role |\n|---|---|\n| A | B |')).toBe(true);
    expect(
      mayListJobs('- [AI Engineer](https://boards.greenhouse.io/a/jobs/1)\n- [ML Engineer](https://jobs.lever.co/b/2)'),
    ).toBe(true);
  });

  it('does not offer it for answers that only suggest searches', () => {
    const searches = [
      '- [LinkedIn](https://www.linkedin.com/jobs/search/?keywords=AI%20Engineer&location=Vienna)',
      '- [Indeed](https://at.indeed.com/jobs?q=AI+Engineer&l=Wien)',
      '- [Google](https://www.google.com/search?q=ai+jobs)',
    ].join('\n');
    expect(mayListJobs(searches)).toBe(false);
  });

  it('recognises search result pages', () => {
    expect(isSearchPage('https://www.stepstone.at/jobs/suche?what=ai')).toBe(true);
    expect(isSearchPage('https://www.linkedin.com/jobs/view/4012345678/')).toBe(false);
    expect(isSearchPage('https://careers.example.com/job/senior-ai-engineer-123')).toBe(false);
  });
});
