<script lang="ts">
  import { onMount } from 'svelte';
  import { api, rgsUrlFor, type ImportJob } from '../lib/api';
  import { app, refreshGames, selectedGame } from '../lib/stores.svelte';
  import {
    VIEWPORT_PRESETS,
    CURRENCIES,
    SOCIAL_CURRENCIES,
    LANGUAGES,
    type ViewportPreset,
  } from '../lib/viewports';
  import GameFrame from '../components/GameFrame.svelte';
  import LiveEventFeed from '../components/LiveEventFeed.svelte';
  import Bookmarks from '../components/Bookmarks.svelte';
  import BookSearch from '../components/BookSearch.svelte';

  const PREFS_KEY = 'slotlab-play-prefs';

  interface PlayPrefs {
    activeViewports: string[];
    currency: string;
    social: boolean;
    lang: string;
    balance: number;
    scalePct: number;
    customViewports: ViewportPreset[];
  }

  function loadPrefs(): Partial<PlayPrefs> {
    try {
      return JSON.parse(localStorage.getItem(PREFS_KEY) ?? '{}');
    } catch {
      return {};
    }
  }

  const saved = loadPrefs();

  let tlsTrusted = $state(true);
  let installBusy = $state(false);

  let activeViewports = $state<string[]>(saved.activeViewports ?? ['desktop', 'mobile-m']);
  let currency = $state(saved.currency ?? 'USD');
  let social = $state(saved.social ?? false);
  // Prefs inherited from a version without social filtering: realign currency ↔ social.
  if (social !== SOCIAL_CURRENCIES.includes(currency)) {
    currency = social ? SOCIAL_CURRENCIES[0] : 'USD';
  }
  let lang = $state(saved.lang ?? 'en');
  let balance = $state(saved.balance ?? 10000);
  let scalePct = $state(saved.scalePct ?? 60);
  let customViewports = $state<ViewportPreset[]>(saved.customViewports ?? []);
  let customW = $state('');
  let customH = $state('');

  let forceMode = $state('');
  let forceSim = $state('');
  let forceActive = $state(false);

  let epoch = $state(1);
  let reloadKey = $state(0);
  /** Game the frames are running for (auto-launch on change). */
  let launchedFor = $state<string | null>(null);

  let importInfo = $state<{ imported: boolean; job: ImportJob | null } | null>(null);
  /** Custom URL (Vite dev). Empty = build served by SlotLab (auto). */
  let customUrl = $state('');
  let statusMsg = $state<string | null>(null);

  const game = $derived(selectedGame());
  const baseUrl = $derived(
    customUrl.trim() ? customUrl.trim().replace(/\/$/, '') : (game?.servedUrl ?? ''),
  );
  const canPlay = $derived(Boolean(game?.loaded && baseUrl));

  const allViewports = $derived([...VIEWPORT_PRESETS, ...customViewports]);

  $effect(() => {
    const prefs: PlayPrefs = {
      activeViewports, currency, social, lang, balance, scalePct, customViewports,
    };
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  });

  onMount(() => {
    api.tlsStatus().then((s) => (tlsTrusted = s.trusted)).catch(() => {});
  });

  // Auto-launch: as soon as the selected game is imported and playable.
  let seenGame = $state<string | null>(null);
  $effect(() => {
    if (!game) return;
    if (seenGame !== game.name) {
      // Game change: restore its persisted custom URL.
      seenGame = game.name;
      customUrl = game.frontUrl ?? '';
      void pollImport();
    }
    if (game.imported && canPlay && launchedFor !== game.name) {
      launchedFor = game.name;
      void launch();
    }
  });

  async function pollImport() {
    if (!app.selectedGame) return;
    try {
      importInfo = await api.importStatus(app.selectedGame);
      if (importInfo.job?.state === 'running' || !importInfo.imported) {
        setTimeout(pollImport, 900);
      } else if (importInfo.imported && game && launchedFor !== game.name && canPlay) {
        launchedFor = game.name;
        void launch();
      }
    } catch {
      importInfo = null;
    }
  }

  async function installCa() {
    installBusy = true;
    try {
      const r = await api.tlsInstall();
      tlsTrusted = r.trusted;
    } catch (e) {
      statusMsg = e instanceof Error ? e.message : String(e);
    } finally {
      installBusy = false;
    }
  }

  function sessionId(viewportId: string): string {
    return `slotlab-${app.selectedGame}-${viewportId}-${epoch}`;
  }

  function frameUrl(viewportId: string, device: string): string {
    if (!game) return '';
    const params = new URLSearchParams({
      sessionID: sessionId(viewportId),
      rgs_url: rgsUrlFor(game.name),
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
    statusMsg = null;
    if (customUrl.trim() !== (game.frontUrl ?? '')) {
      await api.updateFrontUrl(game.name, customUrl.trim());
      await refreshGames();
    }
    await Promise.all(
      activeViewports.map((v) =>
        api.prepareSession({
          sessionId: sessionId(v),
          game: game.name,
          balance: Math.round(balance * 1_000_000),
          currency,
          social,
          language: lang,
        }),
      ),
    );
    reloadKey += 1;
  }

  async function resetSessions() {
    if (!game) return;
    await api.resetSessions(game.name);
    epoch += 1;
    await launch();
  }

  async function applyForce() {
    if (!game || !forceSim.trim()) return;
    await api.setForce({
      game: game.name,
      mode: forceMode || game.modes[0]?.name,
      simId: Number(forceSim),
    });
    forceActive = true;
  }

  async function clearForce() {
    if (!game) return;
    await api.setForce({ game: game.name });
    forceActive = false;
  }

  function toggleViewport(id: string) {
    activeViewports = activeViewports.includes(id)
      ? activeViewports.filter((v) => v !== id)
      : [...activeViewports, id];
    void launch();
  }

  function addCustomViewport() {
    const w = Number(customW);
    const h = Number(customH);
    if (!w || !h || w < 100 || h < 100) return;
    const id = `custom-${w}x${h}`;
    if (allViewports.some((v) => v.id === id)) return;
    customViewports = [
      ...customViewports,
      { id, label: `${w}×${h}`, width: w, height: h, device: w >= h ? 'desktop' : 'mobile' },
    ];
    activeViewports = [...activeViewports, id];
    customW = '';
    customH = '';
    void launch();
  }

  function removeCustomViewport(id: string) {
    customViewports = customViewports.filter((v) => v.id !== id);
    activeViewports = activeViewports.filter((v) => v !== id);
  }

  function forceFrom(mode: string, simId: number) {
    forceMode = mode;
    forceSim = String(simId);
    forceActive = true;
    void api.setForce({ game: game!.name, mode, simId });
  }

  const importPct = $derived(
    importInfo?.job && importInfo.job.bytesTotal > 0
      ? Math.round((importInfo.job.bytesDone / importInfo.job.bytesTotal) * 100)
      : 0,
  );
</script>

{#if !tlsTrusted}
  <div class="banner">
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
  <div class="panel">
    <p class="dim">
      No game. <strong>Games</strong> tab → add a
      <code>publish_files</code> folder — everything else is automatic.
    </p>
  </div>
{:else}
  {#if importInfo && !importInfo.imported}
    <div class="banner">
      {#if importInfo.job?.state === 'failed'}
        <span>Import failed: {importInfo.job.error}</span>
        <button onclick={() => api.startImport(game.name).then(pollImport)}>Retry</button>
      {:else}
        <span>
          Preparing books for <strong>{game.name}</strong>
          {#if importInfo.job?.currentMode}
            — {importInfo.job.currentMode} ({importInfo.job.modesDone}/{importInfo.job.modesTotal})
          {/if}
          <span class="dim">· the game will launch on its own</span>
        </span>
        <progress max="100" value={importPct} style="flex:1"></progress>
        <span class="num">{importPct}%</span>
      {/if}
    </div>
  {/if}

  {#if !game.servedUrl && !customUrl}
    <div class="banner">
      <span>
        No frontend build detected for <strong>{game.name}</strong> —
        build it (<code>pnpm --filter {game.name} build</code>) then
        "Reload" in Games, or provide a Vite dev URL below.
      </span>
    </div>
  {/if}

  <section class="controls-strip">
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
      <input type="number" bind:value={balance} min="0" step="100" style="width: 90px" onchange={launch} />
    </label>
    <label class="check">
      <input type="checkbox" bind:checked={social} onchange={onSocialChange} />
      <span>Social</span>
    </label>
    <span class="sep"></span>
    <label>
      <span>Force · mode</span>
      <select bind:value={forceMode}>
        {#each game.modes as m (m.name)}<option value={m.name}>{m.name}</option>{/each}
      </select>
    </label>
    <label>
      <span>Event ID</span>
      <input bind:value={forceSim} placeholder="50931" style="width: 90px" />
    </label>
    <button onclick={applyForce}>Force</button>
    {#if forceActive}
      <button class="ghost" onclick={clearForce}>✕ {forceMode} #{forceSim}</button>
    {/if}
    <span class="sep"></span>
    <button onclick={launch}>Reload</button>
    <button class="ghost" onclick={resetSessions}>Reset sessions</button>
  </section>

  <section class="controls-strip viewports">
    <span class="strip-label">Viewports</span>
    {#each allViewports as v (v.id)}
      <label class="check">
        <input
          type="checkbox"
          checked={activeViewports.includes(v.id)}
          onchange={() => toggleViewport(v.id)}
        />
        <span>{v.label}</span>
        {#if v.id.startsWith('custom-')}
          <button class="ghost tiny" onclick={() => removeCustomViewport(v.id)}>✕</button>
        {/if}
      </label>
    {/each}
    <span class="custom-add">
      <input style="width: 56px" bind:value={customW} placeholder="800" />
      ×
      <input style="width: 56px" bind:value={customH} placeholder="450" />
      <button class="ghost" onclick={addCustomViewport}>+ custom</button>
    </span>
    <label class="zoom">
      <span>Zoom {scalePct}%</span>
      <input type="range" min="25" max="100" step="5" bind:value={scalePct} />
    </label>
  </section>

  <details class="advanced">
    <summary>Custom frontend URL (Vite dev, hot-reload)</summary>
    <div class="advanced-row">
      <input
        bind:value={customUrl}
        placeholder="http://localhost:5174 — empty = build served by SlotLab ({game.servedUrl ?? 'none'})"
      />
      <button onclick={launch}>Apply</button>
      {#if customUrl}
        <button class="ghost" onclick={() => { customUrl = ''; void launch(); }}>
          Back to auto build
        </button>
      {/if}
    </div>
  </details>

  {#if statusMsg}
    <div class="banner error">{statusMsg}</div>
  {/if}

  {#if canPlay && game.imported}
    <div class="frames">
      {#each allViewports.filter((v) => activeViewports.includes(v.id)) as v (v.id + '-' + reloadKey)}
        <GameFrame
          url={frameUrl(v.id, v.device)}
          width={v.width}
          height={v.height}
          label={v.label}
          scale={scalePct / 100}
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
  .controls-strip {
    --ctrl-h: 32px;
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 10px 14px;
    padding: 10px 0 12px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 12px;
  }

  /* One control height for everything → bottoms line up with align-items: flex-end. */
  .controls-strip :is(select, button:not(.tiny), input:not([type='checkbox']):not([type='range'])) {
    height: var(--ctrl-h);
    box-sizing: border-box;
  }

  .controls-strip label {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .controls-strip label > span:first-child {
    font-size: 10px;
    line-height: 1;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-faint);
    white-space: nowrap;
  }

  .controls-strip label.check {
    flex-direction: row;
    align-items: center;
    gap: 6px;
    height: var(--ctrl-h);
    font-size: 12.5px;
    color: var(--text-dim);
  }

  .sep {
    width: 1px;
    align-self: stretch;
    background: var(--border);
    margin: 0 4px;
  }

  .strip-label {
    display: inline-flex;
    align-items: center;
    height: var(--ctrl-h);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--text-faint);
  }

  .viewports {
    border-bottom: none;
    padding-top: 0;
    margin-bottom: 4px;
  }

  .custom-add {
    display: flex;
    align-items: center;
    height: var(--ctrl-h);
    gap: 5px;
    color: var(--text-faint);
  }

  .zoom {
    margin-left: auto;
  }

  .zoom input[type='range'] {
    height: var(--ctrl-h);
    margin: 0;
    padding: 0;
  }

  .tiny {
    padding: 0 4px;
    font-size: 10px;
  }

  .advanced {
    margin-bottom: 14px;
  }

  .advanced summary {
    font-size: 11.5px;
    color: var(--text-faint);
    cursor: pointer;
    user-select: none;
  }

  .advanced summary:hover {
    color: var(--text-dim);
  }

  .advanced-row {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }

  .advanced-row input {
    flex: 1;
    max-width: 560px;
  }

  .frames {
    display: flex;
    flex-wrap: wrap;
    gap: 18px;
    align-items: flex-start;
    margin-bottom: 20px;
  }

  .side-panels {
    display: grid;
    grid-template-columns: 1.2fr 1fr;
    gap: 16px;
  }

  @media (max-width: 1000px) {
    .side-panels {
      grid-template-columns: 1fr;
    }
  }
</style>
