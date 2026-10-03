<script lang="ts">
  import {
    api,
    fmtInt,
    fmtOneIn,
    fmtPct,
    fmtX,
    playUrlFor,
    type HistoryReport,
  } from '../lib/api';
  import { GameAdder } from '../lib/addGame.svelte';
  import { app, refreshGames, run } from '../lib/stores.svelte';
  import ConfirmButton from '../components/ConfirmButton.svelte';
  import Icon from '../components/Icon.svelte';

  // What the last `run.py prod` changed. Every panel of the app shows the
  // CURRENT numbers; this is the only one that shows a move.
  let history = $state<HistoryReport | null>(null);

  $effect(() => {
    const name = app.selectedGame;
    if (!name) {
      history = null;
      return;
    }
    api.history(name).then((h) => (history = h)).catch(() => (history = null));
  });

  /**
   * Rates keep the app-wide 2 decimals — unless both ends round to the same
   * text, where a diff that reads "96.50 % → 96.50 %" would hide the move.
   */
  function fmtValue(field: string, v: number, other: number): string {
    if (!Number.isFinite(v)) return 'never';
    if (field === 'rtp' || field === 'hitRate') {
      return fmtPct(v) === fmtPct(other) ? fmtPct(v, 4) : fmtPct(v);
    }
    if (field === 'maxPayout') return fmtX(v, 0);
    if (field.endsWith('OneIn')) return fmtOneIn(v);
    if (field === 'outcomes') return fmtInt(v);
    return v.toFixed(3);
  }

  function when(ms: number): string {
    return new Date(ms).toLocaleString();
  }

  const adder = new GameAdder({ toast: true });

  /** Games being reloaded (their button shows it). */
  let reloading = $state<Record<string, boolean>>({});

  async function remove(gameName: string) {
    await run(
      async () => {
        await api.removeGame(gameName);
        await refreshGames();
      },
      { ok: `${gameName} removed from SlotLab`, error: `Could not remove ${gameName}` },
    );
  }

  async function reload(gameName: string) {
    reloading[gameName] = true;
    await run(
      async () => {
        await api.reloadGame(gameName);
        await refreshGames();
      },
      { ok: `${gameName} reloaded`, error: `Could not reload ${gameName}` },
    );
    reloading[gameName] = false;
  }

  const fmtCost = (c: number) => fmtX(c, Number.isInteger(c) ? 0 : 2);
</script>

<section>
  <p class="overline">Add a game</p>
  <form
    class="add-row"
    onsubmit={(e) => {
      e.preventDefault();
      void adder.addTyped();
    }}
  >
    <button type="button" onclick={adder.browse} disabled={adder.busy}>
      <Icon name="folder" size={14} /> Browse…
    </button>
    <input
      bind:value={adder.path}
      aria-label="Game folder path"
      placeholder="…or paste a path: a game folder or its publish_files"
    />
    <button type="submit" class="primary" disabled={adder.busy || !adder.path.trim()}>
      {adder.busy ? 'Adding…' : adder.derivedName ? `Add ${adder.derivedName}` : 'Add'}
    </button>
  </form>
  {#if adder.info}
    <p class="form-note" role="status">{adder.info}</p>
  {/if}
  {#if adder.candidates.length > 0}
    <div class="candidates">
      <p class="dim">
        {adder.candidates.length} games found in this folder:
        <button class="primary" onclick={adder.addAll} disabled={adder.busy}>
          {adder.busy ? 'Adding…' : `Add all ${adder.candidates.length}`}
        </button>
        <span class="faint">or pick them one by one</span>
      </p>
      <div class="cands">
        {#each adder.candidates as c (c.path)}
          <button
            class="cand"
            title={c.path}
            disabled={adder.busy}
            onclick={() => adder.addOne(c.name, c.path)}
          >
            + {c.name}
          </button>
        {/each}
      </div>
    </div>
  {/if}
  {#if adder.error}
    <p class="field-error" role="alert">{adder.error}</p>
  {/if}
</section>

{#if history && history.diff.length > 0}
  <section>
    <p class="overline">
      What changed in the last build
      <span class="faint">
        {history.snapshots.length > 1
          ? when(history.snapshots[history.snapshots.length - 2].takenAt) +
            ' → ' +
            when(history.snapshots[history.snapshots.length - 1].takenAt)
          : ''}
      </span>
    </p>
    <table>
      <thead>
        <tr>
          <th>Mode</th><th>Metric</th>
          <th class="num">Before</th><th class="num">After</th>
        </tr>
      </thead>
      <tbody>
        {#each history.diff as d (d.mode)}
          {#if d.status !== 'changed'}
            <tr>
              <td>{d.mode}</td>
              <td colspan="3"><span class="pill {d.status === 'added' ? 'green' : 'red'}">{d.status}</span></td>
            </tr>
          {:else}
            {#each d.changes as c (c.field)}
              <tr class:notable={c.notable}>
                <td>{c.field === d.changes[0].field ? d.mode : ''}</td>
                <td>{c.label}{#if c.notable}<span class="sr-only"> (moved)</span>{/if}</td>
                <td class="num faint">{fmtValue(c.field, c.before, c.after)}</td>
                <td class="num">{fmtValue(c.field, c.after, c.before)}</td>
              </tr>
            {/each}
          {/if}
        {/each}
      </tbody>
    </table>
    <p class="note faint">
      Recorded whenever a game is loaded with files it has not seen before, so a
      regression that moves no other indicator still leaves a trace. Highlighted
      rows moved by more than rounding.
    </p>
  </section>
{/if}

<section>
  <p class="overline">Games ({app.games.length})</p>
  {#each app.games as game (game.name)}
    <article class="game" class:selected={app.selectedGame === game.name}>
      <header>
        <button
          class="name"
          aria-pressed={app.selectedGame === game.name}
          title="Make it the current game"
          onclick={() => (app.selectedGame = game.name)}
        >
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
          {#if game.imported}
            <a
              class="served"
              href={playUrlFor(game.name, game.servedUrl)}
              target="_blank"
              rel="noreferrer"
            >
              Open game<span class="sr-only"> (new tab)</span>
              <Icon name="link" size={11} />
            </a>
          {:else}
            <span class="faint small">Open game — available once the books are imported</span>
          {/if}
        {:else if game.loaded}
          <span class="faint small">no frontend build detected</span>
        {/if}
        <span class="spacer"></span>
        <button
          class="ghost"
          onclick={() => reload(game.name)}
          disabled={reloading[game.name]}
          aria-label="Reload {game.name}"
        >
          {reloading[game.name] ? 'Reloading…' : 'Reload'}
        </button>
        <ConfirmButton
          onconfirm={() => remove(game.name)}
          confirmLabel="Remove from SlotLab?"
          title="Math files are left untouched"
          ariaLabel="Remove {game.name}"
        >
          Remove
        </ConfirmButton>
      </header>
      <p class="faint path" title={game.publishDir}>{game.publishDir}</p>
      {#if game.error}
        <p class="field-error">{game.error}</p>
      {/if}
      {#if game.modes.length > 0}
        <table class="fixed modes">
          <colgroup>
            <col />
            <col class="c-cost" />
            <col class="c-rate" />
            <col class="c-rate" />
            <col class="c-max" />
            <col class="c-books" />
          </colgroup>
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
                <td
                  class="mode"
                  title={m.aliases.length > 0 ? `${m.name} (alias: ${m.aliases.join(', ')})` : m.name}
                >
                  {m.name}
                  {#if m.aliases.length > 0}
                    <span class="faint">(alias: {m.aliases.join(', ')})</span>
                  {/if}
                </td>
                <td class="num">{fmtCost(m.cost)}</td>
                <td class="num">{fmtPct(m.rtp)}</td>
                <td class="num">{fmtPct(m.hitRate)}</td>
                <td class="num">{fmtX(m.maxPayout, 0)}</td>
                <td class="num">{fmtInt(m.outcomeCount)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </article>
  {/each}
  {#if app.games.length === 0}
    <p class="dim">
      Browse to a game folder above (or paste its path) — SlotLab finds its
      <code>publish_files</code>, derives the name, imports the books and
      detects the frontend build on its own.
    </p>
  {/if}
</section>

<style>
  tr.notable td {
    color: var(--warn);
  }

  section {
    margin-bottom: 28px;
  }

  .add-row {
    display: flex;
    gap: var(--sp-2);
  }

  .add-row button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .add-row input {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-sm);
  }

  .form-note {
    color: var(--text-dim);
    margin: var(--sp-2) 0 0;
    font-size: var(--fs-sm);
  }

  .candidates {
    margin-top: var(--sp-3);
  }

  .candidates p {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--sp-2);
    margin: 0 0 var(--sp-2);
    font-size: var(--fs-sm);
  }

  .cands {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .cand {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
  }

  .note {
    font-size: var(--fs-xs);
    margin: var(--sp-2) 0 0;
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
    flex-wrap: wrap;
    gap: 6px var(--sp-3);
  }

  /* The game name IS the selector: styled as the title, not as a button. */
  .game .name {
    font-size: var(--fs-md);
    font-weight: 680;
    color: var(--text);
    background: transparent;
    border-color: transparent;
    padding: 2px 6px 2px 0;
  }

  .game .name:hover:not(:disabled) {
    background: transparent;
    border-color: transparent;
    text-decoration: underline;
    text-underline-offset: 3px;
  }

  .game.selected .name {
    color: var(--gold);
  }

  .served {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    font-size: var(--fs-xs);
    color: var(--info);
    text-decoration: none;
  }

  .served:hover {
    text-decoration: underline;
  }

  .small {
    font-size: var(--fs-xs);
  }

  .spacer {
    flex: 1;
  }

  .path {
    font-family: var(--font-mono);
    font-size: var(--fs-2xs);
    margin: 2px 0 var(--sp-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  table {
    max-width: 860px;
  }

  /* Same columns for every game, so the numbers line up down the page. */
  .modes .c-cost {
    width: 80px;
  }

  .modes .c-rate {
    width: 104px;
  }

  .modes .c-max {
    width: 112px;
  }

  .modes .c-books {
    width: 112px;
  }

  .modes .mode {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
