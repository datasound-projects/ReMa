import {
  BarController,
  BarElement,
  CategoryScale,
  Chart,
  LinearScale,
  Tooltip,
  type Plugin,
} from 'chart.js';
import { useEffect, useRef, useState, type ReactNode } from 'react';

// Only the pieces the dashboard uses (keeps the bundle small).
Chart.register(BarController, BarElement, CategoryScale, LinearScale, Tooltip);
// Axis labels and tooltips use the app's own typeface.
Chart.defaults.font.family = token('--font-sans', 'system-ui');

export interface BarRow {
  label: string;
  value: number;
  /** Text at the bar's tip, e.g. "83% · 29 jobs". */
  display: string;
  /** Tooltip lines. */
  detail?: string[];
}

export interface Threshold {
  value: number;
  label: string;
}

interface BarChartProps {
  rows: BarRow[];
  /** Axis maximum (e.g. 100 for percentages). */
  max?: number;
  ariaLabel: string;
  thresholds?: Threshold[];
  tick?: (value: number) => string;
  onSelect?: (index: number) => void;
}

function token(name: string, fallback: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value || fallback;
}

/** Puts new rows into an existing chart. */
function showRows(chart: Chart<'bar'>, rows: BarRow[], max: number | undefined) {
  chart.data.labels = rows.map((r) => r.label);
  const dataset = chart.data.datasets[0];
  if (dataset) dataset.data = rows.map((r) => r.value);
  const x = chart.options.scales?.x;
  if (x) x.max = max;
  chart.update();
}

const ROW_HEIGHT = 26;
const AXIS_BAND = 26;

/**
 * Horizontal bars: one hue, thin (≤ 18 px) with a rounded data end and a
 * square baseline, hairline grid, value at the tip, tooltip per bar.
 */
export function BarChart({ rows, max, ariaLabel, thresholds = [], tick, onSelect }: BarChartProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const chartRef = useRef<Chart<'bar'> | null>(null);
  const rowsRef = useRef(rows);
  const selectRef = useRef(onSelect);
  const thresholdsRef = useRef(thresholds);
  const tickRef = useRef(tick);

  useEffect(() => {
    rowsRef.current = rows;
    selectRef.current = onSelect;
    thresholdsRef.current = thresholds;
    tickRef.current = tick;
  });

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ink = token('--color-text', '#191919');
    const secondary = token('--color-text-secondary', '#5c5c59');
    const muted = token('--color-text-tertiary', '#8b8b87');
    const grid = token('--color-border', '#e5e4e0');
    const bar = token('--color-brand', '#0a66c2');
    const barHover = token('--color-brand-hover', '#004182');

    const valueLabels: Plugin<'bar'> = {
      id: 'valueLabels',
      afterDatasetsDraw(chart) {
        const { ctx } = chart;
        const meta = chart.getDatasetMeta(0);
        ctx.save();
        ctx.font = '11px ' + token('--font-sans', 'system-ui');
        ctx.fillStyle = secondary;
        ctx.textBaseline = 'middle';
        meta.data.forEach((element, i) => {
          const text = rowsRef.current[i]?.display ?? '';
          ctx.fillText(text, element.x + 6, element.y);
        });
        ctx.restore();
      },
    };
    const thresholdLines: Plugin<'bar'> = {
      id: 'thresholdLines',
      beforeDatasetsDraw(chart) {
        const { ctx, chartArea } = chart;
        const xScale = chart.scales.x;
        if (!xScale) return;
        ctx.save();
        ctx.strokeStyle = token('--color-border-strong', '#d4d3ce');
        ctx.fillStyle = muted;
        ctx.lineWidth = 1;
        ctx.font = '10px ' + token('--font-sans', 'system-ui');
        ctx.textBaseline = 'bottom';
        for (const t of thresholdsRef.current) {
          const x = Math.round(xScale.getPixelForValue(t.value)) + 0.5;
          ctx.beginPath();
          ctx.moveTo(x, chartArea.top);
          ctx.lineTo(x, chartArea.bottom);
          ctx.stroke();
          // Labels sit in the padding above the plot, clear of the bars.
          ctx.beginPath();
          ctx.moveTo(x, chartArea.top - 12);
          ctx.lineTo(x, chartArea.top);
          ctx.stroke();
          ctx.fillText(t.label, x + 3, chartArea.top - 2);
        }
        ctx.restore();
      },
    };

    const chart = new Chart(canvas, {
      type: 'bar',
      data: { labels: [], datasets: [{ data: [] }] },
      options: {
        indexAxis: 'y',
        responsive: true,
        maintainAspectRatio: false,
        animation: false,
        layout: { padding: { right: 96, top: thresholdsRef.current.length ? 14 : 0 } },
        datasets: {
          bar: {
            backgroundColor: bar,
            hoverBackgroundColor: barHover,
            borderRadius: 4,
            borderSkipped: 'start',
            maxBarThickness: 18,
            categoryPercentage: 0.82,
            barPercentage: 1,
          },
        },
        scales: {
          x: {
            beginAtZero: true,
            grid: { color: grid, lineWidth: 1, drawTicks: false },
            border: { display: false },
            ticks: {
              color: muted,
              font: { size: 11 },
              maxTicksLimit: 5,
              padding: 6,
              callback: (v) => tickRef.current?.(Number(v)) ?? String(v),
            },
          },
          y: {
            grid: { display: false },
            border: { color: token('--color-border-strong', '#d4d3ce') },
            ticks: {
              color: ink,
              font: { size: 12 },
              autoSkip: false,
              callback(_value, index) {
                const label = rowsRef.current[index]?.label ?? '';
                return label.length > 26 ? `${label.slice(0, 25)}…` : label;
              },
            },
          },
        },
        plugins: {
          legend: { display: false },
          tooltip: {
            displayColors: false,
            backgroundColor: ink,
            padding: { x: 10, y: 8 },
            cornerRadius: 8,
            caretSize: 5,
            titleFont: { size: 12, weight: 600 },
            bodyFont: { size: 12 },
            callbacks: {
              title: (items) => rowsRef.current[items[0]?.dataIndex ?? 0]?.label ?? '',
              label: (item) => {
                const row = rowsRef.current[item.dataIndex];
                return row?.detail ?? [row?.display ?? ''];
              },
            },
          },
        },
        onClick: (_event, elements) => {
          const index = elements[0]?.index;
          if (index != null) selectRef.current?.(index);
        },
        onHover: (event, elements) => {
          const target = event.native?.target as HTMLElement | undefined;
          if (target) target.style.cursor = elements.length && selectRef.current ? 'pointer' : 'default';
        },
      },
      plugins: [valueLabels, thresholdLines],
    });
    chartRef.current = chart;
    return () => {
      chart.destroy();
      chartRef.current = null;
    };
  }, []);

  useEffect(() => {
    if (chartRef.current) showRows(chartRef.current, rows, max);
  }, [rows, max]);

  return (
    <div className="bar-chart" style={{ height: rows.length * ROW_HEIGHT + AXIS_BAND + (thresholds.length ? 14 : 0) }}>
      <canvas ref={canvasRef} role="img" aria-label={ariaLabel} />
    </div>
  );
}

/** A chart with its title and a table view of the same data. */
export function ChartCard({
  title,
  subtitle,
  table,
  actions,
  children,
}: {
  title: string;
  subtitle?: ReactNode;
  /** The table twin of the chart (always available). */
  table?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}) {
  const [asTable, setAsTable] = useState(false);
  return (
    <section className="viz-card">
      <header className="viz-card__head">
        <div>
          <h3 className="viz-card__title">{title}</h3>
          {subtitle && <p className="viz-card__subtitle">{subtitle}</p>}
        </div>
        <div className="viz-card__actions">
          {actions}
          {table && (
            <button type="button" className="link-button" onClick={() => setAsTable((t) => !t)}>
              {asTable ? 'Chart' : 'Table'}
            </button>
          )}
        </div>
      </header>
      {asTable && table ? table : children}
    </section>
  );
}
