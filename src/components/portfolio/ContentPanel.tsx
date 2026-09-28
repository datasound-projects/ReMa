import { SECTION_KINDS, addSection, moveSection, removeSection, updateSection } from '../../lib/portfolio/content';
import type { CoverLetter, PortfolioHeader, PortfolioInput } from '../../services/portfolioService';
import { PlusIcon } from '../icons';
import { Menu } from '../ui/Menu';
import { SectionEditor } from './SectionEditor';

interface ContentPanelProps {
  draft: PortfolioInput;
  edit: (update: (d: PortfolioInput) => PortfolioInput, group?: string) => void;
}

/** The form view of the same document the canvas edits. */
export function ContentPanel({ draft, edit }: ContentPanelProps) {
  const isLetter = (draft.kind ?? 'cv') === 'cover_letter';
  const h = draft.content.header;
  const setHeader = (patch: Partial<PortfolioHeader>, group: string) =>
    edit((d) => ({ ...d, content: { ...d.content, header: { ...d.content.header, ...patch } } }), group);
  const letter = draft.letter ?? {};
  const setLetter = (patch: Partial<CoverLetter>, group: string) => edit((d) => ({ ...d, letter: { ...(d.letter ?? {}), ...patch } }), group);

  return (
    <div className="content-panel">
      <section className="studio-section" aria-label="Header">
        <div className="studio-section__head">
          <span className="studio-section__static">{isLetter ? 'Sender' : 'Header'}</span>
          <span className="studio-section__kind">Name and contact</span>
        </div>
        <div className="studio-section__body">
          <div className="profile-grid">
            <Field label="Full name" value={h.fullName} onChange={(fullName) => setHeader({ fullName }, 'h-fullName')} />
            <Field label="Headline" value={h.headline} placeholder="Senior Data Engineer" onChange={(headline) => setHeader({ headline }, 'h-headline')} />
            <Field label="Email" type="email" value={h.email} onChange={(email) => setHeader({ email }, 'h-email')} />
            <Field label="Phone" type="tel" value={h.phone} onChange={(phone) => setHeader({ phone }, 'h-phone')} />
            <Field label="Location" value={h.location} placeholder="City, Country" onChange={(location) => setHeader({ location }, 'h-location')} />
            <Field label="Website" type="url" value={h.website} placeholder="https://" onChange={(website) => setHeader({ website }, 'h-website')} />
            <Field label="LinkedIn" type="url" value={h.linkedin} placeholder="https://linkedin.com/in/…" onChange={(linkedin) => setHeader({ linkedin }, 'h-linkedin')} />
            <Field label="GitHub" type="url" value={h.github} placeholder="https://github.com/…" onChange={(github) => setHeader({ github }, 'h-github')} />
          </div>
        </div>
      </section>

      {isLetter ? (
        <>
          <section className="studio-section" aria-label="Recipient">
            <div className="studio-section__head">
              <span className="studio-section__static">Recipient</span>
            </div>
            <div className="studio-section__body">
              <div className="profile-grid">
                <Field label="Name" value={letter.recipientName ?? ''} onChange={(recipientName) => setLetter({ recipientName }, 'l-recipientName')} />
                <Field label="Title" value={letter.recipientTitle ?? ''} placeholder="Head of Engineering" onChange={(recipientTitle) => setLetter({ recipientTitle }, 'l-recipientTitle')} />
                <Field label="Company" value={letter.company ?? ''} onChange={(company) => setLetter({ company }, 'l-company')} />
                <Field label="Position applied for" value={letter.position ?? ''} onChange={(position) => setLetter({ position }, 'l-position')} />
              </div>
              <label className="field">
                <span className="field__label">Address</span>
                <textarea className="input input--textarea" rows={2} maxLength={600} value={letter.address ?? ''} onChange={(e) => setLetter({ address: e.target.value }, 'l-address')} />
              </label>
            </div>
          </section>
          <section className="studio-section" aria-label="Letter">
            <div className="studio-section__head">
              <span className="studio-section__static">Letter</span>
            </div>
            <div className="studio-section__body">
              <div className="profile-grid">
                <Field label="Date" value={letter.date ?? ''} placeholder="12 March 2026" onChange={(date) => setLetter({ date }, 'l-date')} />
                <Field label="Subject" value={letter.subject ?? ''} placeholder="Application for …" onChange={(subject) => setLetter({ subject }, 'l-subject')} />
                <Field label="Greeting" value={letter.greeting ?? ''} placeholder="Dear …," onChange={(greeting) => setLetter({ greeting }, 'l-greeting')} />
              </div>
              <label className="field">
                <span className="field__label">Body</span>
                <textarea className="input input--textarea" rows={12} maxLength={12000} value={letter.body ?? ''} placeholder="Blank lines separate paragraphs. **bold**, _italic_ and [links](https://…) are kept." onChange={(e) => setLetter({ body: e.target.value }, 'l-body')} />
              </label>
              <div className="profile-grid">
                <Field label="Closing" value={letter.closing ?? ''} placeholder="Kind regards," onChange={(closing) => setLetter({ closing }, 'l-closing')} />
                <Field label="Signature" value={letter.signature ?? ''} onChange={(signature) => setLetter({ signature }, 'l-signature')} />
              </div>
            </div>
          </section>
        </>
      ) : (
        <>
          {draft.content.sections.map((section, index) => (
            <SectionEditor
              key={section.id}
              section={section}
              first={index === 0}
              last={index === draft.content.sections.length - 1}
              onChange={(next) => edit((d) => ({ ...d, content: updateSection(d.content, section.id, () => next) }), `section-${section.id}`)}
              onMove={(delta) => edit((d) => ({ ...d, content: moveSection(d.content, section.id, delta) }))}
              onRemove={() => edit((d) => ({ ...d, content: removeSection(d.content, section.id) }))}
            />
          ))}
          <Menu
            align="start"
            trigger={(props) => (
              <button type="button" className="button button--secondary studio-editor__add" disabled={draft.content.sections.length >= 30} {...props}>
                <PlusIcon className="button__icon" />
                Add section
              </button>
            )}
          >
            {(close) => (
              <div className="section-menu">
                {SECTION_KINDS.map((k) => (
                  <button
                    key={k.kind}
                    type="button"
                    role="menuitem"
                    className="menu__item section-menu__item"
                    onClick={() => {
                      close();
                      edit((d) => ({ ...d, content: addSection(d.content, k.kind) }));
                    }}
                  >
                    <span className="section-menu__label">{k.label}</span>
                    <span className="section-menu__description">{k.description}</span>
                  </button>
                ))}
              </div>
            )}
          </Menu>
        </>
      )}
    </div>
  );
}

function Field({
  label,
  value,
  onChange,
  placeholder,
  type = 'text',
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  type?: 'text' | 'email' | 'tel' | 'url';
}) {
  return (
    <label className="field">
      <span className="field__label">{label}</span>
      <input className="input" type={type} value={value} placeholder={placeholder} maxLength={type === 'url' ? 2000 : 200} onChange={(e) => onChange(e.target.value)} />
    </label>
  );
}
