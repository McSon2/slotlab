<script lang="ts">
  import { onMount, tick, type Component } from 'svelte';
  import { api } from './lib/api';
  import {
    app,
    dismiss,
    notices,
    refreshGames,
    selectedGame,
    type Tab,
  } from './lib/stores.svelte';
  import { storageGet } from './lib/storage';
  import Games from './pages/Games.svelte';
  import Play from './pages/Play.svelte';
  import Icon, { type IconName } from './components/Icon.svelte';

  /** Every other page is its own chunk, fetched on first visit (chart.js
   * rides along with Analyze and CrowdSim): the first screen only parses
   * Games and Play. The rest is preloaded once the app is idle, so a tab
   * switch never waits on the network in practice. */
  type PageModule = Promise<{ default: Component }>;
  const pageLoaders: Partial<Record<Tab, () => PageModule>> = {
    projects: () => import('./pages/Projects.svelte'),
    drive: () => import('./pages/Drive.svelte'),
    analyze: () => import('./pages/Analyze.svelte'),
    balance: () => import('./pages/Balance.svelte'),
    challenge: () => import('./pages/Challenge.svelte'),
    crowdsim: () => import('./pages/CrowdSim.svelte'),
    scan: () => import('./pages/Scan.svelte'),
    preflight: () => import('./pages/Preflight.svelte'),
    review: () => import('./pages/Review.svelte'),
    assets: () => import('./pages/Assets.svelte'),
    board: () => import('./pages/Board.svelte'),
    publish: () => import('./pages/Publish.svelte'),
  };
  let pages = $state.raw<Partial<Record<Tab, Component>>>({});
  let pageError = $state<string | null>(null);

  function loadPage(tab: Tab) {
    const load = pageLoaders[tab];
    if (!load || pages[tab]) return;
    load()
      .then((m) => (pages = { ...pages, [tab]: m.default }))
      .catch((e) => {
        if (app.tab === tab) pageError = `Could not load this page: ${e?.message ?? e}`;
      });
  }

  $effect(() => {
    pageError = null;
    loadPage(app.tab);
  });

  const Page = $derived(pages[app.tab]);

  /** Grouped by the order work happens in: add → play → math → QA → ship. */
  const groups: { label: string; tabs: { id: Tab; label: string; icon: IconName }[] }[] = [
    {
      label: 'Setup',
      tabs: [
        { id: 'projects', label: 'Projects', icon: 'projects' },
        { id: 'drive', label: 'Drive', icon: 'drive' },
        { id: 'games', label: 'Games', icon: 'games' },
      ],
    },
    {
      label: 'Play',
      tabs: [
        { id: 'play', label: 'Play', icon: 'play' },
        { id: 'board', label: 'Board', icon: 'board' },
      ],
    },
    {
      label: 'Math',
      tabs: [
        { id: 'analyze', label: 'Analyze', icon: 'analyze' },
        { id: 'balance', label: 'Balance', icon: 'balance' },
        { id: 'challenge', label: 'Challenge', icon: 'challenge' },
        { id: 'crowdsim', label: 'CrowdSim', icon: 'crowdsim' },
      ],
    },
    {
      label: 'QA',
      tabs: [
        { id: 'scan', label: 'Scan', icon: 'scan' },
        { id: 'preflight', label: 'Preflight', icon: 'preflight' },
        { id: 'assets', label: 'Assets', icon: 'assets' },
        { id: 'review', label: 'Review', icon: 'review' },
      ],
    },
    { label: 'Ship', tabs: [{ id: 'publish', label: 'Publish', icon: 'publish' }] },
  ];

  /** One line under each page title: what the page answers. */
  const pageIntro: Record<Tab, string> = {
    projects: 'Every slot project on the VPS: where its math lives, what it weighs, and what to archive.',
    drive: 'Finished slots stored on Google Drive, ready to come back to the VPS.',
    games: 'The games SlotLab knows: where their books are, and what each mode pays.',
    play: 'Play the build against the emulated RGS, in every screen size at once.',
    board: 'The board the game shows before the first spin: edit it, check it, write it back.',
    analyze: 'What a player lives through, mode by mode — and what to fix first.',
    balance: 'Re-weight the lookup table toward an RTP and a volatility, without new books.',
    challenge: 'Stake challenges: what each target costs and what it pays the hunter.',
    crowdsim: 'Thousands of simulated players: how sessions really end.',
    scan: 'Check every published book against the rules of the game.',
    preflight: 'Everything Stake reviews, checked before you submit.',
    assets: 'What the player downloads and what the textures cost in memory.',
    review: 'The figures to paste into the Stake submission, and a client report.',
    publish: 'Share a playable link with the team, or publish to the homelab.',
  };
  const currentTab = $derived(groups.flatMap((g) => g.tabs).find((t) => t.id === app.tab));

  let tlsTrusted = $state(true);
  /** Phone/tablet: the nav collapses behind a menu button. */
  let navOpen = $state(false);
  let showOnboarding = $state(false);
  let helpBtn = $state<HTMLButtonElement>();
  /** Play stays mounted once opened: switching tabs must not reload the games. */
  let playMounted = $state(app.tab === 'play');

  $effect(() => {
    if (app.tab === 'play') playMounted = true;
  });

  onMount(() => {
    // First-run setup guide: no game registered and never completed/skipped.
    refreshGames().then(() => {
      if (app.games.length === 0) {
        if (!app.tabFromUrl) app.tab = 'games';
        if (!storageGet('slotlab.onboarded')) showOnboarding = true;
      }
    });
    api.tlsStatus().then((s) => (tlsTrusted = s.trusted)).catch(() => {});
    const preload = () => (Object.keys(pageLoaders) as Tab[]).forEach(loadPage);
    // WebKit (the Tauri webview on macOS) has no requestIdleCallback.
    const ric = typeof requestIdleCallback === 'function';
    const idle = ric ? requestIdleCallback(preload, { timeout: 3000 }) : setTimeout(preload, 1500);
    // Refresh the list while imports are running (until "ready" status).
    const iv = setInterval(() => {
      if (app.games.some((g) => g.loaded && !g.imported)) void refreshGames();
    }, 2500);
    return () => {
      clearInterval(iv);
      if (ric) cancelIdleCallback(idle as number);
      else clearTimeout(idle);
    };
  });

  const game = $derived(selectedGame());
  const pendingImports = $derived(
    app.games.filter((g) => g.loaded && !g.imported).length,
  );
</script>

{#if showOnboarding}
  <!-- First run only: not worth a place in the main chunk. -->
  {#await import('./components/Onboarding.svelte') then { default: Onboarding }}
    <Onboarding
      trusted={tlsTrusted}
      onclose={() => {
        showOnboarding = false;
        // After the shell loses `inert`, or the focus call is ignored.
        void tick().then(() => helpBtn?.focus());
        api.tlsStatus().then((s) => (tlsTrusted = s.trusted)).catch(() => {});
      }}
    />
  {/await}
{/if}

<div class="shell" inert={showOnboarding}>
  <a class="skip-link" href="#main">Skip to content</a>
  <aside>
    <div class="brand">
      <span class="chip" aria-hidden="true">SL</span>
      <span class="brand-name">SlotLab</span>
      <button
        class="menu-btn"
        aria-label={navOpen ? 'Close the menu' : 'Open the menu'}
        aria-expanded={navOpen}
        aria-controls="main-nav"
        onclick={() => (navOpen = !navOpen)}
      >
        <span class="menu-current">{groups.flatMap((g) => g.tabs).find((t) => t.id === app.tab)?.label ?? ''}</span>
        <Icon name={navOpen ? 'close' : 'menu'} size={16} />
      </button>
    </div>
    <nav aria-label="Main" id="main-nav" class:open={navOpen}>
      {#each groups as g (g.label)}
        <div class="nav-group" role="group" aria-labelledby="nav-{g.label}">
          <span class="nav-label" id="nav-{g.label}">{g.label}</span>
          {#each g.tabs as t (t.id)}
            <button
              class:active={app.tab === t.id}
              aria-current={app.tab === t.id ? 'page' : undefined}
              onclick={() => {
                app.tab = t.id;
                navOpen = false;
              }}
            >
              <Icon name={t.icon} size={15} />
              {t.label}
            </button>
          {/each}
        </div>
      {/each}
    </nav>
    <div class="rail-foot">
      {#if !tlsTrusted}
        <span class="pill yellow" title="Local certificate not installed — see the Play tab">
          CA<span class="sr-only"> certificate not installed</span>
        </span>
      {:else}
        <span class="pill green" title="Local certificate installed">
          CA<span class="sr-only"> certificate installed</span>
        </span>
      {/if}
      {#if pendingImports > 0}
        <span class="pill yellow" title="{pendingImports} game(s) importing">
          {pendingImports}<span class="sr-only"> game(s) importing</span>
        </span>
      {/if}
      <button
        class="help-btn"
        title="Reopen the setup guide"
        aria-label="Reopen the setup guide"
        bind:this={helpBtn}
        onclick={() => (showOnboarding = true)}
      >
        <Icon name="help" size={14} />
      </button>
    </div>
  </aside>

  <div class="main-col">
    <header>
      <div class="cartridge">
        <label class="cartridge-label" for="game-select">Game</label>
        {#if app.games.length > 0}
          <select id="game-select" bind:value={app.selectedGame}>
            {#each app.games as g (g.name)}
              <option value={g.name}>{g.name}</option>
            {/each}
          </select>
        {:else}
          <span class="dim">no game yet —</span>
          {#if app.tab !== 'games'}
            <button class="ghost" onclick={() => (app.tab = 'games')}>Add one</button>
          {/if}
        {/if}
        {#if game}
          <span class="cartridge-meta">
            {game.modes.length} modes
            {#if game.loaded && !game.imported}
              <span class="pill yellow">importing…</span>
            {:else if game.imported}
              <span class="pill green">ready</span>
            {/if}
          </span>
        {/if}
      </div>
    </header>

    <main class="page" id="main" tabindex="-1">
      {#if currentTab}
        <div class="page-head">
          <h1>{currentTab.label}</h1>
          <p>{pageIntro[app.tab]}</p>
        </div>
      {/if}
      {#if app.error}
        <div class="banner error" role="alert">{app.error}</div>
      {/if}
      {#if playMounted}
        <div class="play-host" hidden={app.tab !== 'play'}>
          <Play active={app.tab === 'play'} />
        </div>
      {/if}
      {#if app.tab === 'games'}
        <Games />
      {:else if Page}
        <Page />
      {:else if pageError}
        <div class="banner error" role="alert">{pageError}</div>
      {:else if pageLoaders[app.tab]}
        <p class="muted" aria-busy="true">Loading…</p>
      {/if}
    </main>
  </div>
</div>

<div class="notices" aria-live="polite">
  {#each notices as n (n.id)}
    <div class="notice {n.kind}" role={n.kind === 'error' ? 'alert' : 'status'}>
      <span>{n.text}</span>
      <button class="ghost" aria-label="Dismiss" onclick={() => dismiss(n.id)}>
        <Icon name="close" size={13} />
      </button>
    </div>
  {/each}
</div>

<style>
  .shell {
    display: flex;
    min-height: 100vh;
  }

  .skip-link {
    position: absolute;
    left: var(--sp-2);
    top: -40px;
    z-index: 100;
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--r-s);
    background: var(--gold);
    color: var(--gold-ink);
    font-weight: 680;
    text-decoration: none;
  }

  .skip-link:focus {
    top: var(--sp-2);
  }

  aside {
    width: 168px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    padding: var(--sp-4) 10px;
    border-right: 1px solid var(--border);
    background: var(--bg-deep);
    position: sticky;
    top: 0;
    height: 100vh;
    overflow-y: auto;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 2px 8px var(--sp-4);
  }

  .chip {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: var(--gold);
    color: var(--gold-ink);
    font-size: var(--fs-2xs);
    font-weight: 800;
    letter-spacing: 0.02em;
    box-shadow:
      0 0 0 2.5px var(--bg-deep),
      0 0 0 3.5px color-mix(in oklch, var(--gold) 40%, transparent);
  }

  .brand-name {
    font-weight: 700;
    font-size: var(--fs-md);
    letter-spacing: -0.01em;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    flex: 1;
  }

  .nav-group {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .nav-label {
    font-size: 10px;
    font-weight: 620;
    text-transform: uppercase;
    letter-spacing: 0.14em;
    color: var(--text-faint);
    padding: 0 10px var(--sp-1);
  }

  nav button {
    display: flex;
    align-items: center;
    gap: 10px;
    background: none;
    border: none;
    color: var(--text-dim);
    padding: 7px 10px;
    border-radius: var(--r-s);
    font-size: 13px;
    text-align: left;
  }

  nav button :global(.icon) {
    opacity: 0.75;
  }

  nav button:hover {
    color: var(--text);
    background: var(--bg-panel);
  }

  nav button.active {
    color: var(--gold);
    background: color-mix(in oklch, var(--gold) 8%, transparent);
    font-weight: 640;
  }

  nav button.active :global(.icon) {
    opacity: 1;
  }

  .rail-foot {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: var(--sp-2);
  }

  .help-btn {
    margin-left: auto;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border-radius: 50%;
    color: var(--text-faint);
    background: none;
    border: 1px solid var(--border);
  }

  .help-btn:hover {
    color: var(--text);
    border-color: var(--border-strong);
  }

  .main-col {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  header {
    display: flex;
    align-items: center;
    padding: 10px var(--sp-6);
    border-bottom: 1px solid var(--border);
    background: oklch(0.17 0.013 165 / 0.85);
    backdrop-filter: blur(6px);
    position: sticky;
    top: 0;
    z-index: 10;
  }

  .cartridge {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .cartridge-label {
    font-size: var(--fs-2xs);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--text-faint);
  }

  .cartridge select {
    font-weight: 640;
    font-size: 14px;
    background: var(--bg-panel);
    border-color: var(--border-strong);
    padding: 5px 10px;
  }

  .cartridge-meta {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: 12px;
    color: var(--text-faint);
  }

  main {
    padding: 22px var(--sp-6) 40px;
    max-width: 1680px;
    width: 100%;
    margin: 0 auto;
  }

  main:focus {
    outline: none;
  }

  .page-head {
    margin: 0 0 var(--sp-4);
  }

  .page-head h1 {
    margin: 0;
    font-size: var(--fs-xl);
    font-weight: 700;
    letter-spacing: -0.015em;
    line-height: 1.2;
  }

  .page-head p {
    margin: 4px 0 0;
    max-width: 70ch;
    font-size: var(--fs-base);
    color: var(--text-dim);
  }

  .notices {
    position: fixed;
    right: var(--sp-5);
    bottom: var(--sp-5);
    z-index: 50;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    max-width: min(440px, calc(100vw - 40px));
    pointer-events: none;
  }

  .notice {
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
    padding: 10px 10px 10px 14px;
    border-radius: var(--r-m);
    border: 1px solid color-mix(in oklch, var(--pass) 40%, transparent);
    background: color-mix(in oklch, var(--pass) 10%, var(--bg-raised));
    box-shadow: 0 8px 28px oklch(0 0 0 / 0.4);
    font-size: var(--fs-sm);
  }

  .notice.info {
    border-color: color-mix(in oklch, var(--info) 40%, transparent);
    background: color-mix(in oklch, var(--info) 10%, var(--bg-raised));
  }

  .notice.error {
    border-color: color-mix(in oklch, var(--fail) 50%, transparent);
    background: color-mix(in oklch, var(--fail) 12%, var(--bg-raised));
    color: var(--text);
  }

  .notice span {
    flex: 1;
    padding-top: 3px;
    overflow-wrap: anywhere;
  }

  .notice button {
    padding: 4px;
    line-height: 0;
  }

  .menu-btn {
    display: none;
  }

  /* Phone / small tablet: the rail becomes a top bar, the nav a drop-down. */
  @media (max-width: 860px) {
    .shell {
      flex-direction: column;
    }

    aside {
      width: 100%;
      height: auto;
      padding: 8px var(--sp-3);
      border-right: none;
      border-bottom: 1px solid var(--border);
      z-index: 30;
      overflow: visible;
    }

    .brand {
      padding: 0;
    }

    .menu-btn {
      display: flex;
      align-items: center;
      gap: 8px;
      margin-left: auto;
      min-height: 40px;
      padding: 6px 12px;
      border-radius: var(--r-s);
      border: 1px solid var(--border);
      background: var(--bg-panel);
      color: var(--text);
      font-size: 14px;
      font-weight: 620;
    }

    nav {
      display: none;
      position: absolute;
      left: 0;
      right: 0;
      top: 100%;
      max-height: calc(100dvh - 60px);
      overflow-y: auto;
      padding: var(--sp-3);
      background: var(--bg-deep);
      border-bottom: 1px solid var(--border);
      box-shadow: 0 16px 32px oklch(0 0 0 / 0.45);
      grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
      gap: var(--sp-3);
    }

    nav.open {
      display: grid;
    }

    nav button {
      width: 100%;
      min-height: 44px;
      font-size: 15px;
    }

    .rail-foot {
      display: none;
    }

    header {
      top: 0;
      padding: 8px var(--sp-3);
    }

    .cartridge {
      flex-wrap: wrap;
      gap: 6px 10px;
      min-width: 0;
    }

    .cartridge select {
      max-width: 100%;
      font-size: 16px; /* below 16px iOS zooms into the field */
    }

    main {
      padding: 14px var(--sp-3) 32px;
    }

    .notices {
      left: var(--sp-3);
      right: var(--sp-3);
      bottom: var(--sp-3);
      max-width: none;
    }
  }

  @media (prefers-reduced-motion: no-preference) {
    .notice {
      animation: notice-in 0.25s var(--ease-out);
    }
  }

  @keyframes notice-in {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }
</style>
