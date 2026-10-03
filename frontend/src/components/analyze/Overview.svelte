<script lang="ts">
  import { fmtOneIn, fmtPct, type ModeStats, type GameCheckReport } from '../../lib/api';
  import { fx, pct, oneIn, type ModePopulations } from '../../lib/populations';
  import { issuesOf, type Issue, type Severity, type Target } from '../../lib/findings';

  /**
   * The whole game on one screen: what deserves a look (findings computed
   * from every population), every mode side by side, then the selected mode's
   * spin-level picture and tail risk.
   */
  interface Props {
    stats: { mode: string; stats: ModeStats }[];
    checks: GameCheckReport;
    pops: ModePopulations[];
    selectedMode: string;
    /** Everything worth a look — computed by the page (`lib/findings.ts`). */
    issues: Issue[];
    /** Bonus analysis and celebration ladders loaded: the list is complete. */
    ready: boolean;
    onSelectMode: (mode: string) => void;
    onTarget: (t: Target) => void;
  }

  const { stats, checks, pops, selectedMode, issues, ready, onSelectMode, onTarget }: Props = $props();

  const current = $derived(stats.find((m) => m.mode === selectedMode)?.stats);

  // ── Worth a look: one card per mode, one line per problem ──
  let showContext = $state(false);
  let openLine = $state<string | null>(null);
  const count = (sv: Severity) => issues.filter((i) => i.severity === sv).length;
  /** Severity → global pill colour (same scale as `--sv-*`). */
  const SV_PILL: Record<Severity, string> = { blocking: 'red', serious: 'yellow', minor: 'gold', context: 'blue' };
  const FAULTS: Severity[] = ['blocking', 'serious', 'minor'];
  const RANK: Record<Severity, number> = { blocking: 0, serious: 1, minor: 2, context: 3 };
  const cards = $derived.by(() => {
    const cols: (string | null)[] = [...stats.map((m) => m.mode), null];
    return cols
      .map((mode) => ({
        mode,
        cost: stats.find((m) => m.mode === mode)?.stats.cost ?? null,
        all: issuesOf(issues, mode),
      }))
      .map((c) => ({ ...c, shown: c.all.filter((i) => showContext || i.severity !== 'context') }))
      .filter((c) => c.shown.length > 0)
      .sort((a, b) => RANK[a.shown[0].severity] - RANK[b.shown[0].severity] || b.shown.length - a.shown.length);
  });
  const clean = $derived(
    stats.map((m) => m.mode).filter((m) => !issuesOf(issues, m).some((i) => i.severity !== 'context')),
  );
  const tally = (list: Issue[]) =>
    (['blocking', 'serious', 'minor'] as Severity[])
      .map((sv) => ({ sv, n: list.filter((i) => i.severity === sv).length }))
      .filter((x) => x.n > 0);

  /** Stake-style volatility label (LOW/MEDIUM/HIGH/EXTREME), from std/cost. */
  function volLabel(std: number): { label: string; cls: string } {
    if (std >= 32) return { label: 'EXTREME', cls: 'red' };
    if (std >= 16) return { label: 'HIGH', cls: 'yellow' };
    if (std >= 6) return { label: 'MEDIUM', cls: 'blue' };
    return { label: 'LOW', cls: 'green' };
  }

  function bonusOf(mode: string) {
    const mp = pops.find((m) => m.mode === mode);
    if (!mp || !mp.imported) return null;
    const b = mp.populations.find((p) => p.kind === 'bonus');
    if (b) return { pop: b, every: false };
    // A buy: every spin is a bonus.
    const tiers = mp.populations.filter((p) => p.kind === 'tier');
    const all = mp.populations.find((p) => p.kind === 'all');
    return tiers.length && all ? { pop: all, every: true } : null;
  }

  const breakdown = $derived.by(() => {
    if (!current) return null;
    const dead = current.zeroRate;
    const win = current.breakevenRate;
    return { dead, sub: Math.max(0, current.hitRate - win), win };
  });

  function streak1in1000(p: number): number {
    if (p <= 0 || p >= 1) return 0;
    return Math.log(0.001) / Math.log(p);
  }

  const streaks = $derived.by(() => {
    if (!current) return null;
    return {
      betweenWin: 1 / Math.max(1e-9, current.hitRate),
      worstZero: streak1in1000(current.zeroRate),
      betweenProfit: 1 / Math.max(1e-9, current.breakevenRate),
      worstLoss: streak1in1000(1 - current.breakevenRate),
    };
  });
</script>

<section class="page">
  <div class="wal-head">
    <p class="overline">Worth a look</p>
    {#each FAULTS as sv (sv)}
      <span class="pill {SV_PILL[sv]}" class:zero={!count(sv)}>{count(sv)} {sv}</span>
    {/each}
    <button
      class="pill {SV_PILL.context}"
      class:zero={!count('context')}
      aria-expanded={showContext}
      onclick={() => (showContext = !showContext)}
    >
      {showContext ? 'hide' : 'show'} {count('context')} context
    </button>
  </div>

  {#if !ready}
    <p class="pending">Analysing bonuses, variants and celebrations…</p>
  {:else if cards.length === 0}
    <p class="all-clear">Nothing to report — every rule passes on every mode.</p>
  {:else}
    <div class="wal-cards">
      {#each cards as c (c.mode ?? 'game')}
        <section class="wal-card {c.shown[0].severity}">
          <header>
            <strong>{c.mode ?? 'Whole game'}</strong>
            {#if c.cost !== null}<span class="cost">{c.cost}x</span>{/if}
            <span class="tally">
              {#each tally(c.all) as t (t.sv)}<span class="t {t.sv}">{t.n} {t.sv}</span>{/each}
            </span>
          </header>
          <ul>
            {#each c.shown as it, i (i)}
              {@const key = `${c.mode}|${it.rule}|${it.title}`}
              {@const moreId = `wal-${(c.mode ?? 'game').replace(/\W+/g, '-')}-${i}`}
              <li class={it.severity}>
                <button
                  class="line"
                  aria-expanded={openLine === key}
                  aria-controls={moreId}
                  onclick={() => (openLine = openLine === key ? null : key)}
                >
                  <i class="sev" title={it.severity} aria-hidden="true"></i>
                  <span class="title"><span class="sr-only">{it.severity}: </span>{it.title}</span>
                  <span class="big">{it.big}</span>
                  <span class="ref">{it.ref}</span>
                </button>
                {#if openLine === key}
                  <div class="more" id={moreId}>
                    <p class="spec">{it.text}</p>
                    {#if it.why}<p class="why">{it.why}</p>{/if}
                    <div class="more-foot">
                      {#if (it.also?.length ?? 0) > 0 || (it.mode && c.mode !== it.mode)}
                        <span class="faint">Same in {[it.mode, ...(it.also ?? [])].filter((m) => m && m !== c.mode).join(', ')}</span>
                      {/if}
                      {#if it.target}
                        {@const t = it.target}
                        <button class="ghost tiny" onclick={() => onTarget(t)}>Open</button>
                      {/if}
                    </div>
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>
  {/if}
  {#if ready && clean.length}
    <p class="clean">✓ Nothing to report in {clean.join(', ')}.</p>
  {/if}

  <p class="overline gap-lg">Every mode</p>
  <div class="wrap">
    <table>
      <thead>
        <tr>
          <th>Mode</th>
          <th class="num">Cost</th>
          <th class="num">RTP</th>
          <th class="num">Hit rate</th>
          <th class="num">Std ÷ cost</th>
          <th class="num">Max win</th>
          <th class="num">Max win HR</th>
          <th class="num">Bonus</th>
          <th class="num">Bonus RTP share</th>
          <th class="num">Bonus median · avg</th>
          <th class="num">Checks</th>
        </tr>
      </thead>
      <tbody>
        {#each stats as m (m.mode)}
          {@const vol = volLabel(m.stats.stdDev / Math.max(1, m.stats.cost))}
          {@const b = bonusOf(m.mode)}
          {@const mc = checks.modes.find((c) => c.mode === m.mode)}
          <!-- Row click for the mouse; the mode button is the keyboard path. -->
          <tr class:row-active={m.mode === selectedMode} onclick={() => onSelectMode(m.mode)}>
            <td>
              <button class="ghost mode-link" aria-pressed={m.mode === selectedMode} onclick={() => onSelectMode(m.mode)}>
                {m.mode}
              </button>
            </td>
            <td class="num faint">{fx(m.stats.cost)}</td>
            <td class="num">{fmtPct(m.stats.rtp, 2)}</td>
            <td class="num">{fmtPct(m.stats.hitRate, 2)}</td>
            <td class="num">{(m.stats.stdDev / Math.max(1, m.stats.cost)).toFixed(2)} <span class="pill {vol.cls} tiny">{vol.label}</span></td>
            <td class="num">{fx(m.stats.maxPayout)}</td>
            <td class="num faint">{fmtOneIn(m.stats.maxPayoutOneIn)}</td>
            <td class="num">{b ? (b.every ? 'every spin' : oneIn(b.pop.oneIn)) : '—'}</td>
            <td class="num">{b ? pct(b.pop.rtpShare) : '—'}</td>
            <td class="num">{b ? `${fx(b.pop.median)} · ${fx(b.pop.mean)}` : '—'}</td>
            <td class="num">
              {#if mc}
                <span class="pill {mc.passed === mc.total ? 'green' : 'red'} tiny">
                  {mc.passed}/{mc.total}<span class="sr-only"> checks pass</span>
                </span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  {#if current && breakdown && streaks}
    <p class="overline gap-xl">{selectedMode} — every spin</p>
    <div class="statline hero">
      <div><span class="dim">RTP</span><strong class="gold">{fmtPct(current.rtp, 2)}</strong></div>
      <div><span class="dim">Hit rate</span><strong>{fmtPct(current.hitRate, 2)}</strong></div>
      <div><span class="dim">Std dev ÷ cost</span><strong>{(current.stdDev / Math.max(1, current.cost)).toFixed(2)}</strong></div>
      <div><span class="dim">Median spin</span><strong>{fx(current.medianPayout)}</strong></div>
      <div><span class="dim">Max win</span><strong>{fx(current.maxPayout)}</strong></div>
      <div><span class="dim">Max win HR</span><strong>{fmtOneIn(current.maxPayoutOneIn)}</strong></div>
    </div>

    <div class="split">
      <div>
        <p class="overline">Outcome breakdown</p>
        <div class="breakdown-bar" aria-hidden="true">
          <span class="seg dead" style="width: {breakdown.dead * 100}%"></span>
          <span class="seg subbet" style="width: {breakdown.sub * 100}%"></span>
          <span class="seg win" style="width: {breakdown.win * 100}%"></span>
        </div>
        <div class="breakdown-legend">
          <span><i class="dot dead" aria-hidden="true"></i> Dead {pct(breakdown.dead)}</span>
          <span><i class="dot subbet" aria-hidden="true"></i> Under the cost {pct(breakdown.sub)}</span>
          <span><i class="dot win" aria-hidden="true"></i> ≥ cost {pct(breakdown.win)}</span>
        </div>
      </div>
      <div>
        <p class="overline">Streaks (1 in 1000)</p>
        <div class="statline">
          <div><span class="dim">Between 2 wins</span><strong>{streaks.betweenWin.toFixed(0)}</strong></div>
          <div><span class="dim">Worst 0 streak</span><strong>{streaks.worstZero.toFixed(0)}</strong></div>
          <div><span class="dim">Between 2 profits</span><strong>{streaks.betweenProfit.toFixed(0)}</strong></div>
          <div><span class="dim">Worst loss streak</span><strong>{streaks.worstLoss.toFixed(0)}</strong></div>
        </div>
      </div>
    </div>

    <p class="overline gap-lg">Tail risk <span class="faint">(SlotLab estimate)</span></p>
    <div class="statline">
      <div><span class="dim">CVaR 0.1 %</span><strong>{fx(current.cvar001)}</strong></div>
      <div><span class="dim">ETL 40x</span><strong>{current.etl40x.toFixed(4)}</strong></div>
      <div><span class="dim">ETL 10kx</span><strong>{current.etl10kx.toFixed(4)}</strong></div>
      <div><span class="dim">P(≥ 5k)</span><strong>{current.probWin5k.toExponential(3)}</strong></div>
      <div><span class="dim">P(≥ 10k)</span><strong>{current.probWin10k.toExponential(3)}</strong></div>
      <div><span class="dim">Unique payouts</span><strong>{current.uniquePayoutCount}</strong></div>
      <div><span class="dim">Most likely</span><strong>{fmtOneIn(current.mostLikelyOneIn)}</strong></div>
    </div>
    <p class="note">
      Tail metrics compare two math settings; they don't exactly match the
      aggregated game-level Tail/CVaR/ETL on the Stake dashboard.
    </p>
  {/if}
</section>

<style>
  .wal-head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    margin-bottom: 8px;
  }

  .wal-head .overline {
    margin: 0 6px 0 0;
  }

  .wal-head .pill {
    font-size: var(--fs-xs);
  }

  .wal-head .pill.zero {
    opacity: 0.4;
  }

  .wal-head button.pill:hover:not(:disabled) {
    background: color-mix(in oklch, currentColor var(--tint-strong), transparent);
    border-color: color-mix(in oklch, currentColor 50%, transparent);
  }

  .gap-lg {
    margin-top: 22px;
  }

  .gap-xl {
    margin-top: 26px;
  }

  .all-clear {
    color: var(--pass);
    font-size: var(--fs-base);
  }

  .wal-cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(460px, 1fr));
    gap: 12px;
    margin-top: 4px;
  }

  @media (max-width: 1000px) {
    .wal-cards {
      grid-template-columns: 1fr;
    }
  }

  .wal-card {
    --c: var(--sv-context);
    border: 1px solid var(--border);
    border-top: 3px solid var(--c);
    border-radius: var(--r-m);
    background: var(--bg-panel);
    overflow: hidden;
  }

  .wal-card.blocking { --c: var(--sv-blocking); }
  .wal-card.serious { --c: var(--sv-serious); }
  .wal-card.minor { --c: var(--sv-minor); }

  .wal-card header {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 10px 14px 8px;
  }

  .wal-card header strong {
    font-size: var(--fs-md);
    font-weight: 700;
    letter-spacing: 0.01em;
  }

  .cost {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    color: var(--text-faint);
  }

  .tally {
    margin-left: auto;
    display: flex;
    gap: 10px;
    font-size: var(--fs-xs);
  }

  .t.blocking { color: var(--sv-blocking); }
  .t.serious { color: var(--sv-serious); }
  .t.minor { color: var(--sv-minor); }

  .wal-card ul {
    list-style: none;
    margin: 0;
    padding: 0 0 4px;
  }

  .wal-card li {
    border-top: 1px solid var(--border);
  }

  .line {
    display: grid;
    grid-template-columns: 10px minmax(0, 1fr) auto;
    grid-template-areas: 'sev title big' '. title ref';
    column-gap: 10px;
    row-gap: 1px;
    align-items: baseline;
    width: 100%;
    padding: 8px 14px;
    text-align: left;
    background: none;
    border: none;
    border-radius: 0;
  }

  .line:hover {
    background: var(--bg-raised);
  }

  .sev {
    grid-area: sev;
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--sv-context);
    align-self: center;
  }

  li.blocking .sev { background: var(--sv-blocking); box-shadow: 0 0 7px var(--sv-blocking); }
  li.serious .sev { background: var(--sv-serious); }
  li.minor .sev { background: var(--sv-minor); }

  .title {
    grid-area: title;
    font-size: var(--fs-base);
    line-height: 1.4;
    color: var(--text);
  }

  li.minor .title,
  li.context .title {
    color: var(--text-dim);
  }

  .big {
    grid-area: big;
    justify-self: end;
    font-family: var(--font-mono);
    font-size: var(--fs-base);
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  li.blocking .big { color: var(--sv-blocking); }
  li.serious .big { color: var(--sv-serious); }
  li.minor .big { color: var(--sv-minor); }
  li.context .big { color: var(--sv-context); }

  .ref {
    grid-area: ref;
    justify-self: end;
    font-size: var(--fs-2xs);
    color: var(--text-faint);
    white-space: nowrap;
  }

  .more {
    padding: 2px 14px 10px 34px;
    font-size: var(--fs-sm);
  }

  .spec {
    margin: 0 0 6px;
    color: var(--text);
    line-height: 1.5;
  }

  .why {
    margin: 0 0 8px;
    color: var(--text-faint);
    line-height: 1.5;
  }

  .more-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .pending {
    font-size: var(--fs-sm);
    color: var(--text-faint);
    padding: 18px 0;
  }

  .clean {
    margin: 12px 0 0;
    font-size: var(--fs-sm);
    color: var(--pass);
  }

  button.tiny {
    padding: 1px 8px;
    font-size: var(--fs-xs);
    flex-shrink: 0;
  }

  .wrap {
    overflow-x: auto;
  }

  tbody tr {
    cursor: pointer;
  }

  tbody tr:hover td {
    background: var(--bg-raised);
  }

  .mode-link {
    padding: 2px 6px;
    margin-left: -6px;
    font-size: var(--fs-sm);
    color: var(--text);
  }

  .row-active .mode-link {
    color: var(--gold);
    font-weight: 640;
  }

  td .pill.tiny {
    margin-left: 4px;
  }

  .statline.hero {
    padding: 4px 0 18px;
  }

  .statline.hero strong {
    font-size: var(--fs-xl);
  }

  .statline.hero .gold {
    color: var(--gold);
  }

  .split {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 40px;
    align-items: start;
  }

  @media (max-width: 1000px) {
    .split {
      grid-template-columns: 1fr;
      gap: 24px;
    }
  }

  .breakdown-bar {
    display: flex;
    height: 22px;
    border-radius: var(--r-s);
    overflow: hidden;
    border: 1px solid var(--border);
  }

  .seg.dead { background: var(--border-strong); }
  .seg.subbet { background: var(--warn); }
  .seg.win { background: var(--pass); }

  .breakdown-legend {
    display: flex;
    gap: 18px;
    margin-top: 10px;
    font-size: var(--fs-sm);
    color: var(--text-dim);
  }

  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    margin-right: 4px;
  }

  .dot.dead { background: var(--border-strong); }
  .dot.subbet { background: var(--warn); }
  .dot.win { background: var(--pass); }

  .note {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    max-width: 780px;
    margin: 12px 0 0;
    line-height: 1.55;
  }
</style>
