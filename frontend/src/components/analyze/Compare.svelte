<script lang="ts">
  import CdfChart from './CdfChart.svelte';
  import BandChart from './BandChart.svelte';
  import ConfirmButton from '../ConfirmButton.svelte';
  import Icon from '../Icon.svelte';
  import {
    fx,
    pct,
    oneIn,
    threshold,
    quantile,
    seriesColor,
    refId,
    type ModePopulations,
    type Population,
    type PopRef,
  } from '../../lib/populations';

  /**
   * Any populations of any modes, side by side: 3 vs 4 scatters, the same
   * tier in Base and in Hunt, a variant against its tier, a bought bonus
   * against the natural one. Same curves, same bands, same metrics.
   */
  interface Props {
    all: ModePopulations[];
    pinned: PopRef[];
    customThresholds: number[];
    onUnpin: (ref: PopRef) => void;
    onClear: () => void;
    onPinMany: (refs: PopRef[]) => void;
    onOpen: (ref: PopRef) => void;
  }

  const { all, pinned, customThresholds, onUnpin, onClear, onPinMany, onOpen }: Props = $props();

  interface Col {
    ref: PopRef;
    mp: ModePopulations;
    pop: Population;
    color: string;
    name: string;
  }

  const cols = $derived.by(() => {
    const out: Col[] = [];
    for (const r of pinned) {
      const mp = all.find((m) => m.mode === r.mode);
      const pop = mp?.populations.find((p) => p.key === r.key);
      if (mp && pop) out.push({ ref: r, mp, pop, color: seriesColor(out.length), name: `${r.mode} · ${pop.label}` });
    }
    return out;
  });
  const missing = $derived(pinned.length - cols.length);

  // ── Presets ──
  const tierKeysAcross = $derived.by(() => {
    const count = new Map<string, { label: string; refs: PopRef[] }>();
    for (const mp of all)
      for (const p of mp.populations.filter((p) => p.kind === 'tier')) {
        const e = count.get(p.key) ?? { label: p.label, refs: [] };
        e.refs.push({ mode: mp.mode, key: p.key });
        count.set(p.key, e);
      }
    return [...count.values()].filter((e) => e.refs.length > 1);
  });
  const modesWithTiers = $derived(all.filter((mp) => mp.populations.filter((p) => p.kind === 'tier').length > 1));
  const bonusOfEveryMode = $derived(
    all
      .map((mp) => {
        const b = mp.populations.find((p) => p.kind === 'bonus') ?? (mp.populations.some((p) => p.kind === 'tier') ? mp.populations.find((p) => p.kind === 'all') : undefined);
        return b ? { mode: mp.mode, key: b.key } : null;
      })
      .filter((r): r is PopRef => r !== null),
  );

  // ── Matrix ──
  interface Row {
    label: string;
    get: (c: Col) => number | null;
    fmt: (v: number) => string;
    /** Highlight the highest value of the row. */
    hi?: boolean;
    section?: string;
  }

  const thr = (x: number): Row => ({
    label: `Under ${fx(x)}`,
    get: (c) => threshold(c.pop, x)?.below ?? null,
    fmt: pct,
    hi: true,
  });

  const rows = $derived.by((): Row[] => [
    { section: 'How often', label: 'Frequency (per spin)', get: (c) => c.pop.oneIn, fmt: oneIn },
    { label: 'Share of bonuses', get: (c) => c.pop.shareOfBonuses, fmt: pct },
    { label: 'Share of its mode RTP', get: (c) => c.pop.rtpShare, fmt: pct, hi: true },
    { label: 'Mode cost', get: (c) => c.mp.cost, fmt: fx },
    { section: 'Typical vs average', label: 'Median', get: (c) => c.pop.median, fmt: fx, hi: true },
    { label: 'Average win', get: (c) => c.pop.mean, fmt: fx, hi: true },
    { label: 'Average ÷ median', get: (c) => c.pop.meanOverMedian, fmt: (v) => v.toFixed(2) + '×', hi: true },
    { label: 'Average ÷ cost', get: (c) => c.pop.mean / (c.mp.cost || 1), fmt: (v) => v.toFixed(3) + '×' },
    { label: 'Average without top 1 %', get: (c) => c.pop.meanWithoutTop1, fmt: fx, hi: true },
    { section: 'Spread', label: 'P10', get: (c) => quantile(c.pop, 0.1)?.value ?? null, fmt: fx },
    { label: 'P25', get: (c) => quantile(c.pop, 0.25)?.value ?? null, fmt: fx },
    { label: 'P75', get: (c) => quantile(c.pop, 0.75)?.value ?? null, fmt: fx },
    { label: 'P90', get: (c) => quantile(c.pop, 0.9)?.value ?? null, fmt: fx },
    { label: 'P99', get: (c) => quantile(c.pop, 0.99)?.value ?? null, fmt: fx },
    { label: 'Min', get: (c) => c.pop.min, fmt: fx },
    { label: 'Max', get: (c) => c.pop.max, fmt: fx },
    { label: 'Std dev', get: (c) => c.pop.stdDev, fmt: fx },
    { label: 'Coeff. of variation', get: (c) => c.pop.cv, fmt: (v) => v.toFixed(2), hi: true },
    { label: 'Top 1 % carries', get: (c) => c.pop.top1Share, fmt: pct, hi: true },
    { label: 'Top 10 % carries', get: (c) => c.pop.top10Share, fmt: pct, hi: true },
    { section: 'Outcomes', label: 'Pay nothing (0x)', get: (c) => c.pop.zeroRate, fmt: pct, hi: true },
    { label: 'Return ≥ its mode cost', get: (c) => c.pop.profitableRate, fmt: pct },
    ...[...new Set([10, 20, 50, 100, ...customThresholds])].sort((a, b) => a - b).map(thr),
    { section: 'Retriggers', label: 'Retrigger at least once', get: (c) => c.pop.retriggers?.rate ?? null, fmt: pct, hi: true },
    { label: 'Average retriggers', get: (c) => c.pop.retriggers?.mean ?? null, fmt: (v) => v.toFixed(2) },
    { label: 'Avg win with retrigger', get: (c) => c.pop.retriggers?.avgWith ?? null, fmt: fx },
    { label: 'Avg win without', get: (c) => c.pop.retriggers?.avgWithout ?? null, fmt: fx },
    { label: 'Value from retriggered', get: (c) => c.pop.retriggers?.valueShareWith ?? null, fmt: pct },
    { label: 'Free spins awarded', get: (c) => c.pop.spins?.meanInitial ?? null, fmt: (v) => v.toFixed(2) },
    { label: 'Free spins played', get: (c) => c.pop.spins?.meanFinal ?? null, fmt: (v) => v.toFixed(2) },
    { label: 'Bonus alone — median', get: (c) => c.pop.bonusOnly?.median ?? null, fmt: fx },
    { section: 'Sample', label: 'Books (drawable)', get: (c) => c.pop.drawableBooks, fmt: (v) => v.toLocaleString('en-US') },
    { label: 'Effective outcomes', get: (c) => c.pop.effectiveOutcomes, fmt: (v) => Math.round(v).toLocaleString('en-US') },
  ]);

  /** Rows where every column is empty are dropped (e.g. retriggers on base spins). */
  const shownRows = $derived(rows.filter((r) => cols.some((c) => r.get(c) !== null)));

  function rowMax(r: Row): number | null {
    const vals = cols.map((c) => r.get(c)).filter((v): v is number => v !== null && isFinite(v));
    if (!r.hi || vals.length < 2) return null;
    const m = Math.max(...vals);
    return vals.every((v) => v === m) ? null : m;
  }

  const sharedCost = $derived(cols.length && cols.every((c) => c.mp.cost === cols[0].mp.cost) ? cols[0].mp.cost : 1);
</script>

<section class="page">
  <div class="presets">
    <span class="dim">Quick sets:</span>
    {#each modesWithTiers as mp (mp.mode)}
      <button class="ghost" onclick={() => onPinMany(mp.populations.filter((p) => p.kind === 'tier').map((p) => ({ mode: mp.mode, key: p.key })))}>
        Tiers of {mp.mode}
      </button>
    {/each}
    {#each tierKeysAcross as t (t.label)}
      <button class="ghost" onclick={() => onPinMany(t.refs)}>{t.label} across modes</button>
    {/each}
    {#if bonusOfEveryMode.length > 1}
      <button class="ghost" onclick={() => onPinMany(bonusOfEveryMode)}>Bonuses of every mode</button>
    {/if}
    {#if pinned.length}
      <ConfirmButton onconfirm={onClear} confirmLabel="Clear all pins?" ariaLabel="Clear all {pinned.length} pinned populations">
        Clear
      </ConfirmButton>
    {/if}
  </div>

  {#if cols.length === 0}
    <p class="dim empty">
      Nothing to compare yet. Pin populations with <strong>+ Compare</strong> in Explore
      (a tier, a variant, a criteria, a whole mode — from any mode), or start from a quick set above.
    </p>
  {:else}
    <div class="chips">
      {#each cols as c (refId(c.ref))}
        <span class="chip" style="--c: {c.color}">
          <i aria-hidden="true"></i>
          <button class="link" onclick={() => onOpen(c.ref)} title="Open in Explore">{c.name}</button>
          <button class="rm" title="Remove from Compare" aria-label="Remove {c.name} from Compare" onclick={() => onUnpin(c.ref)}>
            <Icon name="close" size={12} />
          </button>
        </span>
      {/each}
      {#if missing > 0}<span class="faint">{missing} pinned population(s) not in the current math</span>{/if}
    </div>

    <div class="charts">
      <section>
        <p class="overline">Share of results below x</p>
        <CdfChart series={cols.map((c) => ({ label: c.name, color: c.color, points: c.pop.cdf, max: c.pop.max }))} cost={sharedCost} height={320} />
      </section>
      <section>
        <p class="overline">Where the results fall (% per band)</p>
        <BandChart series={cols.map((c) => ({ label: c.name, color: c.color, bands: c.pop.bands }))} height={320} />
      </section>
    </div>

    <div class="matrix-wrap">
      <table class="matrix">
        <thead>
          <tr>
            <th><span class="sr-only">Metric</span></th>
            {#each cols as c (refId(c.ref))}
              <th class="num"><span class="swatch" style="background: {c.color}" aria-hidden="true"></span>{c.name}</th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each shownRows as r (r.label)}
            {@const m = rowMax(r)}
            {#if r.section}
              <tr class="section"><td colspan={cols.length + 1}>{r.section}</td></tr>
            {/if}
            <tr>
              <td class="dim">{r.label}</td>
              {#each cols as c (refId(c.ref))}
                {@const v = r.get(c)}
                <td class="num" class:hi={m !== null && v === m}>
                  {v === null ? '—' : r.fmt(v)}{#if m !== null && v === m}<span class="sr-only"> (highest)</span>{/if}
                </td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="note">
      Wins are in multiples of the base bet in every column, so a bought bonus and a
      natural one read on the same scale. Highlighted: the highest value of a row.
      Frequency is per spin of each population's own mode.
    </p>
  {/if}
</section>

<style>
  .presets {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-bottom: 14px;
  }

  .presets .dim {
    font-size: var(--fs-sm);
    margin-right: var(--sp-1);
  }

  .empty {
    max-width: 640px;
    line-height: 1.6;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 16px;
    align-items: center;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 4px 3px 8px;
    border: 1px solid var(--border);
    border-radius: var(--r-pill);
    background: var(--bg-panel);
    font-size: var(--fs-sm);
  }

  .chip i {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--c);
  }

  .chip .rm {
    display: inline-flex;
    align-items: center;
    padding: 3px;
    border-radius: var(--r-pill);
  }

  button.link {
    background: none;
    border: none;
    padding: 0;
    color: var(--text);
    cursor: pointer;
  }

  button.link:hover {
    color: var(--gold);
  }

  .charts {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 28px;
    margin-bottom: 22px;
  }

  @media (max-width: 1100px) {
    .charts {
      grid-template-columns: 1fr;
    }
  }

  .matrix-wrap {
    overflow-x: auto;
  }

  .matrix th {
    white-space: nowrap;
    font-size: var(--fs-xs);
  }

  .swatch {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    margin-right: 6px;
  }

  .matrix tr.section td {
    padding-top: 14px;
    font-size: var(--fs-2xs);
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-faint);
    border-bottom: 1px solid var(--border);
  }

  .matrix td.hi {
    color: var(--gold);
    font-weight: 620;
  }

  .note {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    max-width: 820px;
    margin: 12px 0 0;
    line-height: 1.55;
  }
</style>
