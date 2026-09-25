import { useEffect, useRef, useState, type PointerEvent } from 'react';

import { COLLAPSED_WIDTH, MIN_ANALYTICS, useAnalytics } from '../../app/analytics';
import { useOptionalBrowser } from '../../app/browser';
import { useNavigation } from '../../app/navigation';
import { useAnalyticsData } from '../../hooks/useAnalyticsData';
import { RANKING_KEYS, directionLabel, jobsText } from '../../lib/analytics';
import {
  deleteJobSearchRun,
  getAnalyticsOverview,
  getLearning,
  getRequirementsAnalysis,
  getSkillGap,
  listJobSearchRuns,
  researchLearning,
  type AnalyticsPreferences,
  type DashboardTab,
  type RequirementTableQuery,
} from '../../services/analyticsService';
import { toApiError } from '../../services/ipc';
import {
  ChartIcon,
  CloseIcon,
  DatabaseIcon,
  FilterIcon,
  MaximizeIcon,
  MinimizeIcon,
  PanelIcon,
  RestoreIcon,
  SortIcon,
} from '../icons';
import { EmptyState } from '../ui/EmptyState';
import { IconButton } from '../ui/IconButton';
import { FilterEditor, RankingEditor, ScopeEditor } from './Controls';
import { JobsTab } from './JobsTab';
import { LearningTab } from './LearningTab';
import { RequirementsTab } from './RequirementsTab';
import { SkillGapTab } from './SkillGapTab';
import { Loading, Notes } from './common';

const TABS: { id: DashboardTab; label: string }[] = [
  { id: 'jobs', label: 'Jobs' },
  { id: 'skill_gap', label: 'Skill gap' },
  { id: 'requirements', label: 'Requirements' },
  { id: 'learning', label: 'Learning' },
];

const DEFAULT_TABLE: RequirementTableQuery = {
  search: '',
  categories: [],
  classes: [],
  states: [],
  sort: 'jobs',
  direction: 'desc',
};

type Editor = 'scope' | 'filters' | 'ranking';

function Dashboard({ prefs }: { prefs: AnalyticsPreferences }) {
  const analytics = useAnalytics();
  const { navigate } = useNavigation();
  const { update, selectedJobs, setSelectedJobs, setJobCount } = analytics;
  const query = prefs.query;
  const queryKey = JSON.stringify(query);
  const [editor, setEditor] = useState<Editor | null>(null);
  const [table, setTable] = useState<RequirementTableQuery>(DEFAULT_TABLE);
  const [pickedRequirements, setPickedRequirements] = useState<string[]>([]);
  const [researching, setResearching] = useState(false);
  const [researchError, setResearchError] = useState<string | null>(null);
  const tab = prefs.tab;

  const runs = useAnalyticsData(listJobSearchRuns, 'runs');
  const overview = useAnalyticsData(() => getAnalyticsOverview(query), queryKey);
  const gap = useAnalyticsData(
    tab === 'skill_gap' ? () => getSkillGap(query, prefs.gap) : null,
    `${queryKey}|${JSON.stringify(prefs.gap)}`,
  );
  const requirements = useAnalyticsData(
    tab === 'requirements' ? () => getRequirementsAnalysis(query, table) : null,
    `${queryKey}|${JSON.stringify(table)}`,
  );
  const learning = useAnalyticsData(
    tab === 'learning' ? () => getLearning(query, prefs.learning, prefs.gap) : null,
    `${queryKey}|${JSON.stringify(prefs.learning)}|${JSON.stringify(prefs.gap)}`,
  );

  const summary = overview.data?.summary;
  useEffect(() => setJobCount(summary?.analyzed ?? null), [summary?.analyzed, setJobCount]);

  const setQuery = (change: Partial<typeof query>) => update((p) => ({ ...p, query: { ...p.query, ...change } }));
  // Analyzing chosen jobs goes straight to how the Profile compares with them.
  const analyzeJobs = (ids: number[]) => {
    setSelectedJobs([]);
    update((p) => ({
      ...p,
      tab: 'skill_gap',
      query: { ...p.query, scope: { kind: 'jobs', runIds: [], jobIds: ids } },
    }));
  };
  const openProfile = () => navigate({ page: 'profile' });
  const runList = runs.data ?? [];
  const facets = overview.data?.facets ?? null;
  const status = overview.data?.status;

  const scopeText =
    query.scope.kind === 'all'
      ? 'All searches'
      : query.scope.kind === 'searches'
        ? `${query.scope.runIds.length} search${query.scope.runIds.length === 1 ? '' : 'es'}`
        : `${query.scope.jobIds.length} job${query.scope.jobIds.length === 1 ? '' : 's'}`;
  const firstRank = query.ranking?.[0];
  const rankText = firstRank
    ? `${RANKING_KEYS[firstRank.key]}${firstRank.value ? `: ${firstRank.value}` : ''}, ${directionLabel(firstRank.key, firstRank.direction === 'desc')}`
    : 'Newest found';

  const research = async () => {
    setResearching(true);
    setResearchError(null);
    try {
      await researchLearning(query, prefs.learning, prefs.gap);
      learning.refresh();
    } catch (err) {
      setResearchError(toApiError(err).message);
    } finally {
      setResearching(false);
    }
  };

  return (
    <>
      <div className="analytics__controls" role="toolbar" aria-label="Dataset">
        {(
          [
            ['scope', DatabaseIcon, 'Data scope', scopeText],
            ['filters', FilterIcon, 'Filters', query.filters?.length ? `${query.filters.length} active` : 'None'],
            ['ranking', SortIcon, 'Ranking', query.limit ? `${rankText} · top ${query.limit}` : rankText],
          ] as const
        ).map(([id, Icon, label, value]) => (
          <button
            key={id}
            type="button"
            className={`a-control${editor === id ? ' a-control--open' : ''}`}
            aria-expanded={editor === id}
            onClick={() => setEditor(editor === id ? null : id)}
          >
            <Icon className="a-control__icon" />
            <span className="a-control__label">{label}</span>
            <span className="a-control__value">{value}</span>
          </button>
        ))}
      </div>
      {editor && (
        <div className="analytics__editor">
          {editor === 'scope' && (
            <>
              <ScopeEditor
                scope={query.scope}
                runs={runList}
                selectedJobs={selectedJobs}
                onChange={(scope) => setQuery({ scope })}
                onDeleteRun={(run) => {
                  void deleteJobSearchRun(run.id)
                    .then(() => {
                      if (query.scope.runIds.includes(run.id)) {
                        setQuery({ scope: { ...query.scope, runIds: query.scope.runIds.filter((r) => r !== run.id) } });
                      }
                    })
                    .catch(() => {});
                }}
              />
              <div className="a-editor a-editor--options" role="group" aria-label="Job details">
                <span className="a-editor__title">Job details in the background</span>
                <label className="a-check">
                  <input
                    type="checkbox"
                    checked={prefs.readPages}
                    onChange={(e) => update((p) => ({ ...p, readPages: e.target.checked }))}
                  />
                  Read each job’s own page (public job pages only; never sites that refuse it)
                </label>
                <label className="a-check">
                  <input
                    type="checkbox"
                    checked={prefs.useModel}
                    onChange={(e) => update((p) => ({ ...p, useModel: e.target.checked }))}
                  />
                  Let {status?.model ?? 'the default model'} extract requirements from descriptions (validated against
                  the text)
                </label>
              </div>
            </>
          )}
          {editor === 'filters' && (
            <FilterEditor filters={query.filters ?? []} facets={facets} runs={runList} onChange={(filters) => setQuery({ filters })} />
          )}
          {editor === 'ranking' && (
            <RankingEditor
              ranking={query.ranking ?? []}
              limit={query.limit ?? null}
              facets={facets}
              onChange={(ranking) => setQuery({ ranking })}
              onLimit={(limit) => setQuery({ limit })}
            />
          )}
          <button type="button" className="link-button analytics__editor-close" onClick={() => setEditor(null)}>
            Done
          </button>
        </div>
      )}

      {summary && (
        <div className="analytics__dataset">
          <p className="analytics__label" title={summary.label}>
            {summary.label}
          </p>
          <p className="a-muted">
            <strong className="analytics__count">{summary.analyzed}</strong> job{summary.analyzed === 1 ? '' : 's'} analyzed
            {summary.matching !== summary.analyzed && ` of ${summary.matching} matching`}
            {summary.inScope !== summary.matching && ` (${summary.inScope} in scope)`}
            {summary.duplicatesMerged > 0 &&
              ` · ${summary.duplicatesMerged} duplicate result${summary.duplicatesMerged === 1 ? '' : 's'} merged`}
            {' · salary known for '}
            {summary.salary.known}
            {' · requirements known for '}
            {summary.requirementsKnown}
            {status && status.pendingDetails > 0 && ` · reading details of ${status.pendingDetails}…`}
          </p>
        </div>
      )}

      <div className="analytics__tabs" role="tablist" aria-label="Analysis">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={tab === t.id}
            className={`analytics__tab${tab === t.id ? ' analytics__tab--active' : ''}`}
            onClick={() => update((p) => ({ ...p, tab: t.id }))}
          >
            {t.label}
          </button>
        ))}
      </div>

      <div className="analytics__body" role="tabpanel">
        {overview.error && <p className="form-error">{overview.error}</p>}
        {status && status.runs === 0 ? (
          <EmptyState icon={<ChartIcon />} title="No job searches yet">
            Ask Chat for jobs (e.g. “Find remote AI engineer jobs in Austria”) or schedule a job-search task. Every
            result with a job table appears here automatically.
          </EmptyState>
        ) : (
          <>
            {tab === 'jobs' && overview.data && (
              <Loading busy={overview.loading}>
                {overview.data.summary.notes.length > 0 && (
                  <details className="a-coverage">
                    <summary>Data coverage ({overview.data.summary.notes.length})</summary>
                    <Notes notes={overview.data.summary.notes} />
                  </details>
                )}
                <JobsTab
                  overview={overview.data}
                  columns={prefs.columns}
                  onColumns={(columns) => update((p) => ({ ...p, columns }))}
                  selected={selectedJobs}
                  onSelected={setSelectedJobs}
                  onAnalyzeJobs={analyzeJobs}
                />
              </Loading>
            )}
            {tab === 'skill_gap' &&
              (gap.error ? (
                <p className="form-error">{gap.error}</p>
              ) : (
                gap.data && (
                  <Loading busy={gap.loading}>
                    <SkillGapTab
                      view={gap.data}
                      options={prefs.gap}
                      onOptions={(g) => update((p) => ({ ...p, gap: g }))}
                      onJob={(id) => analyzeJobs([id])}
                      onOpenProfile={openProfile}
                    />
                  </Loading>
                )
              ))}
            {tab === 'requirements' &&
              (requirements.error ? (
                <p className="form-error">{requirements.error}</p>
              ) : (
                requirements.data && (
                  <Loading busy={requirements.loading}>
                    <RequirementsTab
                      view={requirements.data}
                      table={table}
                      onTable={setTable}
                      selected={pickedRequirements}
                      onSelected={setPickedRequirements}
                      onFilterJobs={(names) => {
                        setPickedRequirements([]);
                        setQuery({
                          filters: [...(query.filters ?? []), { field: 'skill', op: 'has_any', values: names, number: null, currency: null }],
                        });
                        update((p) => ({ ...p, tab: 'jobs' }));
                      }}
                      onFocusLearning={(names) => {
                        setPickedRequirements([]);
                        update((p) => ({ ...p, tab: 'learning', learning: { ...p.learning, focus: names } }));
                      }}
                    />
                  </Loading>
                )
              ))}
            {tab === 'learning' &&
              (learning.error ? (
                <p className="form-error">{learning.error}</p>
              ) : (
                learning.data && (
                  <Loading busy={learning.loading}>
                    <LearningTab
                      view={learning.data}
                      criteria={prefs.learning}
                      onCriteria={(c) => update((p) => ({ ...p, learning: c }))}
                      onResearch={() => void research()}
                      researching={researching}
                      error={researchError}
                      onOpenProfile={openProfile}
                    />
                  </Loading>
                )
              ))}
          </>
        )}
      </div>
    </>
  );
}

/**
 * The Analytics workspace panel, on the right of the current page. Layout
 * (width, maximized, collapsed) is decided by the app shell.
 */
export function AnalyticsPanel({ width, fill, collapsed }: { width: number; fill: boolean; collapsed: boolean }) {
  const analytics = useAnalytics();
  const browser = useOptionalBrowser();
  const panelRef = useRef<HTMLElement>(null);
  const { mode, prefs } = analytics;

  // Dragging the left edge resizes; a web page next to it is hidden meanwhile.
  const startResize = (event: PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    const handle = event.currentTarget;
    handle.setPointerCapture(event.pointerId);
    const release = browser?.cover() ?? (() => {});
    const panel = panelRef.current?.getBoundingClientRect();
    if (!panel) return release();
    const onMove = (e: globalThis.PointerEvent) => {
      analytics.setWidth(Math.round(Math.max(MIN_ANALYTICS, panel.right - e.clientX)));
    };
    const onUp = () => {
      handle.removeEventListener('pointermove', onMove);
      handle.removeEventListener('pointerup', onUp);
      handle.removeEventListener('pointercancel', onUp);
      release();
    };
    handle.addEventListener('pointermove', onMove);
    handle.addEventListener('pointerup', onUp);
    handle.addEventListener('pointercancel', onUp);
  };

  if (mode === 'collapsed' || collapsed) {
    const expand = () => {
      if (browser?.mode === 'maximized') browser.setMode('docked');
      analytics.setMode('docked');
    };
    return (
      <aside className="analytics analytics--collapsed" style={{ width: COLLAPSED_WIDTH }} aria-label="Analytics (collapsed)">
        <IconButton label="Expand Analytics" className="icon-button--small" onClick={expand}>
          <ChartIcon />
        </IconButton>
        <button type="button" className="analytics__collapsed-title" tabIndex={-1} onClick={expand}>
          <span>Analytics{analytics.jobCount != null && ` · ${jobsText(analytics.jobCount)}`}</span>
        </button>
        <IconButton label="Close Analytics" className="icon-button--small" onClick={analytics.close}>
          <CloseIcon />
        </IconButton>
      </aside>
    );
  }

  const maximized = mode === 'maximized';
  return (
    <aside
      ref={panelRef}
      className={`analytics${maximized || fill ? ' analytics--fill' : ''}`}
      style={maximized || fill ? undefined : { width }}
      aria-label="Analytics"
    >
      {!maximized && !fill && (
        <div className="analytics__resize" role="separator" aria-orientation="vertical" aria-label="Resize Analytics" onPointerDown={startResize} />
      )}
      <div className="analytics__header">
        <ChartIcon className="analytics__header-icon" />
        <span className="analytics__title">Analytics</span>
        <IconButton label="Collapse" className="icon-button--small" onClick={() => analytics.setMode('collapsed')}>
          <PanelIcon side="right" />
        </IconButton>
        <IconButton label="Minimize" className="icon-button--small" onClick={() => analytics.setMode('minimized')}>
          <MinimizeIcon />
        </IconButton>
        <IconButton
          label={maximized ? 'Restore' : 'Maximize'}
          className="icon-button--small"
          onClick={() => {
            if (!maximized && browser?.mode === 'maximized') browser.setMode('docked');
            analytics.setMode(maximized ? 'docked' : 'maximized');
          }}
        >
          {maximized ? <RestoreIcon /> : <MaximizeIcon />}
        </IconButton>
        <IconButton label="Close Analytics" className="icon-button--small" onClick={analytics.close}>
          <CloseIcon />
        </IconButton>
      </div>
      {prefs ? <Dashboard prefs={prefs} /> : <p className="a-empty">Loading…</p>}
    </aside>
  );
}
