import { useAction } from '../../hooks/useAction';
import { KIND_LABELS } from '../../lib/documents';
import { move, removeAt, replaceAt } from '../../lib/profileMerge';
import { SECTION_IDS } from '../../lib/profileSections';
import {
  addProfileDocument,
  type CustomField,
  type CustomFieldKind,
  type ProfileDocument,
} from '../../services/profileService';
import { ChevronDownIcon, ChevronUpIcon, PlusIcon, TrashIcon } from '../icons';
import { IconButton } from '../ui/IconButton';
import { Section } from './ProfileSections';

const FIELD_KINDS: Record<CustomFieldKind, string> = { text: 'Text', url: 'URL', file: 'File' };

export function CustomFieldsSection({
  fields,
  documents,
  onChange,
}: {
  fields: CustomField[];
  documents: ProfileDocument[];
  onChange: (fields: CustomField[]) => void;
}) {
  const action = useAction();
  const set = (index: number, patch: Partial<CustomField>) =>
    onChange(replaceAt(fields, index, { ...(fields[index] as CustomField), ...patch }));

  const attachNew = (index: number) =>
    void action.run(async () => {
      const doc = await addProfileDocument(null);
      if (doc) set(index, { documentId: doc.id });
    });

  return (
    <Section
      id={SECTION_IDS.custom}
      title="Custom fields"
      hint="Anything else an application may ask for: portfolio, research profile, certificates…"
    >
      {fields.length === 0 && <p className="entries__empty">No custom fields yet.</p>}
      {fields.map((field, index) => (
        <div key={index} className="form__row profile-row custom-field">
          <label className="field custom-field__label">
            <span className={index > 0 ? 'sr-only' : 'field__label'}>Name</span>
            <input
              className="input"
              placeholder="Portfolio"
              value={field.label}
              onChange={(e) => set(index, { label: e.target.value })}
            />
          </label>
          <label className="field custom-field__kind">
            <span className={index > 0 ? 'sr-only' : 'field__label'}>Type</span>
            <select
              className="input"
              value={field.kind}
              onChange={(e) => set(index, { kind: e.target.value as CustomFieldKind, value: '', documentId: null })}
            >
              {Object.entries(FIELD_KINDS).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label className="field field--grow">
            <span className={index > 0 ? 'sr-only' : 'field__label'}>Value</span>
            {field.kind === 'file' ? (
              <select
                className="input"
                value={field.documentId ?? ''}
                onChange={(e) => {
                  if (e.target.value === 'new') attachNew(index);
                  else set(index, { documentId: e.target.value ? Number(e.target.value) : null });
                }}
              >
                <option value="">Choose a document</option>
                {documents.map((d) => (
                  <option key={d.id} value={d.id}>
                    {d.name} ({KIND_LABELS[d.kind]})
                  </option>
                ))}
                <option value="new">Add a file…</option>
              </select>
            ) : (
              <input
                className="input"
                type={field.kind === 'url' ? 'url' : 'text'}
                placeholder={field.kind === 'url' ? 'https://' : ''}
                value={field.value}
                onChange={(e) => set(index, { value: e.target.value })}
              />
            )}
          </label>
          <div className="profile-row__actions">
            <IconButton
              label="Move up"
              className="icon-button--small"
              disabled={index === 0}
              onClick={() => onChange(move(fields, index, -1))}
            >
              <ChevronUpIcon />
            </IconButton>
            <IconButton
              label="Move down"
              className="icon-button--small"
              disabled={index === fields.length - 1}
              onClick={() => onChange(move(fields, index, 1))}
            >
              <ChevronDownIcon />
            </IconButton>
            <IconButton label="Remove field" className="icon-button--small" onClick={() => onChange(removeAt(fields, index))}>
              <TrashIcon />
            </IconButton>
          </div>
        </div>
      ))}
      <button
        type="button"
        className="button button--ghost entries__add"
        onClick={() => onChange([...fields, { label: '', kind: 'text', value: '', documentId: null }])}
      >
        <PlusIcon className="button__icon" />
        Add field
      </button>
      {action.error && <p className="form-error">{action.error}</p>}
    </Section>
  );
}
