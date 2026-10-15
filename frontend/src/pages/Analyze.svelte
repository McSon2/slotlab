<script lang="ts">
  import {
    api,
    fmtPct,
    fmtX,
    bookmarksApi,
    type ModeStats,
    type GameCheckReport,
    type ModeBonusStats,
    type ModeTiers,
    type TierSpec,
    type GameVersion,
  } from '../lib/api';
  import type { GameMarket } from '../lib/market';
  import { app, goTo, run, errorText, notify } from '../lib/stores.svelte';
  import { storageGetJson, storageSetJson } from '../lib/storage';
  import { fx, oneIn, refId, type ModePopulations, type PopRef } from '../lib/populations';
  import { allFindings, type Target } from '../lib/findings';
  import Overview from '../components/analyze/Overview.svelte';
  import Explore from '../components/analyze/Explore.svelte';
  import Compare from '../components/analyze/Compare.svelte';
  import Compliance from '../components/analyze/Compliance.svelte';
  import WinTiers from '../components/analyze/WinTiers.svelte';
  import Market from '../components/analyze/Market.svelte';
  import Mechanics from '../components/analyze/Mechanics.svelte';
  import SessionReach from '../components/analyze/SessionReach.svelte';

  /**
   * Analyze — one question per view:
   *   Overview    is the game sound? findings, every mode, the selected mode's spins;
   *   Explore     what does a population really pay? (mode, base spins, each
   *               bonus, tier, variant, criteria — distribution, not just averages);
   *   Compare     how do populations differ? (any of them, any mode, side by side);
   *   Mechanics   what do the events show the player? (every field, weighted by the LUT);
   *   Market      where does each mode stand among the slots the market plays?
   *   Compliance · Win tiers — unchanged.
   */
  type View = 'overview' | 'explore' | 'mechanics' | 'compare' | 'compliance' | 'tiers' | 'market';

  let stats = $state<{ mode: string; stats: ModeStats }[]>([]);
  let checks = $state<GameCheckReport | null>(null);
  let pops = $state<ModePopulations[]>([]);
  let bonusStats = $state<ModeBonusStats[]>([]);
  let tiers = $state<{ report: ModeTiers; spec: TierSpec }[]>([]);
  let ladderSource = $state('');
  let selectedMode = $state('');
  /** Selected population, per mode — switching modes keeps each one's place. */
  let selectedKey = $state<Record<string, string>>({});
  let view = $state<View>('overview');
  let error = $state<string | null>(null);
  let popsError = $state<string | null>(null);
  let bonusError = $state<string | null>(null);
  let tiersError = $state<string | null>(null);
  let market = $state<GameMarket | null>(null);
  let marketError = $state<string | null>(null);
  let marketLoading = $state(false);
  /** Which build the panels show; polled so a `run.py prod` shows up by itself. */
  let version = $state<GameVersion | null>(null);
  let reloading = $state(false);
  /** Stats and checks of the selected game on their way. */
  let loading = $state(false);
  let pinned = $state<PopRef[]>([]);
  let customThresholds = $state<number[]>([]);
  /**
   * 3★ grid: a rule (`strict`) or a reference with Stake's agreement to ship past it
   * (`lenient` — Degen Lab, 2026-10-05). Defaults from the game's folder, kept per game.
   */
  let complianceChoice = $state<'strict' | 'lenient' | null>(null);
  /** Degen Lab ships past the 3★ grid with Stake's agreement: its folder says so. */
  const degenLab = $derived(/degen[-_ ]?lab/i.test(app.games.find((g) => g.name === app.selectedGame)?.publishDir ?? ''));
  const compliance = $derived(complianceChoice ?? (degenLab ? 'lenient' : 'strict'));
  /** Populations and celebration ladders read (or failed): findings are complete. */
  let analysisReady = $state(false);
  let loadedGame = '';

  $effect(() => {
    if (app.selectedGame) void load(app.selectedGame);
  });

  // ── Per-game conveniences (compare set, extra thresholds) ──
  function storageKey(game: string) {
    return `slotlab.analyze.${game}`;
  }

  function restore(game: string) {
    const v = storageGetJson<{ pinned?: unknown; thresholds?: unknown; compliance?: unknown }>(storageKey(game), {});
    pinned = Array.isArray(v?.pinned) ? (v.pinned as PopRef[]) : [];
    customThresholds = Array.isArray(v?.thresholds)
      ? v.thresholds.filter((x: unknown): x is number => typeof x === 'number')
      : [];
    complianceChoice = v?.compliance === 'strict' || v?.compliance === 'lenient' ? v.compliance : null;
  }

  function persist() {
    if (!loadedGame) return;
    storageSetJson(storageKey(loadedGame), { pinned, thresholds: customThresholds, compliance: complianceChoice });
  }

  function toggleCompliance() {
    complianceChoice = compliance === 'strict' ? 'lenient' : 'strict';
    persist();
  }

  async function load(gameName: string) {
    error = null;
    if (gameName !== loadedGame) {
      analysisReady = false;
      loadedGame = gameName;
      restore(gameName);
      selectedKey = {};
      // Never show the previous game's numbers under the new one's name.
      stats = [];
      checks = null;
      pops = [];
      bonusStats = [];
      tiers = [];
      market = null;
      version = null;
      popsError = bonusError = tiersError = marketError = null;
    }
    loading = true;
    // Every source at once: they are independent, and chained they cost one
    // round trip each (the server runs them side by side).
    const extras = Promise.all([loadPops(gameName), loadBonus(gameName), loadTiers(gameName), loadMarket(gameName)]);
    try {
      const [s, c] = await Promise.all([api.gameStats(gameName), api.gameChecks(gameName)]);
      if (gameName !== loadedGame) return;
      stats = s.modes;
      checks = c;
      if (!stats.some((m) => m.mode === selectedMode)) {
        selectedMode = stats[0]?.mode ?? '';
      }
    } catch (e) {
      if (gameName === loadedGame) error = errorText(e);
      return;
    } finally {
      if (gameName === loadedGame) loading = false;
    }
    await extras;
    if (gameName !== loadedGame) return;
    analysisReady = true;
  }

  /** Only for « books that open 2+ bonuses » — non-fatal, but said. */
  async function loadBonus(gameName: string) {
    try {
      const b = (await api.bonusStats(gameName)).modes;
      if (gameName !== loadedGame) return;
      bonusStats = b;
      bonusError = null;
    } catch (e) {
      if (gameName !== loadedGame) return;
      bonusStats = [];
      bonusError = errorText(e);
    }
  }

  async function loadTiers(gameName: string) {
    try {
      const t = await api.gameTiers(gameName);
      if (gameName !== loadedGame) return;
      tiers = t.modes;
      ladderSource = t.ladderSource;
      tiersError = null;
    } catch (e) {
      if (gameName !== loadedGame) return;
      tiers = [];
      tiersError = errorText(e);
    }
  }

  async function loadMarket(gameName: string) {
    marketLoading = true;
    try {
      const m = await api.market(gameName);
      if (gameName !== loadedGame) return;
      market = m;
      marketError = null;
    } catch (e) {
      if (gameName !== loadedGame) return;
      market = null;
      marketError = errorText(e);
    } finally {
      if (gameName === loadedGame) marketLoading = false;
    }
  }

  /**
   * The build on screen vs the files on disk. SlotLab reloads a changed
   * publish folder by itself (server watcher); here the page notices the new
   * fingerprint and reads everything again — no hour spent on figures of a
   * version already replaced (WILD CURRENT, 2026-10-05).
   */
  $effect(() => {
    const game = app.selectedGame;
    if (!game) return;
    let stop = false;
    const tick = async () => {
      try {
        const v = await api.gameVersion(game);
        if (stop || app.selectedGame !== game) return;
        const previous = version;
        version = v;
        if (previous && previous.game === v.game && previous.fingerprint !== v.fingerprint && !reloading) {
          reloading = true;
          notify(`${game}: new math build read — refreshing the analysis`, 'info', 6000);
          loadedGame = '';
          await load(game);
          reloading = false;
        }
      } catch {
        /* the next tick retries */
      }
    };
    void tick();
    const id = setInterval(() => void tick(), 10_000);
    return () => {
      stop = true;
      clearInterval(id);
    };
  });

  async function loadPops(gameName: string) {
    try {
      const p = (await api.populations(gameName, customThresholds)).modes;
      if (gameName !== loadedGame) return;
      pops = p;
      popsError = null;
    } catch (e) {
      if (gameName !== loadedGame) return;
      pops = [];
      popsError = errorText(e);
    }
  }

  /**
   * Books still importing (first launch, or after a math change): bonuses,
   * tiers and criteria come from them. Re-read until every mode is in.
   */
  $effect(() => {
    const game = app.selectedGame;
    if (!game || !pops.length || pops.every((m) => m.imported)) return;
    const t = setTimeout(() => {
      if (app.selectedGame === game) void loadPops(game);
    }, 5000);
    return () => clearTimeout(t);
  });

  const current = $derived(stats.find((m) => m.mode === selectedMode)?.stats);
  /** Everything worth a look, over every source SlotLab has for the game. */
  const issues = $derived(allFindings({ stats, checks, pops, tiers, popsError, market, lenient: compliance === 'lenient' }));
  const currentMarket = $derived(market?.modes.find((m) => m.mode === selectedMode));
  const fmtTime = (ms: number) => new Date(ms).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
  const nBlocking = $derived(issues.filter((i) => i.severity === 'blocking').length);
  const nSerious = $derived(issues.filter((i) => i.severity === 'serious').length);
  const nMinor = $derived(issues.filter((i) => i.severity === 'minor').length);
  const pinnedIds = $derived(new Set(pinned.map(refId)));

  const walText = $derived.by(() => {
    if (!analysisReady) return 'Analysing…';
    if (nBlocking + nSerious + nMinor === 0) return 'Nothing worth a look';
    return `${nBlocking ? `${nBlocking} blocking · ` : ''}${nSerious} serious · ${nMinor} minor`;
  });

  /** 3★ reached, yet some bet-level constraint misses its 2★ limit (why Compliance shows ✕ under 2★). */
  const not2Star = $derived(
    checks && checks.starTier === 3
      ? checks.betLevelConstraints.filter((c) => !c.pass2star && c.pass3star).map((c) => c.label)
      : [],
  );

  /** Books of the selected mode that open the bonus more than once. */
  const multiTrigger = $derived.by(() => {
    const b = bonusStats.find((m) => m.mode === selectedMode);
    const rows = b?.byTriggerCount ?? [];
    const multi = rows.filter((r) => r.triggers > 1);
    const total = b?.triggerProbability ?? 0;
    if (!multi.length || total <= 0) return null;
    return {
      share: multi.reduce((a, r) => a + r.probability, 0) / total,
      books: multi.reduce((a, r) => a + r.count, 0),
    };
  });

  function bonusFreq(mode: string): string | null {
    const mp = pops.find((m) => m.mode === mode);
    const b = mp?.populations.find((p) => p.kind === 'bonus');
    if (b) return oneIn(b.oneIn);
    return mp?.populations.some((p) => p.kind === 'tier') ? 'every spin' : null;
  }

  // ── Navigation ──
  function select(ref: PopRef) {
    selectedMode = ref.mode;
    selectedKey = { ...selectedKey, [ref.mode]: ref.key };
  }

  function open(ref: PopRef) {
    select(ref);
    view = 'explore';
  }

  function openTarget(t: Target) {
    if (t.view === 'explore' && t.mode) {
      open({ mode: t.mode, key: t.key ?? 'all' });
      return;
    }
    if (t.mode) selectedMode = t.mode;
    view = t.view;
  }

  async function reloadNow() {
    const game = app.selectedGame;
    if (!game) return;
    reloading = true;
    await run(() => api.reloadGame(game), { error: 'Could not reload' });
    loadedGame = '';
    await load(game);
    reloading = false;
  }

  function togglePin(ref: PopRef) {
    const id = refId(ref);
    pinned = pinnedIds.has(id) ? pinned.filter((r) => refId(r) !== id) : [...pinned, ref];
    persist();
  }

  function pinMany(refs: PopRef[]) {
    const have = new Set(pinned.map(refId));
    pinned = [...pinned, ...refs.filter((r) => !have.has(refId(r)))];
    persist();
    view = 'compare';
  }

  function unpin(ref: PopRef) {
    pinned = pinned.filter((r) => refId(r) !== refId(ref));
    persist();
  }

  async function setThresholds(next: number[]) {
    customThresholds = [...new Set(next)].sort((a, b) => a - b);
    persist();
    if (app.selectedGame) await loadPops(app.selectedGame);
  }

  async function playBook(mode: string, simId: number) {
    const game = app.selectedGame;
    if (!game) return;
    await run(() => api.setForce({ game, mode, simId }), {
      ok: `${mode} #${simId} forced — open Play to spin it`,
      error: `Could not force ${mode} #${simId}`,
    });
  }

  async function bookmarkBook(mode: string, simId: number, payoutX100: number) {
    const game = app.selectedGame;
    if (!game) return;
    const added = await run(
      () => bookmarksApi.add({ game, mode, simId, payoutX100, description: 'from Analyze' }),
      { ok: `${mode} #${simId} bookmarked`, error: `Could not bookmark ${mode} #${simId}` },
    );
    if (added !== undefined) window.dispatchEvent(new CustomEvent('slotlab:bookmarks-changed'));
  }

  const views: { id: View; label: string }[] = [
    { id: 'overview', label: 'Overview' },
    { id: 'explore', label: 'Explore' },
    { id: 'mechanics', label: 'Mechanics' },
    { id: 'compare', label: 'Compare' },
    { id: 'compliance', label: 'Compliance' },
    { id: 'tiers', label: 'Win tiers' },
    { id: 'market', label: 'Market' },
  ];

  /** Tabs pattern: arrows / Home / End move between the views. */
  function onTabKey(e: KeyboardEvent) {
    const i = views.findIndex((v) => v.id === view);
    const next =
      e.key === 'ArrowRight' ? (i + 1) % views.length
      : e.key === 'ArrowLeft' ? (i - 1 + views.length) % views.length
      : e.key === 'Home' ? 0
      : e.key === 'End' ? views.length - 1
      : -1;
    if (next < 0) return;
    e.preventDefault();
    view = views[next].id;
    document.getElementById(`analyze-tab-${view}`)?.focus();
  }
</script>

{#if !app.selectedGame}
  <div class="panel empty-state">
    <p>Select a game to analyse its math.</p>
    <button class="primary" onclick={() => goTo('games')}>Go to Games</button>
  </div>
{:else if error}
  <div class="banner error" role="alert">
    <span>Could not read {app.selectedGame}: {error}</span>
    <button class="ghost" onclick={() => app.selectedGame && load(app.selectedGame)}>Retry</button>
  </div>
{:else if loading || !checks}
  <div class="loading" role="status" aria-live="polite">
    <p class="dim">Loading statistics for {app.selectedGame}…</p>
    <div class="skel-bar" aria-hidden="true"></div>
    <div class="skel-grid" aria-hidden="true">
      {#each [0, 1, 2, 3] as i (i)}<div class="skel-card"></div>{/each}
    </div>
  </div>
{:else if !current}
  <div class="panel"><p class="dim">{app.selectedGame} has no mode with statistics.</p></div>
{:else}
  {#if version?.sourceChanged || reloading}
    <div class="banner warn" role="status">
      {#if reloading}
        Reading the new math build…
      {:else}
        Math files changed on disk{#if version?.sourceMtime} at {fmtTime(version.sourceMtime)}{/if} — the figures below are those read
        {#if version}at {fmtTime(version.loadedAt)}{/if}. SlotLab reloads them a few seconds after the publication ends.
        <button class="ghost" onclick={reloadNow}>Reload now</button>
      {/if}
    </div>
  {:else if version && !version.imported}
    <div class="banner warn" role="status">
      {version.importing ? 'Books importing — bonus findings appear when it ends.' : 'Books not imported for this build — bonus findings are missing. Games → Import books.'}
    </div>
  {/if}
  {#if popsError}<div class="banner error">Populations unavailable: {popsError}</div>{/if}
  {#if marketError}<div class="banner warn">Market unavailable: {marketError}</div>{/if}
  {#if bonusError}<div class="banner warn">Bonus analysis unavailable: {bonusError}</div>{/if}
  {#if tiersError}<div class="banner warn">Win tiers unavailable: {tiersError}</div>{/if}

  <header class="verdict-bar">
    <div class="verdict-main">
      <button
        class="pill {checks.passed ? 'green' : 'red'} big"
        title="Open Compliance"
        onclick={() => (view = 'compliance')}
      >
        {checks.passed ? 'COMPLIANT' : `${checks.failCount} FAIL`}
      </button>
      <span
        class="stars"
        role="img"
        aria-label="Estimated tier: {checks.starTier} of 3 stars"
        title="Estimated tier (computable constraints)"
      >
        {'★'.repeat(checks.starTier)}<span class="stars-off">{'☆'.repeat(3 - checks.starTier)}</span>
      </span>
      {#if checks.warnCount > 0}<span class="pill yellow">{checks.warnCount} warn</span>{/if}
      <button
        class="pill {compliance === 'strict' ? 'blue' : 'gold'} compliance"
        title={compliance === 'strict' ? 'The 3★ grid is a rule for this game — click if Stake agreed this studio may ship past it (Degen Lab)' : 'Stake agreed this studio may ship past the 3★ grid: overruns are context, not faults — click to make the grid a rule again'}
        aria-pressed={compliance === 'lenient'}
        onclick={toggleCompliance}
      >
        {compliance === 'strict' ? '3★ required' : '3★ exceptions agreed'}
      </button>
      {#if not2Star.length}
        <span class="tier-note" title="These constraints pass the 3★ limit but not the 2★ one — see Compliance">
          not eligible for 2★: {not2Star.join(', ')}
        </span>
      {/if}
    </div>
    <div class="verdict-meta">
      <button
        class="wal"
        class:pending={!analysisReady}
        class:bad={nBlocking > 0}
        class:ok={analysisReady && nBlocking + nSerious + nMinor === 0}
        aria-label="Show findings: {walText}"
        onclick={() => (view = 'overview')}
      >
        {walText}
      </button>
      <span class="faint">{stats.length} modes · RTP variance {fmtPct(checks.crossModeVariance, 2)}</span>
    </div>
  </header>

  <div class="mode-grid" role="group" aria-label="Modes">
    {#each stats as m (m.mode)}
      {@const mc = checks.modes.find((c) => c.mode === m.mode)}
      {@const bf = bonusFreq(m.mode)}
      <button
        class="mode-card"
        class:active={selectedMode === m.mode}
        aria-pressed={selectedMode === m.mode}
        onclick={() => (selectedMode = m.mode)}
      >
        <span class="mc-top">
          <span class="mc-name">{m.mode}</span>
          <span class="mc-cost">{fx(m.stats.cost)}</span>
          {#if mc}
            <span
              class="mc-dot {mc.passed === mc.total ? 'ok' : 'bad'}"
              title="{mc.passed}/{mc.total} checks pass"
              aria-hidden="true"
            ></span>
            <span class="sr-only">{mc.passed} of {mc.total} checks pass</span>
          {/if}
        </span>
        <span class="mc-stats">
          <span class="mc-stat"><i>RTP</i><b>{fmtPct(m.stats.rtp, 2)}</b></span>
          <span class="mc-stat"><i>Std÷cost</i><b>{(m.stats.stdDev / Math.max(1, m.stats.cost)).toFixed(1)}</b></span>
          <span class="mc-stat"><i>Max</i><b>{fmtX(m.stats.maxPayout, 0)}</b></span>
        </span>
        <span class="mc-bonus" class:none={!bf}><i>Bonus</i> {bf ?? '—'}</span>
      </button>
    {/each}
  </div>

  <div class="view-tabs">
    <div class="tablist" role="tablist" aria-label="Analyze views">
      {#each views as v (v.id)}
        <button
          id="analyze-tab-{v.id}"
          role="tab"
          aria-selected={view === v.id}
          aria-controls="analyze-panel"
          tabindex={view === v.id ? 0 : -1}
          class:active={view === v.id}
          onclick={() => (view = v.id)}
          onkeydown={onTabKey}
        >
          {v.label}{#if v.id === 'compare' && pinned.length}<span class="count">{pinned.length}<span class="sr-only"> pinned</span></span>{/if}
        </button>
      {/each}
    </div>
    <span class="view-context faint">{view === 'compare' ? '' : selectedMode}</span>
  </div>

  <div id="analyze-panel" role="tabpanel" aria-labelledby="analyze-tab-{view}">
    {#if view === 'overview'}
      <SessionReach game={app.selectedGame ?? ''} mode={selectedMode} market={currentMarket} />
      <Overview {stats} {checks} {pops} {selectedMode} {issues} {market} ready={analysisReady} onSelectMode={(m) => (selectedMode = m)} onTarget={openTarget} />
    {:else if view === 'explore'}
      <section class="page">
        <Explore
          all={pops}
          mode={selectedMode}
          selectedKey={selectedKey[selectedMode] ?? 'all'}
          pinned={pinnedIds}
          {multiTrigger}
          {customThresholds}
          onSelect={select}
          onTogglePin={togglePin}
          onPinMany={pinMany}
          onPlay={playBook}
          onBookmark={bookmarkBook}
          onAddThreshold={(x) => setThresholds([...customThresholds, x])}
          onRemoveThreshold={(x) => setThresholds(customThresholds.filter((c) => Math.abs(c - x) > 1e-9))}
        />
      </section>
    {:else if view === 'mechanics'}
      <Mechanics game={app.selectedGame ?? ''} {selectedMode} />
    {:else if view === 'compare'}
      <Compare
        all={pops}
        {pinned}
        {customThresholds}
        onUnpin={unpin}
        onClear={() => {
          pinned = [];
          persist();
        }}
        onPinMany={pinMany}
        onOpen={open}
      />
    {:else if view === 'compliance'}
      <Compliance {checks} {selectedMode} />
    {:else if view === 'tiers'}
      <WinTiers game={app.selectedGame ?? ''} {selectedMode} {tiers} {ladderSource} />
    {:else if view === 'market'}
      <section class="page">
        <Market game={app.selectedGame ?? ''} {selectedMode} data={market} loading={marketLoading} error={marketError} onReload={() => app.selectedGame && loadMarket(app.selectedGame)} />
      </section>
    {/if}
  </div>
{/if}

<style>
  .empty-state {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-4);
    flex-wrap: wrap;
  }

  .empty-state p {
    margin: 0;
    color: var(--text-dim);
  }

  .banner.error button {
    margin-left: auto;
  }

  /* ── Loading placeholder ── */
  .loading p {
    margin: 0 0 var(--sp-3);
    font-size: var(--fs-sm);
  }

  .skel-bar,
  .skel-card {
    border-radius: var(--r-m);
    background: var(--bg-panel);
    border: 1px solid var(--border);
  }

  .skel-bar {
    height: 30px;
    width: min(420px, 100%);
    margin-bottom: var(--sp-4);
  }

  .skel-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: var(--sp-2);
  }

  .skel-card {
    height: 92px;
  }

  @media (prefers-reduced-motion: no-preference) {
    .skel-bar,
    .skel-card {
      animation: pulse 1.4s ease-in-out infinite;
    }
  }

  @keyframes pulse {
    50% {
      opacity: 0.45;
    }
  }

  /* ── Verdict ── */
  .verdict-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--sp-2) var(--sp-4);
    padding: var(--sp-1) 0 var(--sp-4);
  }

  .verdict-main {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--sp-3);
  }

  .verdict-main button.pill:hover:not(:disabled) {
    background: color-mix(in oklch, currentColor var(--tint-strong), transparent);
    border-color: color-mix(in oklch, currentColor 50%, transparent);
  }

  .stars {
    color: var(--gold);
    font-size: var(--fs-lg);
    letter-spacing: 2px;
  }

  .stars-off {
    color: var(--text-faint);
  }

  .tier-note {
    font-size: var(--fs-xs);
    color: var(--text-dim);
  }

  .verdict-main .compliance {
    font-size: var(--fs-2xs);
  }

  .verdict-meta {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    font-size: var(--fs-xs);
  }

  .wal {
    --c: var(--warn);
    font-size: var(--fs-xs);
    font-weight: 620;
    padding: 3px 11px;
    border-radius: var(--r-pill);
    color: var(--c);
    border-color: color-mix(in oklch, var(--c) 35%, transparent);
    background: color-mix(in oklch, var(--c) var(--tint-weak), transparent);
  }

  .wal:hover:not(:disabled) {
    border-color: color-mix(in oklch, var(--c) 60%, transparent);
    background: color-mix(in oklch, var(--c) var(--tint), transparent);
  }

  .wal.bad {
    --c: var(--fail);
  }

  .wal.ok {
    --c: var(--pass);
  }

  .wal.pending {
    color: var(--text-faint);
    border-color: var(--border);
    background: none;
  }

  /* ── Mode cards ── */
  .mode-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: var(--sp-2);
    margin-bottom: var(--sp-6);
  }

  .mode-card {
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    gap: 7px;
    min-width: 0;
    text-align: left;
    padding: 10px var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    background: var(--bg-panel);
  }

  .mode-card:hover:not(:disabled) {
    border-color: var(--border-strong);
    background: var(--bg-raised);
  }

  .mode-card.active,
  .mode-card.active:hover:not(:disabled) {
    border-color: var(--gold);
    background: color-mix(in oklch, var(--gold) var(--tint-weak), transparent);
    box-shadow: inset 3px 0 0 var(--gold);
  }

  .mc-top {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .mc-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 680;
    font-size: var(--fs-base);
  }

  .mc-cost {
    flex-shrink: 0;
    color: var(--text-faint);
    font-size: var(--fs-xs);
    font-family: var(--font-mono);
  }

  .mc-dot {
    flex-shrink: 0;
    margin-left: auto;
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .mc-dot.ok { background: var(--pass); box-shadow: 0 0 6px var(--pass); }
  .mc-dot.bad { background: var(--fail); box-shadow: 0 0 6px var(--fail); }

  /* Label over value, one column each; never wider than the card. */
  .mc-stats {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, auto));
    justify-content: space-between;
    column-gap: 6px;
    row-gap: 6px;
  }

  /* Very narrow card: RTP | Std on a row, Max under. */
  @container (max-width: 150px) {
    .mc-stats {
      grid-template-columns: repeat(2, minmax(0, auto));
    }
  }

  .mc-stat {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .mc-stat b {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
  }

  .mc-stats i,
  .mc-bonus i {
    font-style: normal;
    font-family: var(--font-body);
    font-size: var(--fs-2xs);
    white-space: nowrap;
    color: var(--text-faint);
  }

  .mc-bonus {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    color: var(--text-dim);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .mc-bonus.none {
    color: var(--text-faint);
  }

  /* ── Sub-view tabs (the strip is global; the tablist sits inside it) ── */
  .tablist {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-wrap: wrap;
  }
</style>
