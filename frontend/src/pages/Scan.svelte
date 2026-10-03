<script lang="ts">
  // Staging invariants, read from every book and weighted by the LUT.
  //
  // These are the rules nothing else in the toolbox can see: the RTP does not
  // move when an anticipation stops firing, the compliance report stays green,
  // and the game simply announces something that is not happening. The studio's
  // worst case cost weeks — an anticipation at 1 spin in 137 instead of 1 in
  // 16.6 — and was only found by re-reading the events of the published books.
  import {
    api,
    fmtPct,
    fmtOneIn,
    fmtX,
    DEFAULT_TIMINGS,
    type ScanReport,
    type ScanJob,
    type ScanParams,
    type ScanBucket,
  } from '../lib/api';
  import { app } from '../lib/stores.svelte';

  let reports = $state<ScanReport[]>([]);
  let job = $state<ScanJob | null>(null);
  let error = $state<string | null>(null);
  let loaded = $state(false);
  let starting = $state(false);
  let selectedMode = $state('');
  let showParams = $state(false);
  let params = $state<ScanParams>({
    triggerScatters: 0,
    retriggerScatters: 0,
    visibleRows: 0,
    maxBaseScatters: 0,
    timings: { ...DEFAULT_TIMINGS },
  });

  let timer: ReturnType<typeof setTimeout> | null = null;
  /** Bumped on every game switch/unmount: a poll answering for an older
   * game (or a page that is gone) neither writes nor re-arms its timer. */
  let seq = 0;

  $effect(() => {
    const name = app.selectedGame;
    const token = ++seq;
    // A new game starts clean: no stale error, no reports of the previous one.
    error = null;
    reports = [];
    job = null;
    loaded = false;
    if (name) void poll(name, token);
    return () => {
      seq++;
      if (timer) clearTimeout(timer);
      timer = null;
    };
  });

  async function poll(gameName: string, token: number) {
    try {
      const r = await api.scanStatus(gameName);
      if (token !== seq) return;
      reports = r.reports;
      job = r.job;
      loaded = true;
      if (!reports.some((x) => x.mode === selectedMode)) {
        selectedMode = reports[0]?.mode ?? '';
      }
      if (job?.state === 'running') {
        timer = setTimeout(() => poll(gameName, token), 1200);
      }
    } catch (e) {
      if (token !== seq) return;
      loaded = true;
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function startScan(force: boolean) {
    const name = app.selectedGame;
    if (!name) return;
    error = null;
    starting = true;
    try {
      await api.startScan(name, { params, force });
      job = { state: 'running', mode: '', booksDone: 0, booksTotal: 0, error: null };
      if (timer) clearTimeout(timer);
      void poll(name, seq);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      starting = false;
    }
  }

  const running = $derived(job?.state === 'running' || starting);

  const current = $derived(reports.find((r) => r.mode === selectedMode));
  const pct = $derived(
    job && job.booksTotal > 0
      ? Math.round((job.booksDone / job.booksTotal) * 100)
      : 0,
  );

  /** The four anticipation defects — a non-zero one is flagged in words,
   * not only in red (unless the mode anticipates by design). */
  const defects = $derived(
    current
      ? [
          { label: 'Missing', v: current.anticipation.missing },
          { label: 'Too early', v: current.anticipation.early },
          { label: 'Too late', v: current.anticipation.late },
          { label: 'Cannot trigger', v: current.anticipation.spurious },
        ].map((d) => ({ ...d, bad: d.v > 0 && !current.systematicAnticipation }))
      : [],
  );

  /** A distribution row, widest bar first — read as a shape, not a table. */
  function bars(list: ScanBucket[]): { value: number; p: number; w: number; avg: number }[] {
    const max = Math.max(...list.map((b) => b.probability), 1e-9);
    return list.map((b) => ({
      value: b.value,
      p: b.probability,
      w: (b.probability / max) * 100,
      avg: b.avgPayout,
    }));
  }
</script>

{#if error}<div class="banner error" role="alert">{error}</div>{/if}

{#if !app.selectedGame}
  <div class="panel">
    <p class="dim">Select a game to scan its books.</p>
  </div>
{:else}
<header class="verdict-bar">
  <div class="verdict-main">
    <h2 class="page-title">Book scan</h2>
    <span role="status" class="status">
      {#if job?.state === 'running'}
        <span class="pill yellow">
          {job.mode || 'scanning'} · {job.booksDone.toLocaleString('en-US')}
          {#if job.booksTotal}/ {job.booksTotal.toLocaleString('en-US')} ({pct}%){/if}
        </span>
      {:else if job?.state === 'failed'}
        <span class="pill red">Scan failed</span>
      {:else if reports.length > 0}
        <span class="pill green">{reports.length} mode{reports.length === 1 ? '' : 's'} scanned</span>
      {/if}
    </span>
  </div>
  <div class="verdict-actions">
    <button
      class="ghost"
      aria-expanded={showParams}
      aria-controls="scan-params"
      onclick={() => (showParams = !showParams)}
    >
      {showParams ? 'Hide parameters' : 'Parameters'}
    </button>
    {#if reports.length > 0}
      <button onclick={() => startScan(false)} disabled={running}>
        Scan missing modes
      </button>
      <button class="primary" onclick={() => startScan(true)} disabled={running}>
        Re-scan all
      </button>
    {/if}
  </div>
</header>

{#if job?.state === 'failed' && job.error}
  <div class="banner error" role="alert">Scan failed: {job.error}</div>
{/if}

{#if showParams}
  <section class="page params" id="scan-params">
    <p class="overline">Shape <span class="faint">0 = derive it from the books</span></p>
    <div class="param-grid">
      <label><span>Trigger scatters</span>
        <input type="number" min="0" bind:value={params.triggerScatters} /></label>
      <label><span>Re-trigger scatters</span>
        <input type="number" min="0" bind:value={params.retriggerScatters} /></label>
      <label><span>Visible rows</span>
        <input type="number" min="0" bind:value={params.visibleRows} /></label>
      <label><span>Max base scatters</span>
        <input type="number" min="0" bind:value={params.maxBaseScatters} /></label>
    </div>
    <p class="note">
      Leave the re-trigger at 0 for a game that does not announce re-triggers
      inside the bonus — the usual setting, because a tumbling slot shows a
      scatter on one board in four and the suspense would never stop.
    </p>
    <p class="overline">Staging seconds <span class="faint">frontend constants, normal speed</span></p>
    <div class="param-grid">
      <label><span>A board</span>
        <input type="number" step="0.05" bind:value={params.timings.reveal} /></label>
      <label><span>Anticipated reel</span>
        <input type="number" step="0.05" bind:value={params.timings.anticipationStep} /></label>
      <label><span>Cascade</span>
        <input type="number" step="0.05" bind:value={params.timings.tumble} /></label>
      <label><span>A win</span>
        <input type="number" step="0.05" bind:value={params.timings.win} /></label>
      <label><span>Bonus intro</span>
        <input type="number" step="0.1" bind:value={params.timings.bonusIntro} /></label>
      <label><span>Bonus outro</span>
        <input type="number" step="0.1" bind:value={params.timings.bonusOutro} /></label>
    </div>
  </section>
{/if}

{#if reports.length === 0}
  <div class="panel empty">
    {#if !loaded}
      <p class="dim" role="status">Loading…</p>
    {:else if job?.state === 'running'}
      <p class="dim">Reading every book of the mode…</p>
    {:else}
      <p class="dim">
        No scan yet for this build. A full pass reads every event of every book —
        about a minute for a big game, and the result is kept until the math changes.
      </p>
      <button class="primary" onclick={() => startScan(false)} disabled={running}>
        {starting ? 'Starting…' : 'Run the first scan (~1 min)'}
      </button>
    {/if}
  </div>
{:else}
  <div class="view-tabs" role="tablist" aria-label="Bet mode">
    {#each reports as r (r.mode)}
      <button
        role="tab"
        aria-selected={selectedMode === r.mode}
        class:active={selectedMode === r.mode}
        onclick={() => (selectedMode = r.mode)}
      >
        {r.mode}
        {#if r.warnings.length > 0}
          <span class="pill yellow tiny tab-count">
            {r.warnings.length}<span class="sr-only"> warning{r.warnings.length === 1 ? '' : 's'}</span>
          </span>
        {/if}
      </button>
    {/each}
    {#if current}
      <span class="view-context faint">
        {current.books.toLocaleString('en-US')} books · {current.shape.reels}×{current.shape.visibleRows}
        · trigger {current.shape.triggerScatters}
        {#if current.shape.pad > 0}· padding {current.shape.pad}{/if}
      </span>
    {/if}
  </div>

  {#if current}
    {#each current.warnings as w (w)}
      <div class="banner warn">{w}</div>
    {/each}

    <section class="page">
      <p class="overline">Anticipation</p>
      <div class="statline hero">
        <div><span class="dim">Fires</span><strong>{fmtOneIn(current.anticipation.emittedOneIn)}</strong></div>
        <div><span class="dim">Board could complete</span><strong>{fmtOneIn(current.anticipation.deservedOneIn)}</strong></div>
        {#each defects as d (d.label)}
          <div>
            <span class="dim">
              {d.label}
              {#if d.bad}<span class="pill red tiny">defect</span>{/if}
            </span>
            <strong class:bad={d.bad}>{fmtPct(d.v, 4)}</strong>
          </div>
        {/each}
      </div>
      {#if current.anticipation.byStartReel.length > 0}
        <p class="overline">Starts on reel</p>
        <div class="dist">
          {#each bars(current.anticipation.byStartReel) as b (b.value)}
            <div class="dist-row">
              <span class="dist-key">reel {b.value}</span>
              <span class="dist-bar"><i style="width: {b.w}%"></i></span>
              <span class="dist-val num">{fmtPct(b.p, 3)}</span>
            </div>
          {/each}
        </div>
      {/if}
      {#if current.systematicAnticipation}
        <p class="note">
          This mode anticipates on (almost) every spin — the bonus is bought,
          not discovered. Its reels announce a result that is already paid for,
          so the base-game rule does not apply here and the three counters
          above are context, not defects.
        </p>
      {/if}
      <p class="note">
        Measured against the rule itself: the announcement is owed by the reel
        that FOLLOWS the one completing trigger − 1 — except when the threshold
        lands on the last reel, where it is intra-column and only the rows above
        the scatter slow down. Anything else is counted as early, late or
        missing.
      </p>
    </section>

    <div class="split">
      <section class="page">
        <p class="overline">Scatters on a base board <span class="faint">visible window only</span></p>
        <div class="dist">
          {#each bars(current.scattersBase) as b (b.value)}
            <div class="dist-row">
              <span class="dist-key">{b.value}</span>
              <span class="dist-bar"><i style="width: {b.w}%"></i></span>
              <span class="dist-val num">{fmtPct(b.p, 3)}</span>
            </div>
          {/each}
        </div>
      </section>

      <section class="page">
        <p class="overline">
          Bonus tiers
          <span class="pill {current.tiersMonotonic ? 'green' : 'red'}">
            {current.tiersMonotonic ? 'monotonic' : 'NOT monotonic'}
          </span>
        </p>
        {#if current.bonusTiers.length === 0}
          <p class="dim">No bonus in this mode.</p>
        {:else}
          <table>
            <thead>
              <tr><th>Scatters</th><th class="num">P</th><th class="num">1 in</th><th class="num">Avg win</th></tr>
            </thead>
            <tbody>
              {#each current.bonusTiers as t (t.value)}
                <tr>
                  <td>{t.value}</td>
                  <td class="num">{fmtPct(t.probability, 4)}</td>
                  <td class="num faint">{fmtOneIn(t.oneIn)}</td>
                  <td class="num">{fmtX(t.avgPayout, 1)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
          <p class="note">
            A higher tier is never worth less than a lower one — the studio rule
            that no other check verifies.
          </p>
        {/if}
      </section>
    </div>

    <section class="page">
      <p class="overline">Pacing <span class="faint">staging seconds, celebrations excluded</span></p>
      <div class="statline hero">
        <div><span class="dim">Average round</span><strong>{current.pacing.mean.toFixed(2)} s</strong></div>
        <div><span class="dim">Median</span><strong>{current.pacing.p50.toFixed(1)} s</strong></div>
        <div><span class="dim">p95</span><strong>{current.pacing.p95.toFixed(1)} s</strong></div>
        <div><span class="dim">Wild moves / spin</span><strong>{current.wildMovesPerSpin.toFixed(2)}</strong></div>
      </div>
      <p class="note">
        Weighted by the LUT, with the timings in Parameters. Add the celebration
        seconds from Analyze → Win tiers for the whole cost of a round.
      </p>
    </section>

    <section class="page">
      <p class="overline">Events <span class="faint">share of rounds carrying one, and how many</span></p>
      <table>
        <thead>
          <tr><th>Event</th><th class="num">Rounds</th><th class="num">1 in</th><th class="num">Per round</th><th class="num">Avg win</th></tr>
        </thead>
        <tbody>
          {#each current.events as e (e.name)}
            <tr>
              <td><code>{e.name}</code></td>
              <td class="num">{fmtPct(e.probability, 3)}</td>
              <td class="num faint">{fmtOneIn(e.oneIn)}</td>
              <td class="num">{e.perSpin.toFixed(2)}</td>
              <td class="num faint">{fmtX(e.avgPayout, 1)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      <p class="note">
        Every one of these owes the player a feedback. Preflight cross-checks
        the list against the frontend sources — an event no file names is a
        moment the game plays in silence.
      </p>
    </section>

    {#if current.retriggers.length > 1}
      <section class="page">
        <p class="overline">Re-triggers per round</p>
        <div class="dist">
          {#each bars(current.retriggers) as b (b.value)}
            <div class="dist-row">
              <span class="dist-key">{b.value}</span>
              <span class="dist-bar"><i style="width: {b.w}%"></i></span>
              <span class="dist-val num">{fmtPct(b.p, 3)}</span>
            </div>
          {/each}
        </div>
      </section>
    {/if}
  {/if}
{/if}
{/if}

<style>
  .verdict-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--sp-2) var(--sp-4);
    padding: var(--sp-1) 0 var(--sp-4);
  }

  .verdict-main,
  .verdict-actions {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }

  .verdict-main {
    gap: var(--sp-3);
  }

  .page-title {
    font-size: var(--fs-lg);
    margin: 0;
  }

  .status:empty {
    display: none;
  }

  .tab-count {
    margin-left: var(--sp-1);
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--sp-3);
  }

  .empty p {
    margin: 0;
    max-width: 72ch;
  }

  .note {
    color: var(--text-faint);
    font-size: var(--fs-sm);
    max-width: 92ch;
  }

  .params .param-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: var(--sp-3) var(--sp-4);
    margin-bottom: var(--sp-3);
  }

  .param-grid label {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    font-size: var(--fs-xs);
    color: var(--text-dim);
  }

  .split {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(320px, 100%), 1fr));
    gap: 0 var(--sp-8);
  }

  .dist {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }

  .dist-row {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    font-size: var(--fs-xs);
  }

  .dist-key {
    width: 62px;
    color: var(--text-dim);
    text-align: right;
  }

  .dist-bar {
    flex: 1;
    height: 9px;
    background: var(--bg-raised);
    border-radius: 2px;
    overflow: hidden;
  }

  .dist-bar i {
    display: block;
    height: 100%;
    background: var(--gold);
  }

  .dist-val {
    width: 78px;
  }

  strong.bad {
    color: var(--fail);
  }

  .statline .pill {
    margin-left: var(--sp-1);
    text-transform: none;
    letter-spacing: 0.02em;
  }
</style>
