<script lang="ts">
  import Chart from '../Chart.svelte';
  import { bandLabel, chartColors, pct, type Band } from '../../lib/populations';

  /**
   * Where the results fall vs where the money comes from, band by band.
   * One population: two bars per band (% of results, % of value) — a mass on
   * the left and a value on the right is exactly an average carried by a few
   * big wins. Several populations: % of results per band, side by side.
   *
   * The exact-zero band is left out of the chart (it would flatten every
   * other bar on a base mode); its share is given in the caption.
   */
  interface Series {
    label: string;
    color: string;
    bands: Band[];
  }

  interface Props {
    series: Series[];
    height?: number;
  }

  const { series, height = 280 }: Props = $props();

  /** Band indices from the first to the last non-empty one, zero excluded. */
  const range = $derived.by(() => {
    const n = series[0]?.bands.length ?? 0;
    let lo = n;
    let hi = -1;
    for (const s of series) {
      s.bands.forEach((b, i) => {
        if (i > 0 && b.share > 0) {
          lo = Math.min(lo, i);
          hi = Math.max(hi, i);
        }
      });
    }
    return hi < lo ? [] : Array.from({ length: hi - lo + 1 }, (_, k) => lo + k);
  });

  const zeroCaption = $derived(
    series
      .filter((s) => (s.bands[0]?.share ?? 0) > 0)
      .map((s) => (series.length > 1 ? `${s.label} ${pct(s.bands[0].share)}` : pct(s.bands[0].share)))
      .join(' · '),
  );

  /** What the canvas shows, for screen readers. */
  const ariaLabel = $derived.by(() => {
    if (!range.length) return '';
    const from = bandLabel(series[0].bands[range[0]]);
    const to = bandLabel(series[0].bands[range[range.length - 1]]);
    if (series.length === 1) {
      const s = series[0];
      const top = range.reduce((a, i) => (s.bands[i].share > s.bands[a].share ? i : a), range[0]);
      const val = range.reduce((a, i) => (s.bands[i].valueShare > s.bands[a].valueShare ? i : a), range[0]);
      return (
        `Bar chart of ${s.label}: share of results and share of value per win band, from ${from} to ${to}x. ` +
        `Most results fall in ${bandLabel(s.bands[top])}x (${pct(s.bands[top].share)}); ` +
        `most value comes from ${bandLabel(s.bands[val])}x (${pct(s.bands[val].valueShare)}).`
      );
    }
    return `Bar chart: share of results per win band, from ${from} to ${to}x, for ${series.map((s) => s.label).join(', ')}.`;
  });

  const config = $derived.by(() => {
    const colors = chartColors();
    const labels = range.map((i) => bandLabel(series[0].bands[i]));
    const datasets =
      series.length === 1
        ? [
            {
              label: '% of results',
              data: range.map((i) => series[0].bands[i].share * 100),
              backgroundColor: colors.results,
              borderRadius: 2,
            },
            {
              label: '% of value (RTP)',
              data: range.map((i) => series[0].bands[i].valueShare * 100),
              backgroundColor: colors.value,
              borderRadius: 2,
            },
          ]
        : series.map((s) => ({
            label: s.label,
            data: range.map((i) => s.bands[i].share * 100),
            backgroundColor: s.color,
            borderRadius: 2,
          }));
    return {
      type: 'bar' as const,
      data: { labels, datasets },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        animation: false as const,
        plugins: {
          legend: { display: true, labels: { boxWidth: 10, font: { size: 11 } } },
          tooltip: {
            callbacks: {
              label: (c: { dataset: { label?: string }; parsed: { y: number } }) =>
                `${c.dataset.label}: ${c.parsed.y.toFixed(3)} %`,
            },
          },
        },
        scales: {
          x: { title: { display: true, text: 'win band (x base bet)' }, grid: { display: false } },
          y: { title: { display: true, text: '%' }, grid: { color: colors.gridFaint } },
        },
      },
    };
  });
</script>

{#if range.length}
  <Chart config={config as never} {height} label={ariaLabel} />
  {#if zeroCaption}<p class="caption">Exact 0x not drawn: {zeroCaption}</p>{/if}
{:else}
  <p class="caption">Every result is 0x.</p>
{/if}

<style>
  .caption {
    margin: var(--sp-1) 0 0;
    font-size: var(--fs-xs);
    color: var(--text-faint);
  }
</style>
