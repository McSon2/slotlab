<script lang="ts">
  import { bookmarksApi, fmtOneIn, fmtX } from '../lib/api';
  import { errorText, notify, run } from '../lib/stores.svelte';
  import Icon from './Icon.svelte';

  interface SearchModeMeta {
    mode: string;
    imported: boolean;
    criteria?: string[];
    eventTypes?: string[];
  }

  interface SearchHit {
    mode: string;
    simId: number;
    payoutX100: number;
    criteria: string | null;
    /** Événements `freeSpinTrigger` du book. */
    fsTriggers: number;
    /** Retriggers dans le bonus : annonces d'un total de free spins plus haut. */
    fsRetriggers: number;
    oneIn: number | null;
  }

  let {
    game,
    onPlay,
  }: {
    game: string;
    onPlay: (mode: string, simId: number) => void;
  } = $props();

  let meta = $state<SearchModeMeta[]>([]);
  let metaError = $state<string | null>(null);
  let mode = $state('');
  let payoutMin = $state('');
  let payoutMax = $state('');
  let criteria = $state('');
  let eventType = $state('');
  let bonus = $state<'any' | 'without' | 'with'>('any');
  let sort = $state<'payoutDesc' | 'payoutAsc' | 'id'>('payoutDesc');
  let minRetriggers = $state('');
  let hits = $state<SearchHit[]>([]);
  let total = $state(0);
  let offset = $state(0);
  let busy = $state(false);
  /** Modes the last search skipped (books not imported yet). */
  let notImported = $state<string[]>([]);
  /** Row whose bookmark request is in flight. */
  let bookmarking = $state<string | null>(null);
  const limit = 50;

  $effect(() => {
    if (game) void loadMeta(game);
  });

  async function loadMeta(gameName: string) {
    hits = [];
    total = 0;
    notImported = [];
    try {
      const res = await fetch(`/api/ui/games/${encodeURIComponent(gameName)}/search-meta`);
      if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
      const r = await res.json();
      if (gameName !== game) return; // game changed meanwhile
      meta = r.modes ?? [];
      metaError = null;
    } catch (e) {
      meta = [];
      metaError = errorText(e);
    }
  }

  const currentMeta = $derived(meta.find((m) => m.mode === mode));
  const allCriteria = $derived([
    ...new Set(meta.flatMap((m) => m.criteria ?? [])),
  ]);
  const allEventTypes = $derived(
    mode
      ? (currentMeta?.eventTypes ?? [])
      : [...new Set(meta.flatMap((m) => m.eventTypes ?? []))],
  );

  async function search(newOffset = 0) {
    busy = true;
    try {
      const res = await fetch('/api/ui/search', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          game,
          mode: mode || undefined,
          payoutMin: payoutMin ? Number(payoutMin) : undefined,
          payoutMax: payoutMax ? Number(payoutMax) : undefined,
          criteria: criteria || undefined,
          eventTypes: eventType ? [eventType] : [],
          bonus,
          sort,
          // Retriggers DANS le bonus (`fsRetriggers`), pas le nombre de
          // `freeSpinTrigger` : celui-ci vaut 1 pour tout book de bonus de
          // BLOOD WATER, quels que soient ses retriggers.
          minRetriggers: minRetriggers ? Number(minRetriggers) : undefined,
          offset: newOffset,
          limit,
        }),
      });
      const r = await res.json().catch(() => ({}));
      if (r.error) throw new Error(r.error);
      if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
      hits = r.hits ?? [];
      total = r.total ?? 0;
      offset = newOffset;
      notImported = r.notImported ?? [];
      if (hits.length === 0) notify('No book matches these filters', 'info');
    } catch (e) {
      notify(`Search failed: ${errorText(e)}`, 'error');
    } finally {
      busy = false;
    }
  }

  async function bookmark(hit: SearchHit) {
    bookmarking = hit.mode + hit.simId;
    const ok = await run(
      () =>
        bookmarksApi.add({
          game,
          mode: hit.mode,
          simId: hit.simId,
          payoutX100: hit.payoutX100,
          description: 'from Search',
        }),
      { ok: `Bookmarked ${hit.mode} #${hit.simId}`, error: 'Could not add the bookmark' },
    );
    bookmarking = null;
    if (ok !== undefined) {
      window.dispatchEvent(new CustomEvent('slotlab:bookmarks-changed', { detail: { game } }));
    }
  }
</script>

<details class="panel search-books">
  <summary>
    <Icon name="scan" size={13} />
    Search books — find an event by win, criteria or bonus, then force it
  </summary>

  {#if metaError}
    <p class="field-error" role="alert">Could not load the search filters: {metaError}</p>
  {/if}

  <form
    class="controls"
    aria-label="Search books"
    onsubmit={(e) => {
      e.preventDefault();
      void search(0);
    }}
  >
    <label>
      <span>Mode</span>
      <select bind:value={mode}>
        <option value="">all</option>
        {#each meta as m (m.mode)}<option value={m.mode}>{m.mode}</option>{/each}
      </select>
    </label>
    <label>
      <span>Min win (x)</span>
      <input style="width: 80px" bind:value={payoutMin} placeholder="e.g. 100" inputmode="decimal" />
    </label>
    <label>
      <span>Max win (x)</span>
      <input style="width: 80px" bind:value={payoutMax} placeholder="e.g. 5000" inputmode="decimal" />
    </label>
    <label>
      <span>Criteria</span>
      <select bind:value={criteria}>
        <option value="">all</option>
        {#each allCriteria as c (c)}<option>{c}</option>{/each}
      </select>
    </label>
    <label>
      <span>Bonus</span>
      <select bind:value={bonus}>
        <option value="any">any</option>
        <option value="without">without bonus</option>
        <option value="with">with bonus</option>
      </select>
    </label>
    <label>
      <span>Sort</span>
      <select bind:value={sort}>
        <option value="payoutDesc">win ↓</option>
        <option value="payoutAsc">win ↑</option>
        <option value="id">book id</option>
      </select>
    </label>
    <label>
      <span>Event present</span>
      <select bind:value={eventType}>
        <option value="">—</option>
        {#each allEventTypes as t (t)}<option>{t}</option>{/each}
      </select>
    </label>
    <label
      title="Re-triggers inside the bonus: every announcement of a free-spin total above all the previous ones (freeSpinRetrigger, updateFreeSpin, sharkFeed.hunts…)"
    >
      <span>Retriggers ≥</span>
      <input style="width: 64px" bind:value={minRetriggers} placeholder="e.g. 2" inputmode="numeric" />
    </label>
    <button type="submit" class="primary" disabled={busy}>
      {busy ? 'Searching…' : 'Search'}
    </button>
  </form>

  {#if notImported.length > 0}
    <p class="note">Modes not imported yet (ignored): {notImported.join(', ')}</p>
  {/if}

  {#if hits.length > 0}
    <div class="results-head">
      <h3>{total.toLocaleString('en-US')} results</h3>
      <div class="pager">
        <button
          disabled={busy || offset === 0}
          aria-label="Previous page"
          title="Previous page"
          onclick={() => search(Math.max(0, offset - limit))}
        ><Icon name="chevron-left" size={14} /></button>
        <span class="dim num">{offset + 1}–{Math.min(offset + limit, total)}</span>
        <button
          disabled={busy || offset + limit >= total}
          aria-label="Next page"
          title="Next page"
          onclick={() => search(offset + limit)}
        ><Icon name="chevron-right" size={14} /></button>
      </div>
    </div>
    <table>
      <thead>
        <tr>
          <th>Mode</th>
          <th class="num">Event</th>
          <th class="num">Win</th>
          <th class="num">Frequency</th>
          <th>Criteria</th>
          <th class="num">FS triggers</th>
          <th class="num">Retriggers</th>
          <th><span class="sr-only">Actions</span></th>
        </tr>
      </thead>
      <tbody>
        {#each hits as h (h.mode + h.simId)}
          <tr>
            <td>{h.mode}</td>
            <td class="num">{h.simId}</td>
            <td class="num">{fmtX(h.payoutX100 / 100)}</td>
            <td class="num">{h.oneIn ? fmtOneIn(h.oneIn) : '—'}</td>
            <td class="dim">{h.criteria ?? '—'}</td>
            <td class="num">{h.fsTriggers || '—'}</td>
            <!-- 0 sur un book de bonus (pas de retrigger), « — » sans bonus. -->
            <td class="num">{h.fsTriggers || h.fsRetriggers ? h.fsRetriggers : '—'}</td>
            <td class="actions">
              <button
                title="Force on next spin"
                aria-label="Force {h.mode} #{h.simId} on the next spin"
                onclick={() => onPlay(h.mode, h.simId)}
              ><Icon name="play" size={10} /> Play</button>
              <button
                title="Bookmark this event"
                aria-label="Bookmark {h.mode} #{h.simId}"
                disabled={bookmarking === h.mode + h.simId}
                onclick={() => bookmark(h)}
              ><Icon name="star" size={13} /></button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</details>

<style>
  .search-books {
    margin-bottom: var(--sp-4);
  }

  .search-books summary {
    font-size: var(--fs-sm);
    color: var(--text-dim);
    cursor: pointer;
    user-select: none;
  }

  .search-books summary:hover {
    color: var(--text);
  }

  .search-books[open] summary {
    margin-bottom: var(--sp-3);
  }

  .controls {
    --ctrl-h: 32px;
    display: flex;
    flex-wrap: wrap;
    gap: 10px 14px;
    align-items: flex-end;
    margin: 0 0 var(--sp-3);
  }

  /* Same alignment system as the Play strip: one height, bottoms flush. */
  .controls :is(select, input, button) {
    height: var(--ctrl-h);
    box-sizing: border-box;
  }

  .controls label {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .controls label > span {
    font-size: var(--fs-2xs);
    line-height: 1;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-faint);
    white-space: nowrap;
  }

  .note {
    color: var(--text-faint);
    font-size: var(--fs-xs);
    margin: 0 0 var(--sp-2);
  }

  .results-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    margin-bottom: var(--sp-2);
  }

  .pager {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }

  .actions {
    display: flex;
    gap: 6px;
  }

  .actions button {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 8px;
    min-height: 24px;
    font-size: var(--fs-sm);
  }
</style>
