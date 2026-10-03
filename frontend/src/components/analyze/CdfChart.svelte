<script lang="ts">
  import Chart from '../Chart.svelte';
  import { chartColors, fx, pct, type CdfPoint } from '../../lib/populations';

  /**
   * « Share of results below x » — the curve that answers « what fraction
   * ends under 10x? » at a glance. Log x axis: slot wins span five decades.
   * Each point is exact (measured by the backend), the line only joins them.
   */
  interface Series {
    label: string;
    color: string;
    points: CdfPoint[];
    /** Vertical markers (single-series view only). */
    median?: number;
    mean?: number;
    max: number;
  }

  interface Props {
    series: Series[];
    height?: number;
    /** Mode cost: drawn as a marker when > 1 (break-even of a buy). */
    cost?: number;
  }

  const { series, height = 280, cost = 1 }: Props = $props();

  const xMax = $derived(Math.min(100_000, Math.max(10, ...series.map((s) => s.max * 1.6))));

  function trim(points: CdfPoint[]): { x: number; y: number }[] {
    const out: { x: number; y: number }[] = [];
    for (const p of points) {
      if (p.x > xMax) break;
      out.push({ x: p.x, y: p.below * 100 });
      if (p.below >= 1) break;
    }
    return out;
  }

  /** What the canvas shows, for screen readers: where each curve crosses 10x and 100x. */
  const ariaLabel = $derived.by(() => {
    const below = (s: Series, x: number) => {
      let v = 0;
      for (const p of s.points) {
        if (p.x > x) break;
        v = p.below;
      }
      return v;
    };
    const parts = series.map(
      (s) => `${s.label}: ${pct(below(s, 10))} below 10x, ${pct(below(s, 100))} below 100x, max ${fx(s.max)}`,
    );
    return `Line chart, share of results below each win (log scale). ${parts.join('; ')}.`;
  });

  const config = $derived.by(() => {
    const colors = chartColors();
    const single = series.length === 1 ? series[0] : null;
    const marker = (x: number, label: string, color: string) => ({
      label,
      data: [
        { x, y: 0 },
        { x, y: 100 },
      ],
      borderColor: color,
      borderDash: [4, 4],
      borderWidth: 1.2,
      pointRadius: 0,
      fill: false,
    });
    const datasets: Record<string, unknown>[] = series.map((s) => ({
      label: s.label,
      data: trim(s.points),
      borderColor: s.color,
      backgroundColor: s.color,
      borderWidth: 2,
      pointRadius: 2.2,
      pointHoverRadius: 4,
      tension: 0,
      fill: false,
    }));
    if (single) {
      if (single.median !== undefined && single.median > 0) datasets.push(marker(single.median, `median ${fx(single.median)}`, colors.median));
      if (single.mean !== undefined && single.mean > 0) datasets.push(marker(single.mean, `average ${fx(single.mean)}`, colors.mean));
    }
    if (cost > 1) datasets.push(marker(cost, `cost ${fx(cost)}`, colors.cost));
    return {
      type: 'line' as const,
      data: { datasets },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        animation: false as const,
        interaction: { mode: 'nearest' as const, intersect: false },
        plugins: {
          legend: { display: true, labels: { boxWidth: 10, font: { size: 11 } } },
          tooltip: {
            callbacks: {
              label: (c: { dataset: { label?: string }; parsed: { x: number; y: number } }) =>
                `${c.dataset.label}: ${c.parsed.y.toFixed(2)} % below ${fx(c.parsed.x)}`,
            },
          },
        },
        scales: {
          x: {
            type: 'logarithmic' as const,
            min: 0.1,
            max: xMax,
            title: { display: true, text: 'win (x base bet) — log scale' },
            grid: { color: colors.gridFaint },
            ticks: {
              callback: (v: number | string) => {
                const n = Number(v);
                const lead = n / 10 ** Math.floor(Math.log10(n));
                return [1, 2, 5].some((m) => Math.abs(lead - m) < 1e-6) ? fx(n) : '';
              },
            },
          },
          y: {
            min: 0,
            max: 100,
            title: { display: true, text: '% of results below x' },
            grid: { color: colors.gridFaint },
          },
        },
      },
    };
  });
</script>

<Chart config={config as never} {height} label={ariaLabel} />
