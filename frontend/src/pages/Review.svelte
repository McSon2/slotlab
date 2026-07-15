<script lang="ts">
  import { api } from '../lib/api';
  import { app } from '../lib/stores.svelte';

  let text = $state('');
  let error = $state<string | null>(null);
  let copied = $state(false);

  $effect(() => {
    if (app.selectedGame) void load(app.selectedGame);
  });

  async function load(gameName: string) {
    error = null;
    try {
      const r = await api.reviewIds(gameName);
      text = r.text;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      text = '';
    }
  }

  async function copy() {
    await navigator.clipboard.writeText(text);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }
</script>

<div class="panel">
  <div class="head">
    <h2>Event IDs for the Stake review</h2>
    <button class="primary" onclick={copy} disabled={!text}>
      {copied ? 'Copied ✓' : 'Copy'}
    </button>
  </div>
  <p class="dim">
    Automatic selection per mode: mean win / big win (p99.9) / max win,
    from the actually published LUTs. ⚠️ A future <code>run.py prod</code>
    re-sorts the sims and changes the IDs — regenerate this block from the
    books actually uploaded.
  </p>
  {#if error}
    <p style="color: var(--red);">{error}</p>
  {:else}
    <pre>{text}</pre>
  {/if}
</div>

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }

  pre {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 16px;
    font-family: var(--mono);
    font-size: 13px;
    overflow-x: auto;
  }
</style>
