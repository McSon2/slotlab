<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from './lib/api';
  import { app, refreshGames, selectedGame, type Tab } from './lib/stores.svelte';
  import Games from './pages/Games.svelte';
  import Play from './pages/Play.svelte';
  import Analyze from './pages/Analyze.svelte';
  import Balance from './pages/Balance.svelte';
  import CrowdSim from './pages/CrowdSim.svelte';
  import Review from './pages/Review.svelte';
  import Share from './pages/Share.svelte';
  import Onboarding from './components/Onboarding.svelte';

  const tabs: { id: Tab; label: string; glyph: string }[] = [
    { id: 'play', label: 'Play', glyph: '▶' },
    { id: 'analyze', label: 'Analyze', glyph: '∑' },
    { id: 'balance', label: 'Balance', glyph: '⚖' },
    { id: 'crowdsim', label: 'CrowdSim', glyph: '⛁' },
    { id: 'review', label: 'Review', glyph: '★' },
    { id: 'share', label: 'Share', glyph: '⇗' },
    { id: 'games', label: 'Games', glyph: '☰' },
  ];

  let tlsTrusted = $state(true);
  let showOnboarding = $state(false);

  onMount(() => {
    // First-run setup guide: no game registered and never completed/skipped.
    refreshGames().then(() => {
      if (app.games.length === 0 && !localStorage.getItem('slotlab.onboarded')) {
        showOnboarding = true;
      }
    });
    api.tlsStatus().then((s) => (tlsTrusted = s.trusted)).catch(() => {});
    // Refresh the list while imports are running (until "ready" status).
    const iv = setInterval(() => {
      if (app.games.some((g) => g.loaded && !g.imported)) void refreshGames();
    }, 2500);
    return () => clearInterval(iv);
  });

  const game = $derived(selectedGame());
  const pendingImports = $derived(
    app.games.filter((g) => g.loaded && !g.imported).length,
  );
</script>

{#if showOnboarding}
  <Onboarding
    trusted={tlsTrusted}
    onclose={() => {
      showOnboarding = false;
      api.tlsStatus().then((s) => (tlsTrusted = s.trusted)).catch(() => {});
    }}
  />
{/if}

<div class="shell">
  <aside>
    <div class="brand" title="SlotLab">
      <span class="chip">SL</span>
      <span class="brand-name">SlotLab</span>
    </div>
    <nav>
      {#each tabs as t (t.id)}
        <button
          class:active={app.tab === t.id}
          onclick={() => (app.tab = t.id)}
        >
          <span class="glyph">{t.glyph}</span>
          {t.label}
        </button>
      {/each}
    </nav>
    <div class="rail-foot">
      {#if !tlsTrusted}
        <span class="pill yellow" title="Local CA not installed — Play tab">CA</span>
      {:else}
        <span class="pill green" title="Local CA installed">CA</span>
      {/if}
      {#if pendingImports > 0}
        <span class="pill yellow" title="{pendingImports} game(s) importing">
          {pendingImports}
        </span>
      {/if}
      <button
        class="help-btn"
        title="Reopen the setup guide"
        onclick={() => (showOnboarding = true)}
      >?</button>
    </div>
  </aside>

  <div class="main-col">
    <header>
      <div class="cartridge">
        <span class="cartridge-label">Game</span>
        {#if app.games.length > 0}
          <select bind:value={app.selectedGame}>
            {#each app.games as g (g.name)}
              <option value={g.name}>{g.name}</option>
            {/each}
          </select>
        {:else}
          <span class="dim">no game</span>
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

    <main class="page">
      {#if app.error}
        <div class="banner error">{app.error}</div>
      {/if}
      {#if app.tab === 'games'}
        <Games />
      {:else if app.tab === 'play'}
        <Play />
      {:else if app.tab === 'analyze'}
        <Analyze />
      {:else if app.tab === 'balance'}
        <Balance />
      {:else if app.tab === 'crowdsim'}
        <CrowdSim />
      {:else if app.tab === 'review'}
        <Review />
      {:else if app.tab === 'share'}
        <Share />
      {/if}
    </main>
  </div>
</div>

<style>
  .shell {
    display: flex;
    min-height: 100vh;
  }

  aside {
    width: 168px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    padding: 16px 10px;
    border-right: 1px solid var(--border);
    background: var(--bg-deep);
    position: sticky;
    top: 0;
    height: 100vh;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 2px 8px 18px;
  }

  .chip {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: var(--gold);
    color: var(--gold-ink);
    font-size: 10.5px;
    font-weight: 800;
    letter-spacing: 0.02em;
    box-shadow:
      0 0 0 2.5px var(--bg-deep),
      0 0 0 3.5px oklch(0.82 0.13 88 / 0.4);
  }

  .brand-name {
    font-weight: 700;
    font-size: 14.5px;
    letter-spacing: -0.01em;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
  }

  nav button {
    display: flex;
    align-items: center;
    gap: 10px;
    background: none;
    border: none;
    color: var(--text-dim);
    padding: 8px 10px;
    border-radius: var(--r-s);
    font-size: 13px;
    text-align: left;
  }

  nav button .glyph {
    width: 16px;
    text-align: center;
    font-size: 12px;
    opacity: 0.7;
  }

  nav button:hover {
    color: var(--text);
    background: var(--bg-panel);
  }

  nav button.active {
    color: var(--gold);
    background: oklch(0.82 0.13 88 / 0.08);
    font-weight: 640;
  }

  nav button.active .glyph {
    opacity: 1;
  }

  .rail-foot {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px;
  }

  .help-btn {
    margin-left: auto;
    width: 22px;
    height: 22px;
    padding: 0;
    border-radius: 50%;
    font-size: 11.5px;
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
    padding: 10px 24px;
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
    font-size: 10.5px;
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
    gap: 8px;
    font-size: 12px;
    color: var(--text-faint);
  }

  main {
    padding: 22px 24px 40px;
    max-width: 1680px;
    width: 100%;
    margin: 0 auto;
  }
</style>
