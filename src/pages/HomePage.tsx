import { PageContainer } from '../components/layout/PageContainer';
import { StatusIndicator, type StatusTone } from '../components/ui/StatusIndicator';
import { useAppStatus } from '../hooks/useAppStatus';
import type { BackendStatus } from '../types/system';

const BACKEND_STATUS_LABELS: Record<BackendStatus, string> = {
  ready: 'Ready',
};

export function HomePage() {
  const { state, refresh } = useAppStatus();

  let tone: StatusTone;
  let label: string;
  switch (state.status) {
    case 'loading':
      tone = 'pending';
      label = 'Connecting…';
      break;
    case 'success':
      tone = 'ready';
      label = BACKEND_STATUS_LABELS[state.data.status];
      break;
    case 'error':
      tone = 'error';
      label = 'Unavailable';
      break;
  }

  const version = state.status === 'success' ? state.data.version : '—';

  return (
    <PageContainer title="ReMa" subtitle="Career UI Harness">
      <section className="panel" aria-labelledby="system-heading">
        <h2 id="system-heading" className="panel__label">
          System
        </h2>
        <dl className="info-list">
          <div className="info-list__row">
            <dt>Backend</dt>
            <dd>
              <StatusIndicator tone={tone} label={label} />
            </dd>
          </div>
          <div className="info-list__row">
            <dt>Version</dt>
            <dd className="info-list__mono">{version}</dd>
          </div>
        </dl>
        {state.status === 'error' && (
          <div className="panel__footer">
            <p className="panel__message selectable">{state.error.message}</p>
            <button type="button" className="button button--secondary" onClick={refresh}>
              Retry
            </button>
          </div>
        )}
      </section>
    </PageContainer>
  );
}
