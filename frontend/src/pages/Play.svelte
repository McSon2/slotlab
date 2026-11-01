<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import {
    api,
    rgsUrlFor,
    EMPTY_FAULTS,
    type ImportJob,
    type Faults,
    type FaultEndpoint,
    type PoolOptions,
    type PoolSummary,
  } from '../lib/api';
  import { app, goTo, notify, refreshGames, run, selectedGame } from '../lib/stores.svelte';
  import { storageGetJson, storageSetJson } from '../lib/storage';
  import {
    VIEWPORT_PRESETS,
    REVIEW_VIEWPORTS,
    CURRENCIES,
    SOCIAL_CURRENCIES,
    LANGUAGES,
    type ViewportPreset,
  } from '../lib/viewports';
  import GameFrame from '../components/GameFrame.svelte';
  import LiveEventFeed from '../components/LiveEventFeed.svelte';
  import Bookmarks from '../components/Bookmarks.svelte';
  import BookSearch from '../components/BookSearch.svelte';
  import ConfirmButton from '../components/ConfirmButton.svelte';
  import Icon from '../components/Icon.svelte';

  /** False while another tab is shown: App keeps Play mounted (the frames
   * must not reload), so its own background work pauses instead. */
  let { active = true }: { active?: boolean } = $props();

  /** How long the frames survive on another tab. A quick look at Analyze
   * must not reset a game mid-bonus, but each frame is a live WebGL game
   * (GPU context, textures, audio, 100-300 MB): left behind for good, they
   * weigh on every other tab. Past this delay they are released, and come
   * back reloaded. */
  const HIDDEN_FRAMES_TTL_MS = 3 * 60_000;
  let framesAlive = $state(true);
  $effect(() => {
    if (active) {
      framesAlive = true;
      return;
    }
    const t = setTimeout(() => (framesAlive = false), HIDDEN_FRAMES_TTL_MS);
    return () => clearTimeout(t);
  });

  const PREFS_KEY = 'slotlab-play-prefs';

  interface PlayPrefs {
    activeViewports: string[];
    currency: string;
    social: boolean;
    lang: string;
    balance: number;
    scalePct: number;
    customViewports: ViewportPreset[];
    muted: boolean;
  }

  const saved = storageGetJson<Partial<PlayPrefs>>(PREFS_KEY, {});

  let tlsTrusted = $state(true);
  let installBusy = $state(false);

  let activeViewports = $state<string[]>(saved.activeViewports ?? ['desktop', 'mobile-m']);
  const savedSocial = saved.social ?? false;
  const savedCurrency = saved.currency ?? 'USD';
  let social = $state(savedSocial);
  // Prefs inherited from a version without social filtering: realign currency ↔ social.
  let currency = $state(
    savedSocial === SOCIAL_CURRENCIES.includes(savedCurrency)
      ? savedCurrency
      : savedSocial ? SOCIAL_CURRENCIES[0] : 'USD',
  );
  let lang = $state(saved.lang ?? 'en');
  let balance = $state(saved.balance ?? 10000);
  let scalePct = $state(saved.scalePct ?? 60);
  /** Width available to the viewports: a frame never gets wider than the screen
   * (phone): its zoom drops to fit, the chosen zoom stays the ceiling. */
  /** Control cards: open on a wide screen, folded on a phone (the games come first). */
  const wide = typeof matchMedia === 'undefined' || matchMedia('(min-width: 861px)').matches;
  let openSession = $state(wide);
  let openForce = $state(wide);
  let openViewports = $state(wide);
  let framesWidth = $state(0);
  function fitScale(frameWidth: number): number {
    const chosen = scalePct / 100;
    if (!framesWidth) return chosen;
    return Math.min(chosen, (framesWidth - 2) / frameWidth);
  }
  let customViewports = $state<ViewportPreset[]>(saved.customViewports ?? []);
  /** Silences every frame at once (several viewports = several soundtracks). */
  let muted = $state(saved.muted ?? false);
  /** Game clock rate, for watching a spin frame by frame. Deliberately not
   * saved: a session that opens slowed down would pass for a broken game. */
  const SPEEDS = [0, 0.05, 0.1, 0.25, 0.5, 1, 2];
  let speedIdx = $state(SPEEDS.indexOf(1));
  const speed = $derived(SPEEDS[speedIdx]);
  let customW = $state<number | null>(null);
  let customH = $state<number | null>(null);

  let forceMode = $state('');
  let forceSim = $state('');
  /** What the server is forcing right now (the inputs may have moved on). */
  let forced = $state<{ mode: string; simId: number } | null>(null);
  let forceBusy = $state(false);
  const forceSimValid = $derived(/^\d+$/.test(forceSim.trim()));

  // Bonus-only draw, per spin mode (base, hunt…): every spin of that mode
  // opens a bonus, a different one each time (LUT-weighted among the bonus
  // books) — a pinned book always wins over it.
  let poolModes = $state<PoolOptions[]>([]);
  /** What the server restricts right now, by mode. */
  let poolActive = $state<Record<string, PoolSummary>>({});
  /** By mode: 'off' | 'any' | 'tier:N' */
  let poolChoice = $state<Record<string, string>>({});
  /** Mode whose change is in flight. */
  let poolBusy = $state<string | null>(null);
  let poolLoadedFor = $state<string | null>(null);

  let epoch = $state(1);
  let reloadKey = $state(0);
  /** Game the frames are running for (auto-launch on change). */
  let launchedFor = $state<string | null>(null);
  /** Launches in flight (several controls relaunch on change). */
  let launches = $state(0);

  // ── QA scenarios ─────────────────────────────────────────────────────────
  // The states the review reaches by accident: a round left open by a closed
  // tab, a balance too small for the next spin, an RGS that errors or crawls,
  // a broken rgs_url. None of them can be produced by playing normally, and
  // four refusals came from the game's answer to them.
  let faults = $state<Faults>({ ...EMPTY_FAULTS });
  let faultEndpoint = $state<FaultEndpoint>('play');
  let faultKind = $state<'rgsError' | 'http500' | 'hang'>('rgsError');
  let faultCode = $state('ERR_GE');
  let faultOnce = $state(true);
  /** Launches the frames with an rgs_url that resolves nowhere. */
  let brokenRgs = $state(false);
  let faultsBusy = $state(false);
  let balanceBusy = $state(false);
  /** Guidance after "Balance → $x": persistent until the next launch. */
  let balanceNote = $state<string | null>(null);

  let importInfo = $state<{ imported: boolean; job: ImportJob | null } | null>(null);
  let retryBusy = $state(false);
  /** Custom URL (Vite dev). Empty = build served by SlotLab (auto). */
  let customUrl = $state('');

  const game = $derived(selectedGame());
  const baseUrl = $derived(
    customUrl.trim() ? customUrl.trim().replace(/\/$/, '') : (game?.servedUrl ?? ''),
  );
  const canPlay = $derived(Boolean(game?.loaded && baseUrl));
  /** The games list can lag behind the import poll: either one is enough. */
  const isImported = $derived(Boolean(game?.imported || importInfo?.imported));

  const allViewports = $derived([...VIEWPORT_PRESETS, ...customViewports]);

  $effect(() => {
    const prefs: PlayPrefs = {
      activeViewports, currency, social, lang, balance, scalePct, customViewports, muted,
    };
    storageSetJson(PREFS_KEY, prefs);
  });

  onMount(() => {
    api.tlsStatus().then((s) => (tlsTrusted = s.trusted)).catch(() => {});
    return stopImportPoll;
  });

  let seenGame = $state<string | null>(null);
  $effect(() => {
    if (!game) return;
    if (seenGame !== game.name) {
      // Game change: restore its persisted custom URL.
      seenGame = game.name;
      customUrl = game.frontUrl ?? '';
      importInfo = null;
      balanceNote = null;
      // A force belongs to the previous game; the form starts on its first mode.
      forced = null;
      forceSim = '';
      forceMode = game.modes[0]?.name ?? '';
      poolModes = [];
      poolActive = {};
      poolChoice = {};
      poolLoadedFor = null;
      // Faults live on the server: a panel that forgot them would lie about
      // what the next spin is going to do.
      const name = game.name;
      api.getFaults(name)
        .then((r) => { if (name === app.selectedGame) faults = r.faults; })
        .catch(() => (faults = { ...EMPTY_FAULTS }));
      untrack(() => void pollImport());
    }
  });

  // The modes list can change on reload: never leave the select on a mode
  // that no longer exists (an empty select forces nothing).
  $effect(() => {
    if (game && !game.modes.some((m) => m.name === forceMode)) {
      forceMode = game.modes[0]?.name ?? '';
    }
  });

  // The pool needs the books index: loaded once the import is done.
  $effect(() => {
    if (game && isImported && poolLoadedFor !== game.name) {
      poolLoadedFor = game.name;
      untrack(() => void loadPool());
    }
  });

  function poolKey(p: PoolSummary | undefined): string {
    if (!p) return 'off';
    return p.tier == null ? 'any' : `tier:${p.tier}`;
  }

  function fmtOneIn(n: number): string {
    return n >= 100 ? Math.round(n).toLocaleString('en-US') : n.toFixed(1);
  }

  async function loadPool() {
    const name = app.selectedGame;
    if (!name) return;
    try {
      const r = await api.getPool(name);
      if (name !== app.selectedGame) return;
      poolModes = r.modes;
      poolActive = Object.fromEntries(r.active.map((p) => [p.mode, p]));
      poolChoice = Object.fromEntries(r.modes.map((o) => [o.mode, poolKey(poolActive[o.mode])]));
    } catch {
      poolModes = [];
    }
  }

  async function applyPool(mode: string) {
    if (!game) return;
    const name = game.name;
    const choice = poolChoice[mode] ?? 'off';
    const enabled = choice !== 'off';
    const tier = choice.startsWith('tier:') ? Number(choice.slice(5)) : null;
    poolBusy = mode;
    const r = await run(() => api.setPool({ game: name, enabled, mode, tier }), {
      ok: enabled
        ? `${mode}: bonus books only — every spin opens a bonus`
        : `${mode}: full LUT again`,
      error: 'Could not change the draw',
    });
    poolBusy = null;
    if (r) {
      const next = { ...poolActive };
      if (r.active) next[mode] = r.active;
      else delete next[mode];
      poolActive = next;
    }
    poolChoice = { ...poolChoice, [mode]: poolKey(poolActive[mode]) };
  }

  // Auto-launch: as soon as the selected game is imported and playable — and
  // only while Play is on screen, so a game picked from another tab doesn't
  // spin up hidden sessions.
  $effect(() => {
    if (!active || !game) return;
    if (isImported && canPlay && launchedFor !== game.name) {
      launchedFor = game.name;
      untrack(() => void launch());
    }
  });

  // ── Import polling ───────────────────────────────────────────────────────
  // One timer at most; paused while Play is hidden, resumed when it's back.
  let importTimer: ReturnType<typeof setTimeout> | null = null;
  /** Import not finished: a poll is owed (possibly deferred while hidden). */
  let importDue = false;

  function stopImportPoll() {
    if (importTimer) clearTimeout(importTimer);
    importTimer = null;
  }

  function scheduleImportPoll(ms: number) {
    stopImportPoll();
    importDue = true;
    if (active) importTimer = setTimeout(pollImport, ms);
  }

  $effect(() => {
    const on = active;
    untrack(() => {
      if (!on) stopImportPoll();
      else if (importDue && !importTimer) void pollImport();
    });
  });

  async function pollImport() {
    stopImportPoll();
    const name = app.selectedGame;
    if (!name) return;
    try {
      const info = await api.importStatus(name);
      if (name !== app.selectedGame) return; // game changed meanwhile
      importInfo = info;
      if (info.job?.state === 'running' || !info.imported) {
        // A failed import only changes on Retry: no need to hammer it.
        scheduleImportPoll(info.job?.state === 'failed' ? 5000 : 900);
      } else {
        importDue = false;
      }
    } catch {
      importInfo = null;
      importDue = false;
    }
  }

  async function retryImport() {
    if (!game) return;
    const name = game.name;
    retryBusy = true;
    await run(() => api.startImport(name), { error: 'Could not restart the import' });
    retryBusy = false;
    await pollImport();
  }

  async function installCa() {
    installBusy = true;
    const r = await run(() => api.tlsInstall(), { error: 'CA install failed' });
    if (r) tlsTrusted = r.trusted;
    installBusy = false;
  }

  function sessionId(viewportId: string): string {
    return `slotlab-${app.selectedGame}-${viewportId}-${epoch}`;
  }

  function frameUrl(viewportId: string, device: string): string {
    if (!game) return '';
    const params = new URLSearchParams({
      sessionID: sessionId(viewportId),
      // An unreachable RGS must give a blocking error screen, not a game
      // that renders its children over a dead session (Stake review rule §24.7).
      rgs_url: brokenRgs ? 'localhost:9/api/rgs/nowhere' : rgsUrlFor(game.name),
      lang,
      currency,
      device,
      social: String(social),
      demo: 'true',
    });
    return `${baseUrl}/?${params.toString()}&r=${reloadKey}`;
  }

  /** In social mode, only sweepstakes currencies are offered (and vice versa). */
  const currencyOptions = $derived(social ? SOCIAL_CURRENCIES : CURRENCIES);

  async function onSocialChange() {
    const opts = social ? SOCIAL_CURRENCIES : CURRENCIES;
    if (!opts.includes(currency)) currency = opts[0]; // social → XGC, otherwise USD
    await launch();
  }

  async function launch() {
    if (!game || !canPlay) return;
    const g = game;
    balanceNote = null;
    launches += 1;
    const ok = await run(
      async () => {
        if (customUrl.trim() !== (g.frontUrl ?? '')) {
          await api.updateFrontUrl(g.name, customUrl.trim());
          await refreshGames();
        }
        await Promise.all(
          activeViewports.map((v) =>
            api.prepareSession({
              sessionId: sessionId(v),
              game: g.name,
              balance: Math.round(balance * 1_000_000),
              currency,
              social,
              language: lang,
            }),
          ),
        );
        return true;
      },
      { error: 'Could not launch the game' },
    );
    launches -= 1;
    if (ok) reloadKey += 1;
  }

  async function resetSessions() {
    if (!game) return;
    const name = game.name;
    const ok = await run(() => api.resetSessions(name), {
      ok: 'Sessions reset — fresh balance and no open round',
      error: 'Could not reset the sessions',
    });
    if (!ok) return;
    epoch += 1;
    await launch();
  }

  /** Forces a book on the next spin of every viewport. */
  async function force(mode: string, simId: number): Promise<boolean> {
    if (!game) return false;
    const name = game.name;
    forceBusy = true;
    const ok = await run(() => api.setForce({ game: name, mode, simId }), {
      ok: `Forced ${mode} #${simId} — next spin`,
      error: 'Could not force the book',
    });
    forceBusy = false;
    if (ok) forced = { mode, simId };
    return Boolean(ok);
  }

  async function applyForce() {
    if (!game || !forceSimValid) return;
    const mode = forceMode || game.modes[0]?.name;
    if (!mode) return;
    await force(mode, Number(forceSim.trim()));
  }

  async function clearForce() {
    if (!game) return;
    const name = game.name;
    forceBusy = true;
    const ok = await run(() => api.setForce({ game: name }), {
      ok: 'Force cleared — spins draw random books again',
      error: 'Could not clear the force',
    });
    forceBusy = false;
    if (ok) forced = null;
  }

  /** From the search, the live feed or a bookmark: fills the form and forces. */
  function forceFrom(mode: string, simId: number) {
    forceMode = mode;
    forceSim = String(simId);
    void force(mode, simId);
  }

  async function pushFaults(ok?: string) {
    if (!game) return;
    const name = game.name;
    faultsBusy = true;
    const r = await run(() => api.setFaults(name, faults), {
      ok,
      error: 'Could not update the QA scenarios',
    });
    faultsBusy = false;
    if (r) {
      faults = r.faults;
    } else {
      // The panel was edited optimistically: resync it with what the server runs.
      api.getFaults(name).then((s) => (faults = s.faults)).catch(() => {});
    }
  }

  async function armFailure() {
    const code = faultCode.trim() || 'ERR_GE';
    faults = {
      ...faults,
      failures: [
        ...faults.failures.filter((f) => f.endpoint !== faultEndpoint),
        {
          endpoint: faultEndpoint,
          kind: faultKind,
          code,
          remaining: faultOnce ? 1 : 0,
          limited: faultOnce,
        },
      ],
    };
    const what = faultKind === 'rgsError' ? `RGS error ${code}` : faultKind === 'http500' ? 'HTTP 500' : 'no answer';
    await pushFaults(`Armed: ${faultEndpoint} → ${what}${faultOnce ? ' (once)' : ''}`);
  }

  async function dropFailure(endpoint: FaultEndpoint) {
    faults = {
      ...faults,
      failures: faults.failures.filter((f) => f.endpoint !== endpoint),
    };
    await pushFaults();
  }

  async function clearFaults() {
    const wasBroken = brokenRgs;
    faults = { ...EMPTY_FAULTS };
    brokenRgs = false;
    await pushFaults('QA scenarios cleared');
    // The frames were launched on the dead rgs_url: point them back.
    if (wasBroken) await launch();
  }

  /** Walks every open viewport down to a balance, in dollars. */
  async function setBalanceTo(dollars: number) {
    if (!game) return;
    const name = game.name;
    balanceBusy = true;
    const ok = await run(
      () =>
        Promise.all(
          activeViewports.map((v) =>
            api.setBalance({
              sessionId: sessionId(v),
              game: name,
              balance: Math.round(dollars * 1_000_000),
            }),
          ),
        ),
      { error: 'Could not set the balance' },
    );
    balanceBusy = false;
    if (ok) {
      balanceNote =
        `Balance set to $${dollars.toFixed(2)} — spin now: the game must say why it can't, ` +
        `on all three paths (click, autoplay, space hold).`;
    }
  }

  const faultsOn = $derived(
    faults.latencyMs > 0 || faults.keepRoundOpen || faults.failures.length > 0 || brokenRgs,
  );

  function toggleViewport(id: string) {
    activeViewports = activeViewports.includes(id)
      ? activeViewports.filter((v) => v !== id)
      : [...activeViewports, id];
    void launch();
  }

  const customValid = $derived(
    Number(customW) >= 100 && Number(customH) >= 100,
  );

  function addCustomViewport() {
    const w = Math.round(Number(customW));
    const h = Math.round(Number(customH));
    if (!w || !h || w < 100 || h < 100) return;
    const id = `custom-${w}x${h}`;
    if (allViewports.some((v) => v.id === id)) {
      notify(`${w}×${h} is already in the list`, 'info');
      return;
    }
    customViewports = [
      ...customViewports,
      { id, label: `${w}×${h}`, width: w, height: h, device: w >= h ? 'desktop' : 'mobile' },
    ];
    activeViewports = [...activeViewports, id];
    customW = null;
    customH = null;
    void launch();
  }

  function removeCustomViewport(id: string) {
    customViewports = customViewports.filter((v) => v.id !== id);
    activeViewports = activeViewports.filter((v) => v !== id);
  }

  const importPct = $derived(
    importInfo?.job && importInfo.job.bytesTotal > 0
      ? Math.round((importInfo.job.bytesDone / importInfo.job.bytesTotal) * 100)
      : 0,
  );
</script>

{#if !tlsTrusted}
  <div class="banner warn">
    <span>
      The local CA is not installed — games can't reach the RGS over HTTPS.
      Once is enough (macOS password prompted).
    </span>
    <button class="primary" onclick={installCa} disabled={installBusy}>
      {installBusy ? 'Installing…' : 'Install the CA'}
    </button>
  </div>
{/if}

{#if !game}
  <div class="panel empty">
    <p class="dim">
      No game yet. Add a <code>publish_files</code> folder in
      <strong>Games</strong> — everything else is automatic.
    </p>
    <button class="primary" onclick={() => goTo('games')}>Add a game</button>
  </div>
{:else}
  {#if importInfo && !importInfo.imported}
    {#if importInfo.job?.state === 'failed'}
      <div class="banner error">
        <span>Import failed: {importInfo.job.error}</span>
        <button onclick={retryImport} disabled={retryBusy}>
          {retryBusy ? 'Retrying…' : 'Retry'}
        </button>
      </div>
    {:else}
      <div class="banner info">
        <span>
          Preparing books for <strong>{game.name}</strong>
          {#if importInfo.job?.currentMode}
            — {importInfo.job.currentMode} ({importInfo.job.modesDone}/{importInfo.job.modesTotal})
          {/if}
          <span class="dim">· the game will launch on its own</span>
        </span>
        <progress
          max="100"
          value={importPct}
          style="flex:1"
          aria-label="Import progress"
        ></progress>
        <span class="num">{importPct}%</span>
      </div>
    {/if}
  {/if}

  {#if !game.servedUrl && !customUrl}
    <div class="banner warn">
      <span>
        No frontend build detected for <strong>{game.name}</strong> —
        build it (<code>pnpm --filter {game.name} build</code>) then
        "Reload" in Games, or provide a Vite dev URL below.
      </span>
    </div>
  {/if}

  <!-- Controls: four cards. Session and force are what you touch per test;
       viewports and display shape what you see. On a phone every card folds. -->
  <div class="controls">
    <details class="ctl-card" bind:open={openSession}>
      <summary><span class="ctl-title">Session</span><span class="ctl-sum">{currency} · {lang} · ${balance}{social ? ' · social' : ''}</span></summary>
      <div class="ctl-body">
        <div class="fields" role="group" aria-label="Session">
          <label>
            <span>Currency</span>
            <select bind:value={currency} onchange={launch}>
              {#each currencyOptions as c (c)}<option>{c}</option>{/each}
            </select>
          </label>
          <label>
            <span>Language</span>
            <select bind:value={lang} onchange={launch}>
              {#each LANGUAGES as l (l)}<option>{l}</option>{/each}
            </select>
          </label>
          <label>
            <span>Balance $</span>
            <input
              type="number"
              bind:value={balance}
              min="0"
              step="100"
              style="width: 90px"
              onchange={launch}
            />
          </label>
          <label class="check">
            <input type="checkbox" bind:checked={social} onchange={onSocialChange} />
            <span>Social</span>
          </label>
        </div>
        <div class="ctl-actions">
            <button onclick={launch} disabled={launches > 0} title="Prepare the sessions and reload every viewport">
              {launches > 0 ? 'Reloading…' : 'Reload'}
            </button>
            <ConfirmButton
              onconfirm={resetSessions}
              confirmLabel="Reset all sessions?"
              title="New sessions: fresh balance, no open round"
            >Reset sessions</ConfirmButton>
        </div>
      </div>
    </details>

    <details class="ctl-card" bind:open={openForce}>
      <summary>
        <span class="ctl-title">Force a result</span>
        <span class="ctl-sum">{forced ? `${forced.mode} #${forced.simId}` : 'random books'}</span>
      </summary>
      <div class="ctl-body">
        <form
          class="fields"
          aria-label="Force a book"
          onsubmit={(e) => {
            e.preventDefault();
            void applyForce();
          }}
        >
          <label>
            <span>Force · mode</span>
            <select bind:value={forceMode}>
              {#each game.modes as m (m.name)}<option value={m.name}>{m.name}</option>{/each}
            </select>
          </label>
          <label>
            <span>Event ID</span>
            <input
              bind:value={forceSim}
              placeholder="e.g. 50931"
              inputmode="numeric"
              autocomplete="off"
              style="width: 96px"
            />
          </label>
          <button type="submit" disabled={!forceSimValid || forceBusy}>
            {forceBusy ? 'Forcing…' : 'Force'}
          </button>
          {#if forced}
            <button
              type="button"
              class="ghost forced-chip"
              onclick={clearForce}
              disabled={forceBusy}
              title="Stop forcing — spins draw random books again"
              aria-label="Clear the forced book {forced.mode} #{forced.simId}"
            >
              <Icon name="close" size={12} />
              {forced.mode} #{forced.simId}
            </button>
          {/if}
        </form>
        {#if poolModes.length > 0}
          <div class="fields" role="group" aria-label="Bonus-only draw">
            {#each poolModes as o (o.mode)}
              <label title="Each {o.mode} spin draws a random bonus book, weighted like the real game. A forced book wins over it.">
                <span>{o.mode} · draw</span>
                <select
                  value={poolChoice[o.mode] ?? 'off'}
                  onchange={(e) => {
                    poolChoice = { ...poolChoice, [o.mode]: e.currentTarget.value };
                    void applyPool(o.mode);
                  }}
                  disabled={poolBusy === o.mode}
                >
                  <option value="off">All books</option>
                  <option value="any">
                    Bonus only · {o.any.books.toLocaleString('en-US')} books · 1 in {fmtOneIn(o.any.oneIn)}
                  </option>
                  {#if o.tiers.length > 1}
                    {#each o.tiers as t (t.tier)}
                      <option value="tier:{t.tier}">
                        Bonus {t.tier} scatters · {t.books.toLocaleString('en-US')} books · 1 in {fmtOneIn(t.oneIn)}
                      </option>
                    {/each}
                  {/if}
                </select>
              </label>
            {/each}
            {#if forced && poolActive[forced.mode]}
              <span class="pool-note" role="status">{forced.mode} #{forced.simId} forced first — clear it to draw bonuses</span>
            {/if}
          </div>
        {/if}
      </div>
    </details>

    <details class="ctl-card ctl-wide" bind:open={openViewports}>
      <summary>
        <span class="ctl-title">Viewports</span>
        <span class="ctl-sum">{activeViewports.length} open</span>
      </summary>
      <div class="ctl-body">
        <div class="vp-chips" role="group" aria-label="Viewports">
          {#each allViewports as v (v.id)}
            <span class="vp-chip" class:on={activeViewports.includes(v.id)}>
              <button
                type="button"
                aria-pressed={activeViewports.includes(v.id)}
                onclick={() => toggleViewport(v.id)}
              >
                <span class="vp-name">{v.label}</span>
                <span class="vp-size">{v.width}×{v.height}</span>
              </button>
              {#if v.id.startsWith('custom-')}
                <button
                  type="button"
                  class="vp-remove"
                  aria-label="Remove viewport {v.label}"
                  title="Remove viewport {v.label}"
                  onclick={() => removeCustomViewport(v.id)}
                ><Icon name="close" size={11} /></button>
              {/if}
            </span>
          {/each}
        </div>
        <div class="vp-tools">
      <button
        class="ghost"
        title="The {REVIEW_VIEWPORTS.length} viewports the review always opens"
        onclick={() => {
          activeViewports = [...REVIEW_VIEWPORTS];
          void launch();
        }}
      >Review pack</button>
      <form
        class="custom-add"
        aria-label="Add a custom viewport"
        onsubmit={(e) => {
          e.preventDefault();
          addCustomViewport();
        }}
      >
        <input
          type="number"
          min="100"
          step="1"
          style="width: 72px"
          bind:value={customW}
          placeholder="W"
          aria-label="Custom width"
        />
        <span aria-hidden="true">×</span>
        <input
          type="number"
          min="100"
          step="1"
          style="width: 72px"
          bind:value={customH}
          placeholder="H"
          aria-label="Custom height"
        />
        <button type="submit" class="ghost" disabled={!customValid}>+ custom</button>
      </form>
      <label
        class="zoom"
        title={customUrl
          ? 'Only on the build served by SlotLab (the custom URL is not instrumented)'
          : 'Slows the game clocks: animations, timers, Spine, tweens. Audio keeps its pace.'}
      >
        <span>
          Speed {speed === 0 ? 'paused' : `×${speed}`}
          {#if speed !== 1}
            <button type="button" class="ghost reset" onclick={() => (speedIdx = SPEEDS.indexOf(1))}>×1</button>
          {/if}
        </span>
        <input
          type="range"
          min="0"
          max={SPEEDS.length - 1}
          step="1"
          bind:value={speedIdx}
          disabled={Boolean(customUrl)}
        />
      </label>
      <button
        type="button"
        class="ghost mute"
        class:on={muted}
        aria-pressed={muted}
        disabled={Boolean(customUrl)}
        title={customUrl
          ? 'Only on the build served by SlotLab (the custom URL is not instrumented)'
          : muted ? 'Sound off in every frame — click to turn it back on' : 'Mute every frame'}
        onclick={() => (muted = !muted)}
      ><Icon name={muted ? 'sound-off' : 'sound'} /> {muted ? 'Muted' : 'Sound'}</button>
      <label class="zoom">
        <span>Zoom {scalePct}%</span>
        <input type="range" min="25" max="100" step="5" bind:value={scalePct} />
      </label>
        </div>
      </div>
    </details>
  </div>

  <details class="advanced">
    <summary>Custom frontend URL (Vite dev, hot-reload)</summary>
    <form
      class="advanced-row"
      onsubmit={(e) => {
        e.preventDefault();
        void launch();
      }}
    >
      <input
        bind:value={customUrl}
        aria-label="Custom frontend URL"
        placeholder="http://localhost:5174 — empty = build served by SlotLab ({game.servedUrl ?? 'none'})"
      />
      <button type="submit" disabled={launches > 0}>Apply</button>
      {#if customUrl}
        <button type="button" class="ghost" onclick={() => { customUrl = ''; void launch(); }}>
          Back to auto build
        </button>
      {/if}
    </form>
  </details>

  <details class="advanced" class:armed={faultsOn}>
    <summary>
      QA scenarios — fault injection
      {#if faultsOn}<span class="pill yellow">armed</span>{/if}
    </summary>
    <div class="qa-grid">
      <div class="qa-block">
        <p class="overline">Interrupted round</p>
        <label class="check">
          <input
            type="checkbox"
            bind:checked={faults.keepRoundOpen}
            disabled={faultsBusy}
            onchange={() => { faults.keepRoundOpenOnce = faults.keepRoundOpen; void pushFaults(); }}
          />
          <span>Leave the next round open (unpaid)</span>
        </label>
        <p class="note">
          Spin once, then <strong>Reload</strong>: authenticate hands the round
          back and the game must offer to watch or skip it — with the bet of
          THAT round restored, not the one currently selected.
        </p>
      </div>

      <div class="qa-block">
        <p class="overline">Not enough to spin</p>
        <div class="qa-row">
          <button onclick={() => setBalanceTo(0.5)} disabled={balanceBusy}>Balance → $0.50</button>
          <button
            onclick={() => setBalanceTo(0)}
            disabled={balanceBusy}
            aria-label="Balance → $0"
          >→ $0</button>
        </div>
        <p class="note">
          The buttons must stay clickable and answer with a modal. The three
          spin paths are click, autoplay and holding space.
        </p>
      </div>

      <div class="qa-block">
        <p class="overline">RGS misbehaviour</p>
        <form
          class="qa-row"
          aria-label="Arm an RGS failure"
          onsubmit={(e) => {
            e.preventDefault();
            void armFailure();
          }}
        >
          <select bind:value={faultEndpoint} aria-label="Endpoint to fail">
            <option value="play">play</option>
            <option value="authenticate">authenticate</option>
            <option value="balance">balance</option>
            <option value="endRound">end-round</option>
            <option value="event">bet/event</option>
            <option value="replay">replay</option>
          </select>
          <select bind:value={faultKind} aria-label="Failure kind">
            <option value="rgsError">RGS error</option>
            <option value="http500">HTTP 500</option>
            <option value="hang">never answers</option>
          </select>
          {#if faultKind === 'rgsError'}
            <input
              bind:value={faultCode}
              style="width: 92px"
              placeholder="ERR_GE"
              aria-label="RGS error code"
            />
          {/if}
          <label class="check">
            <input type="checkbox" bind:checked={faultOnce} />
            <span>once</span>
          </label>
          <button type="submit" disabled={faultsBusy}>Arm</button>
        </form>
        {#if faults.failures.length > 0}
          <div class="qa-row">
            {#each faults.failures as f (f.endpoint)}
              <button
                class="ghost"
                disabled={faultsBusy}
                title="Disarm this failure"
                onclick={() => dropFailure(f.endpoint)}
              >
                <Icon name="close" size={12} />
                {f.endpoint} · {f.kind}{f.kind === 'rgsError' ? ' ' + f.code : ''}{f.limited ? ` ×${f.remaining}` : ''}
              </button>
            {/each}
          </div>
        {/if}
      </div>

      <div class="qa-block">
        <p class="overline">Slow link</p>
        <div class="qa-row" role="group" aria-label="RGS latency">
          {#each [0, 250, 800, 2000] as ms (ms)}
            <button
              class:primary={faults.latencyMs === ms}
              aria-pressed={faults.latencyMs === ms}
              disabled={faultsBusy}
              onclick={() => { faults.latencyMs = ms; void pushFaults(); }}
            >{ms === 0 ? 'none' : ms + ' ms'}</button>
          {/each}
        </div>
        <p class="note">
          Every RGS answer is delayed. A double click during the wait must not
          buy two spins.
        </p>
      </div>

      <div class="qa-block">
        <p class="overline">Broken rgs_url</p>
        <div class="qa-row">
          <label class="check">
            <input type="checkbox" bind:checked={brokenRgs} onchange={launch} />
            <span>Launch pointing nowhere</span>
          </label>
          <ConfirmButton
            onconfirm={clearFaults}
            confirmLabel="Clear all scenarios?"
            disabled={faultsBusy || !faultsOn}
            title="Disarm every QA scenario above"
          >Clear everything</ConfirmButton>
        </div>
        <p class="note">
          A game whose RGS never answers must show a blocking error screen and
          render nothing else (Stake review rule §24.7).
        </p>
      </div>
    </div>
  </details>

  {#if balanceNote}
    <div class="banner info">
      <span>{balanceNote}</span>
      <button
        class="ghost dismiss"
        aria-label="Dismiss"
        onclick={() => (balanceNote = null)}
      ><Icon name="close" size={13} /></button>
    </div>
  {/if}

  {#if canPlay && isImported && framesAlive}
    <div class="frames" bind:clientWidth={framesWidth}>
      {#each allViewports.filter((v) => activeViewports.includes(v.id)) as v (v.id + '-' + reloadKey)}
        <GameFrame
          url={frameUrl(v.id, v.device)}
          game={game.name}
          width={v.width}
          height={v.height}
          label={v.label}
          scale={fitScale(v.width)}
          {speed}
          {muted}
        />
      {/each}
    </div>
  {/if}

  <BookSearch game={game.name} onPlay={forceFrom} />

  <div class="side-panels">
    <LiveEventFeed game={game.name} onReforce={forceFrom} />
    <Bookmarks game={game.name} frontUrl={baseUrl} onPlay={forceFrom} />
  </div>
{/if}

<style>
  .empty {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3) var(--sp-5);
  }

  .empty p {
    margin: 0;
  }

  .banner .dismiss {
    margin-left: auto;
    padding: var(--sp-1);
    line-height: 0;
  }

  .advanced.armed > summary {
    color: var(--warn);
  }

  .qa-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: var(--sp-4) var(--sp-6);
    padding: var(--sp-3) 2px var(--sp-1);
  }

  .qa-block .note {
    margin: 6px 0 0;
    max-width: 46ch;
    font-size: var(--fs-xs);
    color: var(--text-faint);
  }

  .qa-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-top: 2px;
  }

  /* ── Control cards ─────────────────────────────────────────────────── */

  .controls {
    --ctrl-h: 34px;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: var(--sp-3);
    margin-bottom: var(--sp-4);
  }

  .ctl-card {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    min-width: 0;
  }

  .ctl-wide {
    grid-column: 1 / -1;
  }

  .ctl-card > summary {
    display: flex;
    align-items: baseline;
    gap: var(--sp-3);
    padding: 10px 14px;
    list-style: none;
    cursor: pointer;
    user-select: none;
  }

  .ctl-card > summary::-webkit-details-marker {
    display: none;
  }

  .ctl-card > summary::before {
    content: '';
    align-self: center;
    width: 6px;
    height: 6px;
    border-right: 1.5px solid var(--text-faint);
    border-bottom: 1.5px solid var(--text-faint);
    transform: rotate(-45deg);
    transition: transform 0.15s var(--ease-out);
    flex-shrink: 0;
  }

  .ctl-card[open] > summary::before {
    transform: rotate(45deg);
  }

  .ctl-title {
    font-size: var(--fs-md);
    font-weight: 650;
    color: var(--text);
  }

  .ctl-sum {
    margin-left: auto;
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    color: var(--text-dim);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }

  .ctl-body {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    padding: 2px 14px 14px;
  }

  .fields {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--sp-3);
    margin: 0;
  }

  .controls :is(select, input:not([type='checkbox']):not([type='range'])),
  .controls :global(button:not(.vp-remove):not(.reset)) {
    height: var(--ctrl-h);
    box-sizing: border-box;
  }

  .controls label {
    display: flex;
    flex-direction: column;
    gap: 5px;
    min-width: 0;
  }

  .controls label > span:first-child {
    font-size: var(--fs-xs);
    font-weight: 560;
    line-height: 1.1;
    color: var(--text-dim);
    white-space: nowrap;
  }

  .controls label.check {
    flex-direction: row;
    align-items: center;
    gap: 8px;
    height: var(--ctrl-h);
  }

  .controls label.check > span:first-child {
    font-size: var(--fs-sm);
    font-weight: 400;
    color: var(--text);
  }

  .ctl-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
  }

  .pool-note {
    align-self: center;
    font-size: var(--fs-xs);
    color: var(--gold);
  }

  .forced-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--gold);
  }

  /* Viewports: toggle chips, the size under the name. */
  .vp-chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
  }

  .vp-chip {
    display: inline-flex;
    align-items: stretch;
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    background: var(--bg-deep);
    overflow: hidden;
  }

  .vp-chip.on {
    border-color: color-mix(in oklch, var(--gold) 55%, transparent);
    background: color-mix(in oklch, var(--gold) 10%, var(--bg-deep));
  }

  .vp-chip > button:first-child {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    justify-content: center;
    gap: 1px;
    height: auto !important;
    min-height: 40px;
    padding: 5px 12px;
    border: none;
    border-radius: 0;
    background: none;
    text-align: left;
  }

  .vp-name {
    font-size: var(--fs-sm);
    color: var(--text);
  }

  .vp-chip.on .vp-name {
    color: var(--gold);
    font-weight: 620;
  }

  .vp-size {
    font-family: var(--font-mono);
    font-size: var(--fs-2xs);
    color: var(--text-faint);
  }

  .vp-remove {
    padding: 0 8px;
    line-height: 0;
    border: none;
    border-left: 1px solid var(--border);
    border-radius: 0;
    background: none;
  }

  .vp-tools {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--sp-3) var(--sp-4);
    padding-top: var(--sp-3);
    border-top: 1px solid var(--border);
  }

  .custom-add {
    display: flex;
    align-items: center;
    gap: 5px;
    margin: 0;
    color: var(--text-faint);
  }

  .custom-add input {
    width: 76px !important;
  }

  .mute {
    gap: 6px;
  }

  .mute.on {
    color: var(--warn);
  }

  .zoom {
    min-width: 140px;
  }

  .zoom .reset {
    padding: 0 4px;
    margin-left: 4px;
    font-size: var(--fs-xs);
    height: auto;
    min-height: 0;
  }

  .zoom input[type='range'] {
    height: var(--ctrl-h);
    margin: 0;
    padding: 0;
  }

  @media (max-width: 860px) {
    .controls {
      grid-template-columns: 1fr;
      gap: var(--sp-2);
    }

    .controls {
      --ctrl-h: 42px;
    }

    .fields > label:not(.check) {
      flex: 1 1 120px;
    }

    .fields select,
    .fields input:not([type='checkbox']) {
      width: 100% !important;
    }

    .zoom {
      flex: 1 1 140px;
    }
  }

  /* Secondary tools: quieter than the control cards, same fold. */
  .advanced {
    margin-bottom: var(--sp-2);
    border: 1px dashed var(--border);
    border-radius: var(--r-m);
    padding: 0 14px;
  }

  .advanced[open] {
    padding-bottom: 12px;
  }

  .advanced summary {
    padding: 9px 0;
    font-size: var(--fs-sm);
    color: var(--text-dim);
    cursor: pointer;
    user-select: none;
  }

  .advanced summary:hover {
    color: var(--text);
  }

  .advanced-row {
    display: flex;
    gap: var(--sp-2);
    margin: var(--sp-2) 0 0;
  }

  .advanced-row input {
    flex: 1;
    min-width: 0;
    max-width: 560px;
  }

  .frames {
    display: flex;
    flex-wrap: wrap;
    gap: 18px;
    align-items: flex-start;
    margin-bottom: var(--sp-5);
  }

  .side-panels {
    display: grid;
    grid-template-columns: 1.2fr 1fr;
    gap: var(--sp-4);
  }

  @media (max-width: 1000px) {
    .side-panels {
      grid-template-columns: 1fr;
    }
  }
</style>
