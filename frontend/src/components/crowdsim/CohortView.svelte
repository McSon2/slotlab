<script lang="ts">
  import Chart from '../Chart.svelte';
  import BandChart from '../analyze/BandChart.svelte';
  import RangeCard from './RangeCard.svelte';
  import Findings from './Findings.svelte';
  import FeelView from './FeelView.svelte';
  import type { FrustrationKey, Payer } from '../../lib/feel';
  import { fx, pct, threshold } from '../../lib/populations';
  import { fmtPct } from '../../lib/api';
  import { isBuy, kpis, readCohort, tokenColor, type Cohort } from '../../lib/crowdsim';

  /** One cohort: key figures, then one section at a time. */
  type Section = 'lived' | 'feel' | 'session' | 'players' | 'bonuses' | 'wins';
  interface Props {
    c: Cohort;
    /** Section shown first. */
    initial?: Section;
    /** Plays a player-feel fix of this cohort's mode as a new cohort. */
    onsimulate?: (frustration: FrustrationKey | 'global', payer: Payer) => void;
    simulating?: string | null;
  }

  const { c, initial = 'lived', onsimulate, simulating = null }: Props = $props();

  // The view is re-created per cohort ({#key}): the first section is read once.
  let section = $state<Section>((() => initial)());

  const buy = $derived(isBuy(c));
  const lines = $derived(readCohort(c));
  const tiles = $derived(kpis(c));
  const sections = $derived(
    [
      { id: 'lived' as const, label: 'What they lived' },
      ...(c.feel ? [{ id: 'feel' as const, label: 'Player feel' }] : []),
      { id: 'session' as const, label: 'Session' },
      { id: 'players' as const, label: 'Per player' },
      ...(c.hasBonus ? [{ id: 'bonuses' as const, label: buy ? 'Purchases' : 'Bonuses' }] : []),
      { id: 'wins' as const, label: 'Wins' },
    ],
  );

  /** Metrics worth a card: drop the ones always at 0 for this cohort. */
  const metrics = $derived(c.metrics.filter((m) => !(m.min === 0 && m.max === 0)));
  const n0 = (v: number) => Math.round(v).toLocaleString('en-US');
  const labels = $derived(c.curves.checkpoints.map(String));
  const grid = { color: tokenColor('--text-dim', 0.08) };
  // Series palette (design tokens): good tail / typical / bad tail / reference.
  const HI = (a: number) => tokenColor('--pass', a);
  const MID = tokenColor('--series-2');
  const LO = (a: number) => tokenColor('--series-4', a);
  const REF = tokenColor('--gold');
  const POP = tokenColor('--series-5');

  const netChart = $derived({
    type: 'line' as const,
    data: {
      labels,
      datasets: [
        { label: 'P95', data: c.curves.netP95, borderColor: HI(0.85), pointRadius: 0, borderWidth: 1.2, fill: false },
        { label: 'P75', data: c.curves.netP75, borderColor: HI(0.45), pointRadius: 0, borderWidth: 1, fill: false },
        { label: 'median', data: c.curves.netP50, borderColor: MID, pointRadius: 0, borderWidth: 2.2, fill: false },
        { label: 'P25', data: c.curves.netP25, borderColor: LO(0.45), pointRadius: 0, borderWidth: 1, fill: false },
        { label: 'P5', data: c.curves.netP5, borderColor: LO(0.9), pointRadius: 0, borderWidth: 1.2, fill: false },
        { label: 'mean', data: c.curves.netMean, borderColor: REF, borderDash: [6, 4], pointRadius: 0, borderWidth: 1.4, fill: false },
      ],
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      animation: false as const,
      interaction: { mode: 'index' as const, intersect: false },
      plugins: { legend: { labels: { boxWidth: 10, font: { size: 10.5 } } } },
      scales: {
        x: { title: { display: true, text: 'spins' }, ticks: { maxTicksLimit: 10 }, grid },
        y: { title: { display: true, text: 'net result (bets)' }, grid },
      },
    },
  });

  const rtpChart = $derived({
    type: 'line' as const,
    data: {
      labels,
      datasets: [
        { label: 'P95', data: c.curves.rtpP95.map((v) => v * 100), borderColor: HI(0.85), pointRadius: 0, borderWidth: 1.2, fill: false },
        { label: 'median player', data: c.curves.rtpP50.map((v) => v * 100), borderColor: MID, pointRadius: 0, borderWidth: 2.2, fill: false },
        { label: 'P5', data: c.curves.rtpP5.map((v) => v * 100), borderColor: LO(0.9), pointRadius: 0, borderWidth: 1.2, fill: false },
        { label: 'math RTP', data: labels.map(() => c.theoreticalRtp * 100), borderColor: REF, borderDash: [5, 4], pointRadius: 0, borderWidth: 1.2, fill: false },
      ],
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      animation: false as const,
      interaction: { mode: 'index' as const, intersect: false },
      plugins: { legend: { labels: { boxWidth: 10, font: { size: 10.5 } } } },
      scales: {
        x: { title: { display: true, text: 'spins' }, ticks: { maxTicksLimit: 10 }, grid },
        y: { title: { display: true, text: 'session RTP (%)' }, min: 0, suggestedMax: 250, grid },
      },
    },
  });

  const popChart = $derived({
    type: 'line' as const,
    data: {
      labels,
      datasets: [{ label: 'players in profit', data: c.curves.pop.map((v) => v * 100), borderColor: POP, backgroundColor: tokenColor('--series-5', 0.12), pointRadius: 0, borderWidth: 2, fill: true }],
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      animation: false as const,
      plugins: { legend: { display: false } },
      scales: {
        x: { title: { display: true, text: 'spins' }, ticks: { maxTicksLimit: 10 }, grid },
        y: { title: { display: true, text: '% of players in profit' }, min: 0, max: 100, grid },
      },
    },
  });

  const hist = $derived.by(() => {
    let last = c.bonusHist.length - 1;
    while (last > 0 && c.bonusHist[last] === 0) last--;
    return c.bonusHist.slice(0, last + 1);
  });

  const histChart = $derived({
    type: 'bar' as const,
    data: {
      labels: hist.map((_, i) => (i === c.bonusHist.length - 1 ? `${i}+` : String(i))),
      datasets: [{ label: '% of players', data: hist.map((v) => v * 100), backgroundColor: MID, borderRadius: 3 }],
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      animation: false as const,
      plugins: { legend: { display: false } },
      scales: {
        x: { title: { display: true, text: 'bonuses in the session' }, grid: { display: false } },
        y: { title: { display: true, text: '% of players' }, grid },
      },
    },
  });
</script>

<div class="tiles">
  {#each tiles as t (t.label)}
    <div class="tile">
      <span class="t-label">{t.label}</span>
      <strong>{t.value}</strong>
      <span class="t-sub">{t.sub}</span>
    </div>
  {/each}
</div>

<div class="sections">
  <div class="pills" role="tablist" aria-label="{c.name} — sections">
    {#each sections as s (s.id)}
      <button role="tab" aria-selected={section === s.id} class:on={section === s.id} onclick={() => (section = s.id)}>{s.label}</button>
    {/each}
  </div>
  <span class="meta faint">{n0(c.players)} players × {n0(c.spins)} spins · {n0(c.rounds)} rounds · {(c.elapsedMs / 1000).toFixed(2)} s</span>
</div>

{#if section === 'lived'}
  <div class="card">
    <Findings {lines} />
  </div>
{:else if section === 'feel' && c.feel}
  <FeelView r={c.feel} onsimulate={c.fix ? undefined : onsimulate} {simulating} />
{:else if section === 'session'}
  <div class="grid2">
    <div class="card pad">
      <p class="overline">Net result over the session</p>
      <Chart config={netChart as never} height={290} />
    </div>
    <div class="card pad">
      <p class="overline">Session RTP</p>
      <Chart config={rtpChart as never} height={290} />
    </div>
    <div class="card pad">
      <p class="overline">Players in profit</p>
      <Chart config={popChart as never} height={220} />
    </div>
    <div class="card pad">
      <p class="overline">Share of players who…</p>
      <div class="shares">
        {#each c.shares as s (s.key)}
          <div class="share"><strong>{pct(s.value)}</strong><span>{s.label.toLowerCase()}</span></div>
        {/each}
      </div>
    </div>
  </div>
{:else if section === 'players'}
  <p class="hint">
    Each card: the median player in big; the bar runs from P5 to P95, shaded from P25 to P75,
    <span class="gold" aria-hidden="true">|</span> median, <span class="coral" aria-hidden="true">┊</span> mean.
  </p>
  <div class="ranges">
    {#each metrics as m (m.key)}
      <RangeCard {m} players={c.players} />
    {/each}
  </div>
{:else if section === 'bonuses'}
  <div class="grid2">
    {#if !buy}
      <div class="card pad">
        <p class="overline">Bonuses per player</p>
        <Chart config={histChart as never} height={250} />
      </div>
    {/if}
    {#each c.modes as mo (mo.mode)}
      {@const tiers = mo.populations.filter((p) => p.kind === 'tier' || p.kind === 'variant' || p.kind === 'bonus')}
      {#if tiers.length}
        <div class="card pad">
          <p class="overline">{mo.mode} — what the bonuses paid</p>
          <div class="tier-cards">
            {#each tiers as p (p.key)}
              {@const count = p.probability * mo.rounds}
              <div class="tc" class:sub={p.kind === 'variant'} class:agg={p.kind === 'bonus'}>
                <div class="tc-head">
                  <strong>{p.label}</strong>
                  <span class="faint">{n0(count)} seen · 1 in {n0(p.oneIn)}</span>
                </div>
                <div class="tc-nums">
                  <div><span>median</span><b class="gold">{fx(p.median)}</b></div>
                  <div><span>average</span><b>{fx(p.mean)}</b></div>
                  <div><span>&lt; 10x</span><b>{pct(threshold(p, 10)?.below ?? 0)}</b></div>
                  <div><span>best</span><b>{fx(p.max)}</b></div>
                </div>
              </div>
            {/each}
          </div>
        </div>
      {/if}
    {/each}
  </div>
{:else if section === 'wins'}
  <div class="grid2">
    {#each c.modes as mo (mo.mode)}
      {@const all = mo.populations.find((p) => p.kind === 'all')}
      <div class="card pad">
        <p class="overline">{mo.mode} — where results fell vs where the money came from</p>
        {#if all}<BandChart series={[{ label: mo.mode, color: MID, bands: all.bands }]} height={250} />{/if}
        <p class="foot">{n0(mo.rounds)} rounds{c.mix.length > 1 ? ` (${pct(mo.roundShare)})` : ''} · RTP {fmtPct(mo.rtp)} for {fmtPct(mo.theoreticalRtp)}</p>
      </div>
      <div class="card pad">
        <p class="overline">{mo.mode} — big wins: expected vs seen</p>
        <div class="rare">
          {#each mo.rare as r (r.atLeast)}
            {@const ratio = r.expected > 0 ? r.observed / r.expected : 0}
            {@const off = r.expected >= 5 && Math.abs(ratio - 1) > 0.25}
            <div class="rr">
              <span class="rr-x">≥ {fx(r.atLeast)}</span>
              <span class="rr-bar" aria-hidden="true"><span style="width: {Math.min(100, ratio * 50)}%" class:off></span><i></i></span>
              <span class="rr-n"><b>{n0(r.observed)}</b> seen · {r.expected < 10 ? r.expected.toFixed(1) : n0(r.expected)} expected{#if off} <span class="off-tag">{ratio > 1 ? 'more' : 'fewer'} than expected</span>{/if}</span>
            </div>
          {/each}
        </div>
        <p class="foot">
          The bar is “seen ÷ expected”: the middle line is exactly the math. Catching more or fewer
          of the rare wins is what moves the empirical RTP away from {fmtPct(mo.theoreticalRtp)}.
        </p>
      </div>
    {/each}
  </div>
{/if}

<style>
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
    gap: 10px;
    margin-bottom: 16px;
  }

  .tile {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    background: var(--bg-panel);
  }

  .t-label {
    font-size: var(--fs-2xs);
    text-transform: uppercase;
    letter-spacing: 0.07em;
    color: var(--text-faint);
  }

  .tile strong {
    font-family: var(--font-mono);
    font-size: var(--fs-xl);
    font-weight: 650;
    color: var(--text);
  }

  .t-sub {
    font-size: var(--fs-xs);
    color: var(--text-dim);
  }

  .sections {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-bottom: 14px;
  }

  .pills {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .pills button {
    padding: 5px 13px;
    border-radius: var(--r-pill);
    font-size: var(--fs-sm);
    background: var(--bg-panel);
  }

  .pills button.on {
    border-color: var(--gold);
    color: var(--gold);
    background: color-mix(in oklch, var(--gold) var(--tint-weak), transparent);
  }

  .meta {
    margin-left: auto;
    font-size: var(--fs-xs);
  }

  .card {
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    background: var(--bg-panel);
    overflow: hidden;
  }

  .card.pad {
    padding: 12px 14px;
  }

  .grid2 {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(520px, 1fr));
    gap: 12px;
  }

  @media (max-width: 1100px) {
    .grid2 {
      grid-template-columns: 1fr;
    }
  }

  .shares {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 10px;
    margin-top: 6px;
  }

  .share {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 10px;
    border-radius: var(--r-s);
    background: var(--bg-raised);
  }

  .share strong {
    font-family: var(--font-mono);
    font-size: var(--fs-md);
  }

  .share span {
    font-size: var(--fs-xs);
    color: var(--text-dim);
  }

  .hint {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    margin: 0 0 10px;
  }

  .gold {
    color: var(--gold);
  }

  .coral {
    color: var(--series-4);
  }

  .ranges {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: 10px;
  }

  .tier-cards {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 6px;
  }

  .tc {
    padding: 9px 11px;
    border-radius: var(--r-s);
    background: var(--bg-raised);
    border-left: 3px solid var(--series-2);
  }

  .tc.agg {
    border-left-color: var(--gold);
  }

  .tc.sub {
    margin-left: 18px;
    border-left-color: var(--border-strong);
  }

  .tc-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 10px;
    font-size: var(--fs-sm);
  }

  .tc-head span {
    font-size: var(--fs-xs);
  }

  .tc-nums {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
    margin-top: 6px;
  }

  .tc-nums div {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .tc-nums span {
    font-size: var(--fs-2xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-faint);
  }

  .tc-nums b {
    font-family: var(--font-mono);
    font-size: var(--fs-md);
    font-weight: 600;
  }

  .rare {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 8px;
  }

  .rr {
    display: grid;
    grid-template-columns: 80px 1fr auto;
    align-items: center;
    gap: 12px;
    font-size: var(--fs-sm);
  }

  .rr-x {
    font-family: var(--font-mono);
  }

  .rr-bar {
    position: relative;
    height: 8px;
    border-radius: var(--r-s);
    background: var(--border);
  }

  .rr-bar span {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    border-radius: var(--r-s);
    background: color-mix(in oklch, var(--series-2) 70%, transparent);
  }

  .rr-bar span.off {
    background: var(--warn);
  }

  .rr-bar i {
    position: absolute;
    left: 50%;
    top: -3px;
    height: 14px;
    border-left: 2px solid var(--gold);
  }

  .rr-n {
    color: var(--text-dim);
    font-size: var(--fs-xs);
  }

  .off-tag {
    color: var(--warn);
  }

  .rr-n b {
    color: var(--text);
    font-family: var(--font-mono);
  }

  .foot {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    margin: 10px 0 0;
    line-height: 1.5;
  }
</style>
