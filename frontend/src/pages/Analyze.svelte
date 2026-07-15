<script lang="ts">
  import {
    api,
    fmtPct,
    fmtX,
    fmtOneIn,
    bookmarksApi,
    type ModeStats,
    type GameCheckReport,
    type Check,
    type ModeCompliance,
    type ModeBonusStats,
  } from '../lib/api';
  import { app } from '../lib/stores.svelte';
  import Chart from '../components/Chart.svelte';

  type View = 'overview' | 'compliance' | 'distribution' | 'bonus' | 'risk';

  let stats = $state<{ mode: string; stats: ModeStats }[]>([]);
  let checks = $state<GameCheckReport | null>(null);
  let bonusStats = $state<ModeBonusStats[]>([]);
  let selectedMode = $state('');
  let view = $state<View>('overview');
  let error = $state<string | null>(null);
  let forcedMsg = $state<string | null>(null);

  $effect(() => {
    if (app.selectedGame) void load(app.selectedGame);
  });

  async function load(gameName: string) {
    error = null;
    try {
      const [s, c] = await Promise.all([
        api.gameStats(gameName),
        api.gameChecks(gameName),
      ]);
      stats = s.modes;
      checks = c;
      // Bonus stats need the books import — non-fatal if unavailable.
      try {
        bonusStats = (await api.bonusStats(gameName)).modes;
      } catch {
        bonusStats = [];
      }
      if (!stats.some((m) => m.mode === selectedMode)) {
        selectedMode = stats[0]?.mode ?? '';
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  const current = $derived(stats.find((m) => m.mode === selectedMode)?.stats);
  const currentChecks = $derived(checks?.modes.find((m) => m.mode === selectedMode));
  const currentBonus = $derived(bonusStats.find((m) => m.mode === selectedMode));
  const stdPerCost = $derived(current ? current.stdDev / Math.max(1, current.cost) : 0);

  /** Stake-style volatility label (LOW/MEDIUM/HIGH/EXTREME), from std/cost. */
  function volLabel(std: number): { label: string; cls: string } {
    if (std >= 32) return { label: 'EXTREME', cls: 'red' };
    if (std >= 16) return { label: 'HIGH', cls: 'yellow' };
    if (std >= 6) return { label: 'MEDIUM', cls: 'blue' };
    return { label: 'LOW', cls: 'green' };
  }

  function modeCompliance(mode: string): ModeCompliance | undefined {
    return checks?.modes.find((m) => m.mode === mode);
  }

  const breakdown = $derived.by(() => {
    if (!current) return null;
    const dead = current.zeroRate;
    const win = current.breakevenRate;
    const sub = Math.max(0, current.hitRate - win);
    return { dead, sub, win };
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

  function bucketLabel(b: { start: number; end: number }): string {
    if (b.start === 0 && b.end === 0) return '0';
    if (b.end > 1e8) return `${b.start}+`;
    return `${b.start}–${b.end}`;
  }

  /** Buckets that carry RTP (the "0" bucket carries none). */
  const rtpBuckets = $derived(current ? current.buckets.filter((b) => b.rtpContribution > 0) : []);

  /** Horizontal chart: "where the RTP comes from" — single axis, readable. */
  const distChart = $derived.by(() => {
    if (!current) return null;
    return {
      type: 'bar' as const,
      data: {
        labels: rtpBuckets.map(bucketLabel),
        datasets: [
          {
            label: 'RTP contribution',
            data: rtpBuckets.map((b) => b.rtpContribution * 100),
            backgroundColor: '#c9a227',
            borderRadius: 3,
          },
        ],
      },
      options: {
        indexAxis: 'y' as const,
        responsive: true,
        maintainAspectRatio: false,
        plugins: { legend: { display: false } },
        scales: {
          x: { title: { display: true, text: '% of RTP' }, grid: { color: 'rgba(120,140,128,0.08)' } },
          y: { grid: { display: false } },
        },
      },
    };
  });

  async function playBook(mode: string, simId: number) {
    if (!app.selectedGame) return;
    await api.setForce({ game: app.selectedGame, mode, simId });
    forcedMsg = `${mode} #${simId} forced — next spin in Play`;
    setTimeout(() => (forcedMsg = null), 4000);
  }

  async function bookmarkBook(mode: string, simId: number, payoutX100: number) {
    if (!app.selectedGame) return;
    await bookmarksApi.add({ game: app.selectedGame, mode, simId, payoutX100, description: 'from Analyze' });
    forcedMsg = `${mode} #${simId} bookmarked`;
    setTimeout(() => (forcedMsg = null), 3000);
  }

  function statusPill(status: Check['status']): string {
    return status === 'pass' ? 'green' : status === 'warn' ? 'yellow' : status === 'fail' ? 'red' : 'blue';
  }

  const views: { id: View; label: string }[] = [
    { id: 'overview', label: 'Overview' },
    { id: 'compliance', label: 'Compliance' },
    { id: 'distribution', label: 'Distribution' },
    { id: 'bonus', label: 'Bonus' },
    { id: 'risk', label: 'Risk' },
  ];
</script>

{#if error}
  <div class="banner error">{error}</div>
{:else if !current || !checks}
  <div class="panel"><p class="dim">Select a loaded game.</p></div>
{:else}
  {#if forcedMsg}<div class="banner ok">{forcedMsg}</div>{/if}

  <!-- 1. Compact verdict: the "is it OK?" answer at a glance -->
  <header class="verdict-bar">
    <div class="verdict-main">
      <span class="pill {checks.passed ? 'green' : 'red'} big">
        {checks.passed ? 'COMPLIANT' : `${checks.failCount} FAIL`}
      </span>
      <span class="stars" title="Estimated tier (computable constraints)">
        {'★'.repeat(checks.starTier)}<span class="stars-off">{'★'.repeat(3 - checks.starTier)}</span>
      </span>
      {#if checks.warnCount > 0}<span class="pill yellow">{checks.warnCount} warn</span>{/if}
    </div>
    <div class="verdict-meta faint">
      {stats.length} modes · RTP variance {fmtPct(checks.crossModeVariance, 2)}
    </div>
  </header>

  <!-- 2. Mode grid: scannable, selects on click -->
  <div class="mode-grid">
    {#each stats as m (m.mode)}
      {@const mc = modeCompliance(m.mode)}
      {@const vol = volLabel(m.stats.stdDev / Math.max(1, m.stats.cost))}
      <button
        class="mode-card"
        class:active={selectedMode === m.mode}
        onclick={() => (selectedMode = m.mode)}
      >
        <div class="mc-top">
          <span class="mc-name">{m.mode}</span>
          <span class="mc-cost">{m.stats.cost}x</span>
          {#if mc}
            <span class="mc-dot {mc.passed === mc.total ? 'ok' : 'bad'}"
              title="{mc.passed}/{mc.total} checks"></span>
          {/if}
        </div>
        <div class="mc-vol"><span class="pill {vol.cls}">{vol.label}</span></div>
        <div class="mc-stats">
          <span><i>RTP</i>{fmtPct(m.stats.rtp, 2)}</span>
          <span><i>Hit</i>{fmtPct(m.stats.hitRate, 1)}</span>
          <span><i>Max</i>{fmtX(m.stats.maxPayout, 0)}</span>
        </div>
      </button>
    {/each}
  </div>

  <!-- 3. View selector: one lens at a time -->
  <nav class="view-tabs">
    {#each views as v (v.id)}
      <button class:active={view === v.id} onclick={() => (view = v.id)}>{v.label}</button>
    {/each}
    <span class="view-context faint">{selectedMode}</span>
  </nav>

  {#if view === 'overview'}
    <section class="page">
      <div class="statline hero">
        <div><span class="dim">RTP</span><strong class="gold">{fmtPct(current.rtp, 4)}</strong></div>
        <div><span class="dim">Hit rate</span><strong>{fmtPct(current.hitRate)}</strong></div>
        <div><span class="dim">Std Dev (÷cost)</span><strong>{stdPerCost.toFixed(2)}</strong></div>
        <div><span class="dim">Max win</span><strong>{fmtX(current.maxPayout, 0)}</strong></div>
        <div><span class="dim">Max win HR</span><strong>{fmtOneIn(current.maxPayoutOneIn)}</strong></div>
        <div><span class="dim">P &lt; bet</span><strong>{fmtPct(1 - current.breakevenRate)}</strong></div>
      </div>

      {#if breakdown && streaks}
        <div class="split">
          <div>
            <p class="overline">Outcome breakdown</p>
            <div class="breakdown-bar">
              <span class="seg dead" style="width: {breakdown.dead * 100}%"></span>
              <span class="seg subbet" style="width: {breakdown.sub * 100}%"></span>
              <span class="seg win" style="width: {breakdown.win * 100}%"></span>
            </div>
            <div class="breakdown-legend">
              <span><i class="dot dead"></i> Dead {fmtPct(breakdown.dead)}</span>
              <span><i class="dot subbet"></i> Sub-bet {fmtPct(breakdown.sub)}</span>
              <span><i class="dot win"></i> Win {fmtPct(breakdown.win)}</span>
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
      {/if}
    </section>
  {:else if view === 'compliance'}
    <section class="page">
      <p class="overline">
        Compliance per mode — {selectedMode}
        {#if currentChecks}<span class="faint">({currentChecks.passed}/{currentChecks.total})</span>{/if}
      </p>
      {#if currentChecks}
        <table>
          <tbody>
            {#each currentChecks.checks as c (c.id)}
              <tr>
                <td>{c.label}</td>
                <td class="num faint">{c.threshold}</td>
                <td class="num">{c.value}</td>
                <td><span class="pill {statusPill(c.status)}">{c.status}</span></td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

      <p class="overline" style="margin-top: 24px;">Overall Bet Level Compliance</p>
      <table class="tier-table">
        <thead>
          <tr>
            <th>Constraint</th><th class="num">Value</th>
            <th class="num">2★</th><th></th><th class="num">3★</th><th></th>
          </tr>
        </thead>
        <tbody>
          {#each checks.betLevelConstraints as c (c.label)}
            <tr>
              <td>{c.label}</td>
              <td class="num">{c.value}</td>
              <td class="num faint">{c.limit2star}</td>
              <td><span class="pill {c.pass2star ? 'green' : 'red'}">{c.pass2star ? '✓' : '✕'}</span></td>
              <td class="num faint">{c.limit3star}</td>
              <td><span class="pill {c.pass3star ? 'green' : 'red'}">{c.pass3star ? '✓' : '✕'}</span></td>
            </tr>
          {/each}
        </tbody>
      </table>
      <p class="note">
        Tail Probability, CVaR and ETL are a proprietary Stake aggregate not
        replicated by SlotLab — see the <em>Risk</em> tab (indicative) and
        confirm on the dashboard.
      </p>

      {#if checks.publishChecks.length > 0}
        <p class="overline" style="margin-top: 24px;">Publish checks (RGS)</p>
        <table>
          <tbody>
            {#each checks.publishChecks as c (c.id)}
              <tr>
                <td>{c.label}</td>
                <td class="num">{c.value}</td>
                <td><span class="pill {statusPill(c.status)}">{c.status}</span></td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </section>
  {:else if view === 'distribution'}
    <section class="page">
      <p class="overline">Where the RTP comes from — {selectedMode}</p>
      <p class="sub">
        Each bar = the share of the {fmtPct(current.rtp, 2)} RTP contributed by
        a win band (in bet multiples).
      </p>
      {#if distChart}<Chart config={distChart} height={Math.max(220, rtpBuckets.length * 26)} />{/if}

      <p class="overline" style="margin-top: 26px;">All bands</p>
      <table class="dist-table">
        <thead>
          <tr>
            <th>Band (x bet)</th>
            <th class="num">Frequency</th>
            <th class="num">Probability</th>
            <th class="num">Books</th>
            <th class="num">RTP</th>
          </tr>
        </thead>
        <tbody>
          {#each current.buckets as b (bucketLabel(b))}
            <tr>
              <td>{bucketLabel(b)}</td>
              <td class="num">{b.start === 0 && b.end === 0 ? '—' : fmtOneIn(b.oneIn)}</td>
              <td class="num">{fmtPct(b.probability, 3)}</td>
              <td class="num">{b.count.toLocaleString('en-US')}</td>
              <td class="num rtp-cell">
                {fmtPct(b.rtpContribution, 2)}
                <span class="rtp-track"><span class="rtp-fill" style="width: {b.rtpContribution / current.rtp * 100}%"></span></span>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>

      <p class="overline" style="margin-top: 26px;">Top payouts</p>
      <table>
        <thead>
          <tr><th class="num">Win</th><th class="num">Frequency</th><th class="num">Books</th><th>Replay an event</th></tr>
        </thead>
        <tbody>
          {#each current.topPayouts as tp (tp.payout)}
            <tr>
              <td class="num">{fmtX(tp.payout, 2)}</td>
              <td class="num">{fmtOneIn(tp.oneIn)}</td>
              <td class="num">{tp.count}</td>
              <td>
                {#each tp.simIds.slice(0, 4) as sid (sid)}
                  <span class="sim-actions">#{sid}
                    <button title="Play this book" onclick={() => playBook(selectedMode, sid)}>▶</button>
                    <button title="Bookmark" onclick={() => bookmarkBook(selectedMode, sid, Math.round(tp.payout * 100))}>★</button>
                  </span>
                {/each}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </section>
  {:else if view === 'bonus'}
    <section class="page">
      {#if bonusStats.length === 0}
        <p class="dim">
          Bonus stats need the imported books — the import runs
          automatically (progress shown in Play), then come back here.
        </p>
      {:else}
        {#if currentBonus?.imported}
          <p class="overline">Bonus trigger — {selectedMode}</p>
          <div class="statline hero">
            <div><span class="dim">Trigger chance</span><strong class="gold">{fmtPct(currentBonus.triggerProbability ?? 0, 3)}</strong></div>
            <div><span class="dim">Frequency</span><strong>{fmtOneIn(currentBonus.triggerOneIn ?? Infinity)}</strong></div>
            <div><span class="dim">Avg win when triggered</span><strong>{fmtX(currentBonus.avgTriggerWin ?? 0, 1)}</strong></div>
            <div><span class="dim">Share of RTP</span><strong>{fmtPct(currentBonus.triggerRtpShare ?? 0, 1)}</strong></div>
            <div><span class="dim">Books with trigger</span><strong>{(currentBonus.booksWithTrigger ?? 0).toLocaleString('en-US')} <span class="faint">/ {(currentBonus.totalBooks ?? 0).toLocaleString('en-US')}</span></strong></div>
          </div>
          <p class="sub">
            Per-spin probabilities weighted by the lookup table — the odds a
            player actually experiences, unlike the raw book counts shown in
            Search. “Avg win when triggered” and “Share of RTP” cover the
            whole book (base win included), in bet multiples.
          </p>
        {:else}
          <p class="overline">Bonus trigger — {selectedMode}</p>
          <p class="dim">Books not imported for this mode.</p>
        {/if}

        <p class="overline" style="margin-top: 24px;">All modes compared</p>
        <table>
          <thead>
            <tr>
              <th>Mode</th>
              <th class="num">Cost</th>
              <th class="num">Trigger chance</th>
              <th class="num">Frequency</th>
              <th class="num">Avg win</th>
              <th class="num">RTP share</th>
              <th class="num">Books w/ trigger</th>
            </tr>
          </thead>
          <tbody>
            {#each bonusStats as b (b.mode)}
              <tr class:row-active={b.mode === selectedMode}>
                <td>{b.mode}</td>
                {#if b.imported}
                  <td class="num faint">{b.cost}x</td>
                  <td class="num">{fmtPct(b.triggerProbability ?? 0, 3)}</td>
                  <td class="num">{fmtOneIn(b.triggerOneIn ?? Infinity)}</td>
                  <td class="num">{fmtX(b.avgTriggerWin ?? 0, 1)}</td>
                  <td class="num">{fmtPct(b.triggerRtpShare ?? 0, 1)}</td>
                  <td class="num faint">{(b.booksWithTrigger ?? 0).toLocaleString('en-US')} / {(b.totalBooks ?? 0).toLocaleString('en-US')}</td>
                {:else}
                  <td colspan="6" class="faint">books not imported</td>
                {/if}
              </tr>
            {/each}
          </tbody>
        </table>

        {#if currentBonus?.imported}
          {#if (currentBonus.byTriggerCount?.length ?? 0) > 0}
            <p class="overline" style="margin-top: 26px;">Triggers per book — {selectedMode}</p>
            <table>
              <thead>
                <tr>
                  <th>Triggers</th>
                  <th class="num">Books</th>
                  <th class="num">Probability</th>
                  <th class="num">Frequency</th>
                  <th class="num">Avg win</th>
                </tr>
              </thead>
              <tbody>
                {#each currentBonus.byTriggerCount ?? [] as tc (tc.triggers)}
                  <tr>
                    <td>{tc.triggers}×</td>
                    <td class="num">{tc.count.toLocaleString('en-US')}</td>
                    <td class="num">{fmtPct(tc.probability, 4)}</td>
                    <td class="num">{fmtOneIn(tc.oneIn)}</td>
                    <td class="num">{fmtX(tc.avgWin, 1)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
            <p class="note">
              2×+ rows are books where the bonus (re)triggers more than once —
              a proxy for retrigger odds.
            </p>
          {:else}
            <p class="note" style="margin-top: 20px;">
              No book in {selectedMode} contains a free-spin trigger event.
            </p>
          {/if}

          {#if (currentBonus.byCriteria?.length ?? 0) > 1 || ((currentBonus.byCriteria?.length ?? 0) === 1 && currentBonus.byCriteria?.[0]?.criteria !== '(none)')}
            <p class="overline" style="margin-top: 26px;">Outcome types — {selectedMode}</p>
            <table>
              <thead>
                <tr>
                  <th>Criteria</th>
                  <th class="num">Probability</th>
                  <th class="num">Frequency</th>
                  <th class="num">Avg win</th>
                  <th class="num">RTP contribution</th>
                  <th class="num">Books (w/ trigger)</th>
                </tr>
              </thead>
              <tbody>
                {#each currentBonus.byCriteria ?? [] as c (c.criteria)}
                  <tr>
                    <td>{c.criteria}</td>
                    <td class="num">{fmtPct(c.probability, 3)}</td>
                    <td class="num">{fmtOneIn(c.oneIn)}</td>
                    <td class="num">{fmtX(c.avgWin, 1)}</td>
                    <td class="num">{fmtPct(c.rtpContribution, 2)}</td>
                    <td class="num faint">{c.count.toLocaleString('en-US')} ({c.booksWithTrigger.toLocaleString('en-US')})</td>
                  </tr>
                {/each}
              </tbody>
            </table>
            <p class="note">
              Criteria are the outcome labels written by the math-sdk
              (e.g. basegame / freegame / wincap) — they tell you WHICH kind
              of result a spin resolves to, and how likely each one is.
            </p>
          {/if}
        {/if}
      {/if}
    </section>
  {:else if view === 'risk'}
    <section class="page">
      <p class="overline">Risk analysis — {selectedMode} <span class="faint">(SlotLab estimate)</span></p>
      <div class="statline hero">
        <div><span class="dim">CVaR 0.1%</span><strong>{fmtX(current.cvar001, 0)}</strong></div>
        <div><span class="dim">ETL 40x</span><strong>{current.etl40x.toFixed(4)}</strong></div>
        <div><span class="dim">ETL 10kx</span><strong>{current.etl10kx.toFixed(4)}</strong></div>
        <div><span class="dim">Volatility CV</span><strong>{current.volatility.toFixed(2)}</strong></div>
      </div>
      <div class="statline">
        <div><span class="dim">P(≥5k)</span><strong>{current.probWin5k.toExponential(3)}</strong></div>
        <div><span class="dim">P(≥10k)</span><strong>{current.probWin10k.toExponential(3)}</strong></div>
        <div><span class="dim">Mean</span><strong>{fmtX(current.meanPayout, 4)}</strong></div>
        <div><span class="dim">Median</span><strong>{fmtX(current.medianPayout)}</strong></div>
        <div><span class="dim">Unique payouts</span><strong>{current.uniquePayoutCount}</strong></div>
        <div><span class="dim">Most likely</span><strong>{fmtOneIn(current.mostLikelyOneIn)}</strong></div>
      </div>
      <p class="note">
        These tail metrics help compare two math settings, but don't exactly
        match the aggregated values on the Stake dashboard
        (game-level Tail/CVaR/ETL). See CrowdSim for the player experience.
      </p>
    </section>
  {/if}
{/if}

<style>
  .verdict-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 0 16px;
  }

  .verdict-main {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .pill.big {
    font-size: 12.5px;
    padding: 3px 12px 3px 10px;
  }

  .stars {
    color: var(--gold);
    font-size: 17px;
    letter-spacing: 2px;
  }

  .stars-off {
    color: var(--border-strong);
  }

  .verdict-meta {
    font-size: 12px;
  }

  /* Mode grid — scannable cards */
  .mode-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(158px, 1fr));
    gap: 8px;
    margin-bottom: 22px;
  }

  .mode-card {
    display: flex;
    flex-direction: column;
    gap: 7px;
    text-align: left;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    background: var(--bg-panel);
  }

  .mode-card:hover {
    border-color: var(--border-strong);
    background: var(--bg-raised);
  }

  .mode-card.active {
    border-color: var(--gold);
    background: oklch(0.82 0.13 88 / 0.07);
    box-shadow: inset 3px 0 0 var(--gold);
  }

  .mc-top {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .mc-name {
    font-weight: 680;
    font-size: 13.5px;
  }

  .mc-cost {
    color: var(--text-faint);
    font-size: 11px;
    font-family: var(--font-mono);
  }

  .mc-dot {
    margin-left: auto;
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .mc-dot.ok { background: var(--pass); box-shadow: 0 0 6px var(--pass); }
  .mc-dot.bad { background: var(--fail); box-shadow: 0 0 6px var(--fail); }

  .mc-vol .pill {
    font-size: 9.5px;
    padding: 0 7px 0 6px;
  }

  .mc-stats {
    display: flex;
    justify-content: space-between;
    gap: 4px;
  }

  .mc-stats span {
    display: flex;
    flex-direction: column;
    gap: 1px;
    font-family: var(--font-mono);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  .mc-stats i {
    font-style: normal;
    font-family: var(--font-body);
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-faint);
  }

  /* View selector */
  .view-tabs {
    display: flex;
    align-items: center;
    gap: 2px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 20px;
  }

  .view-tabs button {
    border: none;
    border-radius: 0;
    background: transparent;
    color: var(--text-dim);
    padding: 8px 16px;
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
  }

  .view-tabs button:hover {
    color: var(--text);
    background: transparent;
  }

  .view-tabs button.active {
    color: var(--gold);
    border-bottom-color: var(--gold);
    font-weight: 640;
  }

  .view-context {
    margin-left: auto;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    font-family: var(--font-mono);
  }

  .statline.hero {
    padding: 4px 0 18px;
  }

  .statline.hero strong {
    font-size: 20px;
  }

  .statline.hero .gold {
    color: var(--gold);
  }

  .split {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 40px;
    align-items: start;
    padding-top: 6px;
  }

  @media (max-width: 1000px) {
    .split {
      grid-template-columns: 1fr;
      gap: 24px;
    }
  }

  .sub {
    font-size: 12px;
    color: var(--text-dim);
    margin: -4px 0 14px;
    max-width: 620px;
  }

  /* Inline RTP bar in the distribution table */
  .rtp-cell {
    position: relative;
    min-width: 120px;
  }

  .rtp-track {
    display: block;
    height: 3px;
    margin-top: 3px;
    background: oklch(0.26 0.015 165);
    border-radius: 2px;
    overflow: hidden;
  }

  .rtp-fill {
    display: block;
    height: 100%;
    background: var(--gold);
    border-radius: 2px;
  }

  .dist-table td {
    vertical-align: middle;
  }

  .breakdown-bar {
    display: flex;
    height: 22px;
    border-radius: 5px;
    overflow: hidden;
    border: 1px solid var(--border);
  }

  .seg.dead { background: oklch(0.4 0.02 165); }
  .seg.subbet { background: var(--warn); }
  .seg.win { background: var(--pass); }

  .breakdown-legend {
    display: flex;
    gap: 18px;
    margin-top: 10px;
    font-size: 12px;
    color: var(--text-dim);
  }

  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    margin-right: 4px;
  }

  .dot.dead { background: oklch(0.4 0.02 165); }
  .dot.subbet { background: var(--warn); }
  .dot.win { background: var(--pass); }

  .tier-table td:first-child { color: var(--text); }

  .row-active td:first-child {
    color: var(--gold);
    font-weight: 640;
  }

  .note {
    font-size: 11.5px;
    color: var(--text-faint);
    max-width: 780px;
    margin: 12px 0 0;
    line-height: 1.55;
  }

  .sim-actions {
    display: inline-flex;
    gap: 2px;
    align-items: center;
    margin-right: 8px;
    font-family: var(--font-mono);
    font-size: 12px;
  }

  .sim-actions button {
    padding: 0 5px;
    font-size: 10px;
  }
</style>
