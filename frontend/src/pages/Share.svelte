<script lang="ts">
  import { onMount } from 'svelte';
  import { app, selectedGame } from '../lib/stores.svelte';

  interface ShareStatus {
    active: boolean;
    game?: string;
    url?: string;
    playUrl?: string;
  }

  let status = $state<ShareStatus>({ active: false });
  let buildDir = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);
  let copied = $state(false);

  const game = $derived(selectedGame());

  onMount(() => {
    void refresh();
  });

  $effect(() => {
    // Build path suggestion from the game's publish_dir.
    if (game && !buildDir) {
      const m = game.publishDir.match(/^(.*)\/math\/games\//);
      if (m) buildDir = `${m[1]}/frontend/apps/${gameFolderGuess(m[1])}/build`;
    }
  });

  function gameFolderGuess(repoRoot: string): string {
    // apps/<x> often carries the game_id name; the user fixes it if needed.
    return game?.name ?? '';
  }

  async function refresh() {
    status = await fetch('/api/ui/share/status').then((r) => r.json());
  }

  async function start() {
    if (!app.selectedGame || !buildDir.trim()) {
      error = 'Provide the game build folder.';
      return;
    }
    busy = true;
    error = null;
    try {
      const r = await fetch('/api/ui/share/start', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ game: app.selectedGame, buildDir: buildDir.trim() }),
      }).then((r) => r.json());
      if (r.error) throw new Error(r.error);
      await refresh();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function stop() {
    await fetch('/api/ui/share/stop', { method: 'POST' });
    await refresh();
  }

  async function copy() {
    if (!status.playUrl) return;
    await navigator.clipboard.writeText(status.playUrl);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }
</script>

<div class="panel">
  <h2>Share the slot (temporary public link)</h2>
  <p class="dim">
    Serves the game's <strong>static build</strong> + the emulated RGS behind a
    Cloudflare quick tunnel (<code>*.trycloudflare.com</code>, free, no
    account). Your friend installs nothing: they open the link and play with a
    demo balance. The link dies when you stop sharing or close SlotLab.
    The SlotLab UI is never exposed. Prerequisites:
    <code>brew install cloudflared</code> + imported books + game build
    (<code>pnpm --filter &lt;game&gt; build</code>).
  </p>

  {#if status.active}
    <div class="active-share">
      <p>
        <span class="pill green">ACTIVE</span>
        <strong>{status.game}</strong>
      </p>
      <div class="link-row">
        <input readonly value={status.playUrl} />
        <button class="primary" onclick={copy}>{copied ? 'Copied ✓' : 'Copy link'}</button>
        <a href={status.playUrl} target="_blank" rel="noreferrer">
          <button>Open</button>
        </a>
        <button class="danger" onclick={stop}>Stop</button>
      </div>
    </div>
  {:else if game}
    <div class="form">
      <label>
        <span>Game</span>
        <input readonly value={game.name} />
      </label>
      <label class="wide">
        <span>Frontend build folder</span>
        <input
          bind:value={buildDir}
          placeholder="/…/frontend/apps/{game.name}/build"
        />
      </label>
      <button class="primary" onclick={start} disabled={busy}>
        {busy ? 'Starting the tunnel…' : 'Start sharing'}
      </button>
    </div>
  {:else}
    <p class="dim">Select a loaded game.</p>
  {/if}

  {#if error}
    <p style="color: var(--red);">{error}</p>
  {/if}
</div>

<style>
  .form {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    align-items: flex-end;
  }

  .form label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    color: var(--text-dim);
  }

  .form label.wide {
    flex: 1;
    min-width: 380px;
  }

  .active-share .link-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .link-row input {
    flex: 1;
    font-family: var(--mono);
    font-size: 12px;
  }
</style>
