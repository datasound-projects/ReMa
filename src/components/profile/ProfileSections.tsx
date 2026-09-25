import type { ReactNode } from 'react';

import { removeAt, replaceAt } from '../../lib/profileMerge';
import type { Education, Experience, Profile } from '../../services/profileService';
import { PlusIcon, TrashIcon } from '../icons';
import { IconButton } from '../ui/IconButton';
import { ChipInput, EntryList } from './EntryList';

type Update = (patch: Partial<Profile>) => void;

export function Section({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <section className="profile-section">
      <div className="profile-section__head">
        <h2 className="section-title">{title}</h2>
        {hint && <span className="profile-section__hint">{hint}</span>}
      </div>
      <div className="panel profile-section__body">{children}</div>
    </section>
  );
}

function Field({
  label,
  value,
  onChange,
  type = 'text',
  placeholder,
  grow,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: 'text' | 'email' | 'tel' | 'url';
  placeholder?: string;
  grow?: boolean;
}) {
  return (
    <label className={grow ? 'field field--grow' : 'field'}>
      <span className="field__label">{label}</span>
      <input
        className="input"
        type={type}
        value={value}
        placeholder={placeholder}
        spellCheck={type === 'text'}
        onChange={(e) => onChange(e.target.value)}
      />
    </label>
  );
}

export function PersonalSection({ profile, update }: { profile: Profile; update: Update }) {
  return (
    <Section title="Personal information">
      <div className="profile-grid">
        <Field label="First name" value={profile.firstName} onChange={(firstName) => update({ firstName })} />
        <Field label="Last name" value={profile.lastName} onChange={(lastName) => update({ lastName })} />
        <Field label="Email" type="email" value={profile.email} onChange={(email) => update({ email })} />
        <Field label="Phone" type="tel" value={profile.phone} onChange={(phone) => update({ phone })} />
        <Field
          label="Location"
          placeholder="City, Country"
          value={profile.location}
          onChange={(location) => update({ location })}
        />
      </div>
    </Section>
  );
}

export function ProfessionalSection({ profile, update }: { profile: Profile; update: Update }) {
  return (
    <Section title="Professional profile">
      <Field
        label="Professional title"
        placeholder="e.g. Data Engineer"
        value={profile.title}
        onChange={(title) => update({ title })}
      />
      <label className="field">
        <span className="field__label">Summary</span>
        <textarea
          className="input input--textarea"
          rows={4}
          value={profile.summary}
          onChange={(e) => update({ summary: e.target.value })}
        />
      </label>
      <div className="field">
        <span className="field__label">Skills</span>
        <ChipInput
          label="Add a skill"
          placeholder="Type a skill and press Enter"
          values={profile.skills}
          onChange={(skills) => update({ skills })}
        />
      </div>
    </Section>
  );
}

const period = (start: string, end: string, current = false) =>
  [start, current ? 'present' : end].filter(Boolean).join(' – ');

export function ExperienceSection({ profile, update }: { profile: Profile; update: Update }) {
  return (
    <Section title="Experience">
      <EntryList<Experience>
        items={profile.experience}
        onChange={(experience) => update({ experience })}
        addLabel="Add experience"
        empty="No positions yet."
        create={() => ({ title: '', company: '', location: '', start: '', end: '', current: false, description: '' })}
        summary={(e) => (
          <>
            <span className="entry__title">{[e.title, e.company].filter(Boolean).join(' · ') || 'New position'}</span>
            <span className="entry__meta">{period(e.start, e.end, e.current)}</span>
          </>
        )}
        editor={(e, set) => (
          <>
            <div className="profile-grid">
              <Field label="Job title" value={e.title} onChange={(title) => set({ ...e, title })} />
              <Field label="Company" value={e.company} onChange={(company) => set({ ...e, company })} />
              <Field label="Location" value={e.location} onChange={(location) => set({ ...e, location })} />
              <div className="form__row">
                <Field label="Start" placeholder="2021-03" value={e.start} onChange={(start) => set({ ...e, start })} />
                {!e.current && (
                  <Field label="End" placeholder="2023-08" value={e.end} onChange={(end) => set({ ...e, end })} />
                )}
              </div>
            </div>
            <label className="checkbox">
              <input
                type="checkbox"
                checked={e.current}
                onChange={(ev) => set({ ...e, current: ev.target.checked })}
              />
              <span>I currently work here</span>
            </label>
            <label className="field">
              <span className="field__label">Description</span>
              <textarea
                className="input input--textarea"
                rows={3}
                value={e.description}
                onChange={(ev) => set({ ...e, description: ev.target.value })}
              />
            </label>
          </>
        )}
      />
    </Section>
  );
}

export function EducationSection({ profile, update }: { profile: Profile; update: Update }) {
  return (
    <Section title="Education">
      <EntryList<Education>
        items={profile.education}
        onChange={(education) => update({ education })}
        addLabel="Add education"
        empty="No education yet."
        create={() => ({ school: '', degree: '', field: '', start: '', end: '', description: '' })}
        summary={(e) => (
          <>
            <span className="entry__title">
              {[e.degree, e.field, e.school].filter(Boolean).join(' · ') || 'New education'}
            </span>
            <span className="entry__meta">{period(e.start, e.end)}</span>
          </>
        )}
        editor={(e, set) => (
          <>
            <div className="profile-grid">
              <Field label="School" value={e.school} onChange={(school) => set({ ...e, school })} />
              <Field label="Degree" value={e.degree} onChange={(degree) => set({ ...e, degree })} />
              <Field label="Field of study" value={e.field} onChange={(field) => set({ ...e, field })} />
              <div className="form__row">
                <Field label="Start" value={e.start} onChange={(start) => set({ ...e, start })} />
                <Field label="End" value={e.end} onChange={(end) => set({ ...e, end })} />
              </div>
            </div>
            <label className="field">
              <span className="field__label">Description</span>
              <textarea
                className="input input--textarea"
                rows={2}
                value={e.description}
                onChange={(ev) => set({ ...e, description: ev.target.value })}
              />
            </label>
          </>
        )}
      />
    </Section>
  );
}

export function LanguagesSection({ profile, update }: { profile: Profile; update: Update }) {
  const languages = profile.languages;
  return (
    <Section title="Languages">
      {languages.map((language, index) => (
        <div key={index} className="form__row profile-row">
          <Field
            grow
            label="Language"
            value={language.name}
            onChange={(name) => update({ languages: replaceAt(languages, index, { ...language, name }) })}
          />
          <Field
            grow
            label="Level"
            placeholder="Native, C1, Fluent…"
            value={language.level}
            onChange={(level) => update({ languages: replaceAt(languages, index, { ...language, level }) })}
          />
          <IconButton
            label="Remove language"
            className="icon-button--small profile-row__remove"
            onClick={() => update({ languages: removeAt(languages, index) })}
          >
            <TrashIcon />
          </IconButton>
        </div>
      ))}
      <button
        type="button"
        className="button button--ghost entries__add"
        onClick={() => update({ languages: [...languages, { name: '', level: '' }] })}
      >
        <PlusIcon className="button__icon" />
        Add language
      </button>
    </Section>
  );
}

export function LinksSection({ profile, update }: { profile: Profile; update: Update }) {
  const links = profile.otherLinks;
  return (
    <Section title="Links" hint="A resume website can replace a CV file.">
      <div className="profile-grid">
        <Field
          type="url"
          label="Personal website"
          placeholder="https://"
          value={profile.website}
          onChange={(website) => update({ website })}
        />
        <Field
          type="url"
          label="Resume website"
          placeholder="https://"
          value={profile.resumeWebsite}
          onChange={(resumeWebsite) => update({ resumeWebsite })}
        />
        <Field
          type="url"
          label="GitHub"
          placeholder="https://github.com/…"
          value={profile.github}
          onChange={(github) => update({ github })}
        />
        <Field
          type="url"
          label="LinkedIn"
          placeholder="https://linkedin.com/in/…"
          value={profile.linkedin}
          onChange={(linkedin) => update({ linkedin })}
        />
      </div>
      {links.map((link, index) => (
        <div key={index} className="form__row profile-row">
          <Field
            label="Name"
            placeholder="Blog"
            value={link.label}
            onChange={(label) => update({ otherLinks: replaceAt(links, index, { ...link, label }) })}
          />
          <Field
            grow
            type="url"
            label="Address"
            placeholder="https://"
            value={link.url}
            onChange={(url) => update({ otherLinks: replaceAt(links, index, { ...link, url }) })}
          />
          <IconButton
            label="Remove link"
            className="icon-button--small profile-row__remove"
            onClick={() => update({ otherLinks: removeAt(links, index) })}
          >
            <TrashIcon />
          </IconButton>
        </div>
      ))}
      <button
        type="button"
        className="button button--ghost entries__add"
        onClick={() => update({ otherLinks: [...links, { label: '', url: '' }] })}
      >
        <PlusIcon className="button__icon" />
        Add link
      </button>
    </Section>
  );
}
