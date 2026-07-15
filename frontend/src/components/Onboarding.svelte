<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type FoundGame } from '../lib/api';
  import { app, refreshGames } from '../lib/stores.svelte';

  let {
    trusted = false,
    onclose,
  }: { trusted?: boolean; onclose: () => void } = $props();

  // 0 welcome · 1 certificate · 2 first game · 3 ready
  let step = $state(0);

  // The prop is only a hint (App may still be fetching when we mount) —
  // ask the server ourselves so step 1 never wrongly shows "installed".
  let tlsTrusted = $state(trusted);
  let tlsBusy = $state(false);
  let tlsError = $state<string | null>(null);

  onMount(() => {
    api.tlsStatus().then((s) => (tlsTrusted = s.trusted)).catch(() => {});
  });

  let publishDir = $state('');
  let gameBusy = $state(false);
  let gameError = $state<string | null>(null);
  let candidates = $state<FoundGame[]>([]);
  let added = $state<string[]>([]);

  const derivedName = $derived.by(() => {
    const m = publishDir.match(/\/math\/games\/([^/]+)\/library\/publish_files\/?$/);
    return m?.[1] ?? '';
  });

  function finish(openPlay: boolean) {
    localStorage.setItem('slotlab.onboarded', '1');
    if (openPlay) app.tab = 'play';
    onclose();
  }

  async function installCa() {
    tlsBusy = true;
    tlsError = null;
    try {
      const r = await api.tlsInstall();
      tlsTrusted = r.trusted;
      if (!r.trusted) tlsError = 'The certificate was not marked as trusted. Try again.';
    } catch (e) {
      tlsError = e instanceof Error ? e.message : String(e);
    } finally {
      tlsBusy = false;
    }
  }

  /** Candidates not registered yet (by name or path). */
  function freshOf(games: FoundGame[]): FoundGame[] {
    return games.filter(
      (g) => !app.games.some((x) => x.name === g.name || x.publishDir === g.path),
    );
  }

  /** Single fresh game → add it right away; several → offer the list. */
  async function handleFound(games: FoundGame[]) {
    const fresh = freshOf(games);
    if (fresh.length === 0) {
      gameError =
        games.length === 1
          ? 'This game is already added.'
          : `All ${games.length} games in this folder are already added.`;
      return;
    }
    if (fresh.length === 1) {
      await addOne(fresh[0].name, fresh[0].path);
    } else {
      candidates = fresh;
    }
  }

  async function browse() {
    gameError = null;
    candidates = [];
    try {
      const r = await api.pickFolder();
      if (r.cancelled) return;
      if (r.error) {
        gameError = r.error;
        return;
      }
      const games = r.games ?? [];
      if (games.length === 0) {
        publishDir = r.picked ?? '';
        gameError =
          'No publish_files found in this folder. Pick a game folder (or any parent folder — SlotLab searches inside).';
      } else {
        await handleFound(games);
      }
    } catch (e) {
      gameError = e instanceof Error ? e.message : String(e);
    }
  }

  async function addAllCandidates() {
    gameBusy = true;
    gameError = null;
    for (const c of [...candidates]) {
      try {
        await api.registerGame(c.name, c.path, '');
        added = [...added, c.name];
      } catch {
        /* keep going */
      }
    }
    await refreshGames();
    if (added.length > 0) app.selectedGame = added[0];
    candidates = [];
    gameBusy = false;
  }

  async function addOne(name: string, path: string) {
    gameBusy = true;
    gameError = null;
    try {
      await api.registerGame(name, path, '');
      await refreshGames();
      app.selectedGame = name;
      added = [...added, name];
      candidates = candidates.filter((c) => c.name !== name);
      publishDir = '';
    } catch (e) {
      gameError = e instanceof Error ? e.message : String(e);
    } finally {
      gameBusy = false;
    }
  }

  async function addTyped() {
    if (derivedName) {
      await addOne(derivedName, publishDir.trim());
      return;
    }
    // Hand-typed path that isn't a publish_files itself → resolve it.
    gameBusy = true;
    gameError = null;
    candidates = [];
    try {
      const r = await api.resolvePath(publishDir.trim());
      if (r.games.length === 0) {
        gameError =
          'No publish_files found at this path. Point to a game folder (or any parent folder).';
      } else {
        gameBusy = false;
        await handleFound(r.games);
      }
    } catch (e) {
      gameError = e instanceof Error ? e.message : String(e);
    } finally {
      gameBusy = false;
    }
  }

  const steps = ['Welcome', 'Certificate', 'First game', 'Ready'];
</script>

<div class="veil" role="dialog" aria-modal="true" aria-label="SlotLab setup">
  <div class="card">
    <header>
      <div class="dots" aria-hidden="true">
        {#each steps as label, i (label)}
          <span class="dot" class:on={i === step} class:done={i < step}></span>
        {/each}
      </div>
      <button class="ghost skip" onclick={() => finish(false)}>
        Skip setup
      </button>
    </header>

    {#if step === 0}
      <div class="body center">
        <div class="chip">SL</div>
        <h1>Welcome to SlotLab</h1>
        <p class="lead">
          Your dev &amp; QA bench for Stake Engine slots — play any game against
          a local RGS, audit its math, and share a playable build in one click.
        </p>
        <ul class="features">
          <li>
            <span class="glyph">▶</span>
            <div>
              <strong>Play</strong>
              <span>Run games locally: viewports, forced outcomes, live spin feed, replays.</span>
            </div>
          </li>
          <li>
            <span class="glyph">∑</span>
            <div>
              <strong>Analyze</strong>
              <span>RTP, volatility, distribution and 21 compliance checks per mode.</span>
            </div>
          </li>
          <li>
            <span class="glyph">⇗</span>
            <div>
              <strong>Share</strong>
              <span>A public play link for anyone — nothing to install on their side.</span>
            </div>
          </li>
        </ul>
      </div>
      <footer>
        <span></span>
        <button class="primary big" onclick={() => (step = 1)}>Get started</button>
      </footer>
    {:else if step === 1}
      <div class="body">
        <p class="overline">Step 1 · Local HTTPS certificate</p>
        <h2>Let your browser trust the local casino server</h2>
        <p>
          SlotLab emulates Stake's game server (the RGS) on your machine.
          Games only talk to it over <code>https</code>, so SlotLab needs its
          local certificate — <em>SlotLab Local CA</em> — added to your macOS
          keychain.
        </p>
        <ul class="reassure">
          <li>✓ macOS will ask for <strong>your session password</strong> — that's the standard system dialog, nothing else is changed.</li>
          <li>✓ The certificate only vouches for <code>localhost</code>, is stored in <strong>your</strong> login keychain, and never leaves this Mac.</li>
          <li>✓ You can remove it anytime in Keychain Access (search “SlotLab”).</li>
        </ul>
        {#if tlsTrusted}
          <p class="status ok">✓ Certificate installed — you're good.</p>
        {:else}
          <button class="primary big" onclick={installCa} disabled={tlsBusy}>
            {tlsBusy ? 'Waiting for macOS…' : 'Install the certificate'}
          </button>
          {#if tlsError}
            <p class="status err">{tlsError}</p>
          {/if}
        {/if}
      </div>
      <footer>
        <button class="ghost" onclick={() => (step = 0)}>Back</button>
        {#if tlsTrusted}
          <button class="primary big" onclick={() => (step = 2)}>Continue</button>
        {:else}
          <button class="ghost" onclick={() => (step = 2)}>Do this later</button>
        {/if}
      </footer>
    {:else if step === 2}
      <div class="body">
        <p class="overline">Step 2 · Add your first game</p>
        <h2>Point SlotLab at a game's math</h2>
        <p>
          Pick a game folder, a repo, or any parent folder — SlotLab digs out
          every <code>publish_files</code> inside, derives the names and
          imports the books on its own. Several games found = add them all in
          one click.
        </p>
        <div class="add-row">
          <button onclick={browse} disabled={gameBusy}>📁 Browse…</button>
          <input
            bind:value={publishDir}
            placeholder="…or paste a path"
            onkeydown={(e) => e.key === 'Enter' && publishDir.trim() && addTyped()}
          />
          <button
            class="primary"
            onclick={addTyped}
            disabled={gameBusy || !publishDir.trim()}
          >
            {gameBusy ? 'Adding…' : derivedName ? `Add ${derivedName}` : 'Add'}
          </button>
        </div>
        {#if candidates.length > 0}
          <p class="dim found">
            {candidates.length} games found:
            <button class="primary" onclick={addAllCandidates} disabled={gameBusy}>
              {gameBusy ? 'Adding…' : `Add all ${candidates.length}`}
            </button>
            <span class="dim">or pick them one by one —</span>
          </p>
          <div class="chips">
            {#each candidates as c (c.path)}
              <button
                class="chip-btn"
                title={c.path}
                onclick={() => addOne(c.name, c.path)}
                disabled={gameBusy}
              >
                + {c.name}
              </button>
            {/each}
          </div>
        {/if}
        {#if added.length > 0}
          <p class="status ok">
            ✓ Added: {added.join(', ')} — books are importing in the background.
          </p>
        {/if}
        {#if gameError}
          <p class="status err">{gameError}</p>
        {/if}
      </div>
      <footer>
        <button class="ghost" onclick={() => (step = 1)}>Back</button>
        {#if added.length > 0}
          <button class="primary big" onclick={() => (step = 3)}>Continue</button>
        {:else}
          <button class="ghost" onclick={() => (step = 3)}>Do this later</button>
        {/if}
      </footer>
    {:else}
      <div class="body center">
        <div class="chip ok-chip">✓</div>
        <h1>You're all set</h1>
        <p class="lead">
          {#if added.length > 0}
            <strong>{added.join(', ')}</strong> is importing its books in the
            background (a few minutes for a big game, one time only). The
            <strong>Games</strong> tab shows the progress; everything else is
            ready as soon as it turns green.
          {:else}
            Add a game anytime from the <strong>Games</strong> tab — everything
            else lights up from there.
          {/if}
        </p>
        <p class="dim">
          Tip: reopen this guide anytime via the “?” button, bottom-left.
        </p>
      </div>
      <footer>
        <button class="ghost" onclick={() => (step = 2)}>Back</button>
        <button class="primary big" onclick={() => finish(true)}>Open SlotLab</button>
      </footer>
    {/if}
  </div>
</div>

<style>
  .veil {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    place-items: center;
    padding: 24px;
    background:
      radial-gradient(1200px 500px at 75% -10%, oklch(0.2 0.02 165 / 0.85), transparent 70%),
      var(--bg);
  }

  .card {
    width: min(680px, 100%);
    min-height: 540px;
    display: flex;
    flex-direction: column;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    box-shadow: 0 30px 80px oklch(0 0 0 / 0.45);
    padding: 20px 34px 26px;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 18px;
  }

  .dots {
    display: flex;
    gap: 7px;
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--border-strong);
    transition: background 0.2s var(--ease-out), transform 0.2s var(--ease-out);
  }

  .dot.on {
    background: var(--gold);
    transform: scale(1.25);
  }

  .dot.done {
    background: oklch(0.82 0.13 88 / 0.45);
  }

  .skip {
    font-size: 12px;
    color: var(--text-faint);
  }

  .body {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }

  .body.center {
    align-items: center;
    text-align: center;
  }

  .chip {
    display: grid;
    place-items: center;
    width: 58px;
    height: 58px;
    border-radius: 50%;
    background: var(--gold);
    color: var(--gold-ink);
    font-size: 21px;
    font-weight: 800;
    margin-bottom: 18px;
    box-shadow:
      0 0 0 5px var(--bg-panel),
      0 0 0 6.5px oklch(0.82 0.13 88 / 0.4);
  }

  .ok-chip {
    background: var(--pass);
    color: oklch(0.2 0.05 155);
    box-shadow:
      0 0 0 5px var(--bg-panel),
      0 0 0 6.5px oklch(0.78 0.16 155 / 0.4);
  }

  h1 {
    font-size: 24px;
    margin-bottom: 8px;
  }

  h2 {
    font-size: 19px;
    margin-bottom: 10px;
  }

  .lead {
    font-size: 14px;
    color: var(--text-dim);
    max-width: 46ch;
    margin: 0 0 22px;
  }

  .features {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
    text-align: left;
  }

  .features li {
    display: flex;
    gap: 14px;
    align-items: baseline;
  }

  .features .glyph {
    color: var(--gold);
    width: 18px;
    text-align: center;
    flex-shrink: 0;
  }

  .features strong {
    display: block;
    font-weight: 660;
  }

  .features span:not(.glyph) {
    color: var(--text-dim);
    font-size: 12.5px;
  }

  .reassure {
    list-style: none;
    padding: 14px 16px;
    margin: 14px 0 20px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--r-s);
    font-size: 12.5px;
    color: var(--text-dim);
  }

  .add-row {
    display: flex;
    gap: 10px;
    margin-top: 14px;
  }

  .add-row input {
    flex: 1;
    font-family: var(--font-mono);
    font-size: 12px;
  }

  .found {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 14px 0 8px;
    font-size: 12.5px;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip-btn {
    font-family: var(--font-mono);
    font-size: 12px;
  }

  .status {
    margin: 14px 0 0;
    font-size: 13px;
  }

  .status.ok {
    color: var(--pass);
  }

  .status.err {
    color: var(--fail);
  }

  .primary.big {
    padding: 9px 22px;
    font-size: 13.5px;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 22px;
  }

  .dim {
    color: var(--text-faint);
  }
</style>
