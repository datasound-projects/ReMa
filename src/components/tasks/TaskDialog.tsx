import { useState } from 'react';

import { dataOr } from '../../hooks/useAsyncData';
import { useConnectors } from '../../hooks/useConnectors';
import { WEEKDAYS } from '../../lib/format';
import {
  defaultForm,
  formFromTask,
  formToInput,
  JOB_MAIL_SYNC_DESCRIPTION,
  jobMailSyncForm,
  MAX_LOOKBACK_DAYS,
  type TaskForm,
} from '../../lib/taskForm';
import { toApiError } from '../../services/ipc';
import { sameModel, type ModelCatalog, type ModelRef } from '../../services/providerService';
import { createTask, updateTask, type ScheduledTask } from '../../services/taskService';
import { MailCalendarIcon } from '../icons';
import { Dialog } from '../ui/Dialog';
import { StatusIndicator } from '../ui/StatusIndicator';

interface TaskDialogProps {
  catalog: ModelCatalog | null;
  timezone: string;
  /** Edit this task; otherwise a new task is created. */
  task?: ScheduledTask;
  /** Prefill for a new task (from the chat composer). */
  initialPrompt?: string;
  initialModel?: ModelRef | null;
  /** Prefill from the composer's Profile toggle. */
  initialUseProfile?: boolean;
  /** Set up the built-in Job Mail & Interview Sync (no `task` yet). */
  jobMailSync?: boolean;
  onClose: () => void;
  onSaved: (task: ScheduledTask) => void;
}

const modelKey = (model: ModelRef) => `${model.providerId}/${model.modelId}`;

/**
 * Create or edit a scheduled task: a prompt, or the built-in Job Mail &
 * Interview Sync (mail sources, initial lookback, calendar), with the same
 * model and schedule controls.
 */
export function TaskDialog({
  catalog,
  timezone,
  task,
  initialPrompt = '',
  initialModel = null,
  initialUseProfile = false,
  jobMailSync = false,
  onClose,
  onSaved,
}: TaskDialogProps) {
  const [form, setForm] = useState<TaskForm>(() =>
    task
      ? formFromTask(task)
      : jobMailSync
        ? jobMailSyncForm(initialModel)
        : defaultForm(initialPrompt, initialModel, initialUseProfile),
  );
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const update = (patch: Partial<TaskForm>) => setForm((f) => ({ ...f, ...patch }));
  const connectors = dataOr(useConnectors().state, null)?.connectors ?? [];
  const usable = (kind: 'mail' | 'calendar') =>
    connectors.filter((c) => c.kind === kind && (c.state === 'connected' || c.state === 'syncing'));
  const mailboxes = usable('mail');
  const calendars = usable('calendar');
  const jobs = form.type === 'job_applications';
  const title = jobs
    ? task
      ? 'Job Mail & Interview Sync settings'
      : 'Set up Job Mail & Interview Sync'
    : task
      ? 'Edit task'
      : 'Schedule task';

  const taskTimezone = task?.timezone ?? timezone;
  const models = catalog?.models ?? [];
  // Keep the task's model selectable even if it is no longer enabled.
  const modelMissing = form.model && !models.some((o) => sameModel(o.model, form.model));
  const providers = [...new Set(models.map((o) => o.providerName))];

  const save = async () => {
    const input = formToInput(form, taskTimezone);
    if (typeof input === 'string') {
      setError(input);
      return;
    }
    setSaving(true);
    setError(null);
    try {
      onSaved(task ? await updateTask(task.id, input) : await createTask(input));
    } catch (err) {
      setError(toApiError(err).message);
      setSaving(false);
    }
  };

  const toggleDay = (day: TaskForm['weekdays'][number]) =>
    update({
      weekdays: form.weekdays.includes(day)
        ? form.weekdays.filter((d) => d !== day)
        : [...form.weekdays, day],
    });

  return (
    <Dialog
      title={title}
      onClose={onClose}
      actions={
        <>
          {error && (
            <p className="form-error" role="alert">
              {error}
            </p>
          )}
          <button type="button" className="button button--secondary" onClick={onClose}>
            Cancel
          </button>
          <button
            type="button"
            className="button button--primary"
            disabled={saving}
            onClick={() => void save()}
          >
            {task ? 'Save' : jobs ? 'Turn on' : 'Schedule'}
          </button>
        </>
      }
    >
      <div className="form">
        {jobs && (
          <div className="builtin-intro">
            <MailCalendarIcon className="builtin-intro__icon" aria-hidden="true" />
            <p className="builtin-intro__text">
              {JOB_MAIL_SYNC_DESCRIPTION} ReMa reads your mail only while this task is on.
            </p>
          </div>
        )}

        {jobs && (
          <div className="job-options">
            <div className="field">
              <span className="field__label">Mail sources</span>
              {mailboxes.length === 0 ? (
                <p className="form__hint">
                  Connect Gmail or Outlook Mail in Settings → Connectors to use this task.
                </p>
              ) : (
                <ul className="source-list">
                  {mailboxes.map((c) => (
                    <li key={c.id}>
                      <StatusIndicator tone="ready" label={c.name} />
                      {c.accountEmail && <span className="source-list__account">{c.accountEmail}</span>}
                    </li>
                  ))}
                </ul>
              )}
            </div>
            <div className="form__inline">
              <label htmlFor="lookback-days">Initial lookback: last</label>
              <input
                id="lookback-days"
                type="number"
                min={1}
                max={MAX_LOOKBACK_DAYS}
                className="input input--number"
                value={form.lookbackDays}
                onChange={(e) => update({ lookbackDays: Number(e.target.value) })}
              />
              <span>days</span>
            </div>
            <p className="form__hint">
              The first run reads this many days of mail; later runs read only new mail. A longer lookback reads the
              added days once; a shorter one removes nothing.
            </p>
            <label className="checkbox">
              <input
                type="checkbox"
                checked={form.syncCalendar}
                onChange={(e) => update({ syncCalendar: e.target.checked })}
              />
              <span>Add confirmed interviews to my calendar when the time is free</span>
            </label>
            <p className="form__hint">
              {form.syncCalendar
                ? calendars.length > 0
                  ? `Checks ${calendars.map((c) => c.name).join(' and ')} for conflicts first; a conflict is flagged, never double-booked.`
                  : 'No calendar is connected: confirmed interviews are tracked but not added. Connect Google Calendar or Outlook Calendar in Settings → Connectors.'
                : 'Confirmed interviews are tracked in Applications only.'}
            </p>
          </div>
        )}

        <label className="field">
          <span className="field__label">{jobs ? 'Instructions (optional)' : 'Prompt'}</span>
          <textarea
            className="input input--textarea"
            rows={jobs ? 2 : 3}
            placeholder={
              jobs ? 'E.g. I applied for data engineering roles in Vienna. Ignore recruiter newsletters.' : ''
            }
            value={form.prompt}
            onChange={(e) => update({ prompt: e.target.value })}
          />
        </label>

        {!jobs && (
          <label className="checkbox">
            <input
              type="checkbox"
              checked={form.useProfile}
              onChange={(e) => update({ useProfile: e.target.checked })}
            />
            <span>Include my Profile (skills, experience, links; no email or phone)</span>
          </label>
        )}

        <div className="form__row">
          {!jobs && (
            <label className="field field--grow">
              <span className="field__label">Name</span>
              <input
                className="input"
                placeholder="From the prompt"
                value={form.name}
                onChange={(e) => update({ name: e.target.value })}
              />
            </label>
          )}
          <label className="field field--grow">
            <span className="field__label">Model</span>
            <select
              className="input"
              value={form.model ? modelKey(form.model) : ''}
              onChange={(e) => {
                const [providerId = '', ...rest] = e.target.value.split('/');
                update({ model: { providerId, modelId: rest.join('/') } });
              }}
            >
              {!form.model && <option value="">Choose a model</option>}
              {modelMissing && form.model && (
                <option value={modelKey(form.model)}>{form.model.modelId}</option>
              )}
              {providers.map((provider) => (
                <optgroup key={provider} label={provider}>
                  {models
                    .filter((o) => o.providerName === provider)
                    .map((o) => (
                      <option key={modelKey(o.model)} value={modelKey(o.model)}>
                        {o.displayName}
                      </option>
                    ))}
                </optgroup>
              ))}
            </select>
          </label>
        </div>

        <div className="form__row">
          <label className="field">
            <span className="field__label">{form.repeat === 'once' ? 'Run on' : 'Starts'}</span>
            <input
              type="date"
              className="input"
              value={form.startDate}
              onChange={(e) => update({ startDate: e.target.value })}
            />
          </label>
          <label className="field">
            <span className="field__label">At</span>
            <input
              type="time"
              className="input"
              value={form.startTime}
              onChange={(e) => update({ startTime: e.target.value })}
            />
          </label>
          <label className="field field--grow">
            <span className="field__label">Repeat</span>
            <select
              className="input"
              value={form.repeat}
              onChange={(e) => update({ repeat: e.target.value as TaskForm['repeat'] })}
            >
              <option value="once">Does not repeat</option>
              <option value="daily">Every day</option>
              <option value="weekdays">Every weekday</option>
              <option value="weekly">Weekly on…</option>
              <option value="days">Every few days</option>
              <option value="interval">Every few minutes or hours</option>
            </select>
          </label>
        </div>

        {form.repeat === 'weekly' && (
          <div className="weekday-picker" role="group" aria-label="Weekdays">
            {WEEKDAYS.map((day) => (
              <button
                key={day.id}
                type="button"
                aria-pressed={form.weekdays.includes(day.id)}
                aria-label={day.short}
                title={day.short}
                className="weekday-picker__day"
                onClick={() => toggleDay(day.id)}
              >
                {day.letter}
              </button>
            ))}
          </div>
        )}

        {form.repeat === 'days' && (
          <div className="form__inline">
            <span>Every</span>
            <input
              type="number"
              min={2}
              max={365}
              className="input input--number"
              value={form.everyDays}
              onChange={(e) => update({ everyDays: Number(e.target.value) })}
            />
            <span>days at {form.startTime}</span>
          </div>
        )}

        {form.repeat === 'interval' && (
          <div className="form__inline">
            <span>Every</span>
            <input
              type="number"
              min={1}
              className="input input--number"
              value={form.intervalEvery}
              onChange={(e) => update({ intervalEvery: Number(e.target.value) })}
            />
            <select
              className="input input--auto"
              value={form.intervalUnit}
              onChange={(e) => update({ intervalUnit: e.target.value as TaskForm['intervalUnit'] })}
            >
              <option value="minutes">minutes</option>
              <option value="hours">hours</option>
            </select>
            <span className="form__hint">Minimum 15 minutes</span>
          </div>
        )}

        {form.repeat !== 'once' && (
          <div className="form__inline">
            <span>Ends</span>
            <select
              className="input input--auto"
              value={form.endKind}
              onChange={(e) => update({ endKind: e.target.value as TaskForm['endKind'] })}
            >
              <option value="never">Never</option>
              <option value="on_date">On date</option>
              <option value="after_runs">After</option>
            </select>
            {form.endKind === 'on_date' && (
              <input
                type="date"
                className="input input--auto"
                value={form.endDate}
                onChange={(e) => update({ endDate: e.target.value })}
              />
            )}
            {form.endKind === 'after_runs' && (
              <>
                <input
                  type="number"
                  min={1}
                  className="input input--number"
                  value={form.endCount}
                  onChange={(e) => update({ endCount: Number(e.target.value) })}
                />
                <span>runs</span>
              </>
            )}
          </div>
        )}

        <p className="form__hint">Times are in {taskTimezone}.</p>
      </div>
    </Dialog>
  );
}
