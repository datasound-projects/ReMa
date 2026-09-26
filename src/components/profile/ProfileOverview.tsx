import { SECTION_IDS } from '../../lib/profileSections';
import type { Profile } from '../../services/profileService';
import { ProfileIcon } from '../icons';

interface SectionSummary {
  id: string;
  label: string;
  filled: boolean;
  /** "2 roles", "Not added", … */
  detail: string;
}

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
const filledText = (values: string[]) => values.filter((v) => v.trim()).length;

function summarize(profile: Profile): SectionSummary[] {
  const personal = filledText([profile.firstName, profile.lastName, profile.email, profile.phone, profile.location]);
  const professional = filledText([profile.title, profile.summary]);
  const links =
    filledText([profile.website, profile.resumeWebsite, profile.github, profile.linkedin]) +
    profile.otherLinks.filter((l) => l.url.trim()).length;
  const entry = (id: string, label: string, n: number, one: string, many: string): SectionSummary => ({
    id,
    label,
    filled: n > 0,
    detail: n > 0 ? count(n, one, many) : 'Not added',
  });
  return [
    { id: SECTION_IDS.personal, label: 'Personal', filled: personal > 0, detail: personal > 0 ? `${personal} of 5` : 'Not added' },
    {
      id: SECTION_IDS.professional,
      label: 'Professional',
      filled: professional > 0,
      detail: professional > 0 ? `${professional} of 2` : 'Not added',
    },
    entry(SECTION_IDS.experience, 'Experience', profile.experience.length, 'role', 'roles'),
    entry(SECTION_IDS.education, 'Education', profile.education.length, 'entry', 'entries'),
    entry(SECTION_IDS.skills, 'Skills', profile.skills.length, 'skill', 'skills'),
    entry(SECTION_IDS.languages, 'Languages', profile.languages.filter((l) => l.name.trim()).length, 'language', 'languages'),
    entry(SECTION_IDS.links, 'Links', links, 'link', 'links'),
    entry(SECTION_IDS.custom, 'Custom fields', profile.customFields.length, 'field', 'fields'),
  ];
}

/**
 * The top of the Profile: who it describes, how complete it is, and every
 * section at a glance (filled or not). A section chip jumps to it.
 */
export function ProfileOverview({ profile }: { profile: Profile }) {
  const sections = summarize(profile);
  const filled = sections.filter((s) => s.filled).length;
  const name = [profile.firstName, profile.lastName].filter((s) => s.trim()).join(' ');
  const initials = [profile.firstName, profile.lastName]
    .map((s) => s.trim().charAt(0).toUpperCase())
    .join('');
  const role = [profile.title, profile.location].filter((s) => s.trim()).join(' · ');
  const jump = (id: string) => document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' });

  return (
    <section className="profile-overview" aria-label="Profile overview">
      <div className="profile-overview__identity">
        <span className="avatar" aria-hidden="true">
          {initials || <ProfileIcon />}
        </span>
        <div className="profile-overview__who">
          <p className="profile-overview__name">{name || 'Your name'}</p>
          <p className="profile-overview__role">{role || 'Add your title and location'}</p>
        </div>
        <div className="profile-overview__progress">
          <span className="profile-overview__count">
            <strong>{filled}</strong> of {sections.length} sections filled · all optional
          </span>
          <span
            className="progress"
            role="progressbar"
            aria-label="Profile completeness"
            aria-valuemin={0}
            aria-valuemax={sections.length}
            aria-valuenow={filled}
          >
            <span className="progress__bar" style={{ width: `${(filled / sections.length) * 100}%` }} />
          </span>
        </div>
      </div>
      <nav className="profile-overview__nav" aria-label="Profile sections">
        {sections.map((section) => (
          <button
            key={section.id}
            type="button"
            className={section.filled ? 'section-chip section-chip--filled' : 'section-chip'}
            onClick={() => jump(section.id)}
          >
            <span className="section-chip__dot" aria-hidden="true" />
            {section.label}
            <span className="section-chip__detail">{section.detail}</span>
          </button>
        ))}
      </nav>
    </section>
  );
}
