<script lang="ts">
  import { api, fmtPct, fmtX, playUrlFor, type FoundGame } from '../lib/api';
  import { app, refreshGames } from '../lib/stores.svelte';

  let publishDir = $state('');
  let busy = $state(false);
  let formError = $state<string | null>(null);
  /** Multiple games found (repo root) → the user picks. */
  let candidates = $state<FoundGame[]>([]);

  /** Derives the game_id from `<repo>/math/games/<id>/library/publish_files`. */
  const derivedName = $derived.by(() => {
    const m = publishDir.match(/\/math\/games\/([^/]+)\/library\/publish_files\/?$/);
    return m?.[1] ?? '';
  });

  async function browse() {
    formError = null;
    candidates = [];
    try {
      const r = await api.pickFolder();
      if (r.cancelled) return;
      if (r.error) {
        formError = r.error;
        return;
      }
      const games = r.games ?? [];
      if (games.length === 0) {
        publishDir = r.picked ?? '';
        formError =
          'No publish_files found in this folder. Pick a game folder or its publish_files.';
      } else if (games.length === 1) {
        publishDir = games[0].path;
      } else {
        candidates = games;
      }
    } catch (e) {
      formError = e instanceof Error ? e.message : String(e);
    }
  }

  async function addOne(name: string, path: string) {
    busy = true;
    formError = null;
    try {
      await api.registerGame(name, path, '');
      await refreshGames();
      app.selectedGame = name;
      return true;
    } catch (e) {
      formError = e instanceof Error ? e.message : String(e);
      return false;
    } finally {
      busy = false;
    }
  }

  async function addAllCandidates() {
    busy = true;
    for (const g of candidates) {
      try {
        await api.registerGame(g.name, g.path, '');
      } catch {
        /* ignore, keep going */
      }
    }
    await refreshGames();
    if (candidates[0]) app.selectedGame = candidates[0].name;
    candidates = [];
    busy = false;
  }

  async function register() {
    // Hand-typed path that doesn't match → try to resolve it.
    if (!derivedName) {
      const r = await api.resolvePath(publishDir.trim());
      if (r.games.length === 1) {
        publishDir = r.games[0].path;
      } else if (r.games.length > 1) {
        candidates = r.games;
        return;
      } else {
        formError =
          'No publish_files found. Use "Browse" or provide a …/library/publish_files path.';
        return;
      }
    }
    if (await addOne(derivedName, publishDir.trim())) {
      publishDir = '';
    }
  }

  async function remove(gameName: string) {
    if (!confirm(`Remove ${gameName} from SlotLab? (math files are left untouched)`)) return;
    await api.removeGame(gameName);
    await refreshGames();
  }

  async function reload(gameName: string) {
    busy = true;
    try {
      await api.reloadGame(gameName);
      await refreshGames();
    } catch (e) {
      formError = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

<section>
  <p class="overline">Add a game</p>
  <div class="add-row">
    <button onclick={browse} disabled={busy}>📁 Browse…</button>
    <input
      bind:value={publishDir}
      placeholder="…or paste a path: game folder, or its publish_files"
      onkeydown={(e) => e.key === 'Enter' && register()}
    />
    <button class="primary" onclick={register} disabled={busy || !publishDir.trim()}>
      {busy ? 'Loading…' : derivedName ? `Add ${derivedName}` : 'Add'}
    </button>
  </div>
  {#if candidates.length > 0}
    <div class="candidates">
      <p class="dim">
        {candidates.length} games found in this folder:
        <button class="ghost" onclick={addAllCandidates} disabled={busy}>
          add all
        </button>
      </p>
      <div class="chips">
        {#each candidates as c (c.path)}
          <button class="chip-btn" onclick={() => addOne(c.name, c.path).then((ok) => ok && (candidates = candidates.filter((x) => x.path !== c.path)))}>
            + {c.name}
          </button>
        {/each}
      </div>
    </div>
  {/if}
  {#if formError}
    <p class="form-error">{formError}</p>
  {/if}
</section>

<section>
  <p class="overline">Games ({app.games.length})</p>
  {#each app.games as game (game.name)}
    <article class="game" class:selected={app.selectedGame === game.name}>
      <header>
        <button class="ghost name" onclick={() => (app.selectedGame = game.name)}>
          {game.name}
        </button>
        {#if !game.loaded}
          <span class="pill red">error</span>
        {:else if !game.imported}
          <span class="pill yellow">importing…</span>
        {:else}
          <span class="pill green">ready</span>
        {/if}
        {#if game.servedUrl}
          <a
            class="served"
            href={game.imported ? playUrlFor(game.name, game.servedUrl) : undefined}
            class:disabled={!game.imported}
            target="_blank"
            rel="noreferrer"
            title={game.imported
              ? 'Open the playable game in a tab'
              : 'Importing books…'}
          >
            open game ⇗
          </a>
        {:else if game.loaded}
          <span class="faint">no frontend build detected</span>
        {/if}
        <span class="spacer"></span>
        <button class="ghost" onclick={() => reload(game.name)}>Reload</button>
        <button class="ghost danger" onclick={() => remove(game.name)}>Remove</button>
      </header>
      <p class="faint path">{game.publishDir}</p>
      {#if game.error}
        <p class="form-error">{game.error}</p>
      {/if}
      {#if game.modes.length > 0}
        <table>
          <thead>
            <tr>
              <th>Mode</th>
              <th class="num">Cost</th>
              <th class="num">RTP</th>
              <th class="num">Hit rate</th>
              <th class="num">Max win</th>
              <th class="num">Outcomes</th>
            </tr>
          </thead>
          <tbody>
            {#each game.modes as m (m.name)}
              <tr>
                <td>
                  {m.name}
                  {#if m.aliases.length > 0}
                    <span class="faint">(alias: {m.aliases.join(', ')})</span>
                  {/if}
                </td>
                <td class="num">{m.cost}x</td>
                <td class="num">{fmtPct(m.rtp, 4)}</td>
                <td class="num">{fmtPct(m.hitRate)}</td>
                <td class="num">{fmtX(m.maxPayout, 0)}</td>
                <td class="num">{m.outcomeCount.toLocaleString('en-US')}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </article>
  {/each}
  {#if app.games.length === 0}
    <p class="dim">
      Paste the path of a <code>publish_files</code> folder above —
      SlotLab derives the name, imports the books and detects the frontend
      build on its own.
    </p>
  {/if}
</section>

<style>
  section {
    margin-bottom: 28px;
  }

  .add-row {
    display: flex;
    gap: 10px;
  }

  .add-row input {
    flex: 1;
    font-family: var(--font-mono);
    font-size: 12px;
  }

  .form-error {
    color: var(--fail);
    margin: 8px 0 0;
    font-size: 12.5px;
  }

  .candidates {
    margin-top: 12px;
  }

  .candidates p {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 8px;
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

  .game {
    padding: 14px 0 18px;
    border-bottom: 1px solid var(--border);
  }

  .game:last-child {
    border-bottom: none;
  }

  .game header {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .game .name {
    font-size: 15px;
    font-weight: 680;
    color: var(--text);
    padding: 2px 6px 2px 0;
  }

  .game.selected .name {
    color: var(--gold);
  }

  .served {
    font-size: 11.5px;
    color: var(--info);
    text-decoration: none;
  }

  .served:hover {
    text-decoration: underline;
  }

  .served.disabled {
    color: var(--text-faint);
    pointer-events: none;
  }

  .spacer {
    flex: 1;
  }

  .path {
    font-family: var(--font-mono);
    font-size: 11px;
    margin: 2px 0 8px;
  }

  table {
    max-width: 860px;
  }
</style>
