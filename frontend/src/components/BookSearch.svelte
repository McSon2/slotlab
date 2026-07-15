<script lang="ts">
  import { bookmarksApi, fmtOneIn, fmtX } from '../lib/api';

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
    fsTriggers: number;
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
  let mode = $state('');
  let payoutMin = $state('');
  let payoutMax = $state('');
  let criteria = $state('');
  let eventType = $state('');
  let minFs = $state('');
  let hits = $state<SearchHit[]>([]);
  let total = $state(0);
  let offset = $state(0);
  let busy = $state(false);
  let msg = $state<string | null>(null);
  const limit = 50;

  $effect(() => {
    if (game) void loadMeta(game);
  });

  async function loadMeta(gameName: string) {
    const r = await fetch(
      `/api/ui/games/${encodeURIComponent(gameName)}/search-meta`,
    ).then((r) => r.json());
    meta = r.modes ?? [];
    hits = [];
    total = 0;
    msg = null;
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

  async function run(newOffset = 0) {
    busy = true;
    msg = null;
    try {
      const r = await fetch('/api/ui/search', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          game,
          mode: mode || undefined,
          payoutMin: payoutMin ? Number(payoutMin) : undefined,
          payoutMax: payoutMax ? Number(payoutMax) : undefined,
          criteria: criteria || undefined,
          eventTypes: eventType ? [eventType] : [],
          minFsTriggers: minFs ? Number(minFs) : undefined,
          offset: newOffset,
          limit,
        }),
      }).then((r) => r.json());
      if (r.error) throw new Error(r.error);
      hits = r.hits;
      total = r.total;
      offset = newOffset;
      if (r.notImported?.length) {
        msg = `Modes not imported (ignored): ${r.notImported.join(', ')}`;
      }
    } catch (e) {
      msg = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function playBook(hit: SearchHit) {
    onPlay(hit.mode, hit.simId);
    msg = `${hit.mode} #${hit.simId} forced — next spin plays this book`;
  }

  async function bookmark(hit: SearchHit) {
    await bookmarksApi.add({
      game,
      mode: hit.mode,
      simId: hit.simId,
      payoutX100: hit.payoutX100,
      description: 'from Search',
    });
    msg = `${hit.mode} #${hit.simId} bookmarked`;
  }
</script>

<details class="panel search-books">
  <summary>⌕ Search books — find an event by win, criteria or bonus, then force it</summary>

  <div class="controls">
    <label>
      <span>Mode</span>
      <select bind:value={mode}>
        <option value="">all</option>
        {#each meta as m (m.mode)}<option value={m.mode}>{m.mode}</option>{/each}
      </select>
    </label>
    <label>
      <span>Min win (x)</span>
      <input style="width: 80px" bind:value={payoutMin} placeholder="100" />
    </label>
    <label>
      <span>Max win (x)</span>
      <input style="width: 80px" bind:value={payoutMax} placeholder="5000" />
    </label>
    <label>
      <span>Criteria</span>
      <select bind:value={criteria}>
        <option value="">all</option>
        {#each allCriteria as c (c)}<option>{c}</option>{/each}
      </select>
    </label>
    <label>
      <span>Event present</span>
      <select bind:value={eventType}>
        <option value="">—</option>
        {#each allEventTypes as t (t)}<option>{t}</option>{/each}
      </select>
    </label>
    <label>
      <span>Retriggers ≥</span>
      <input style="width: 56px" bind:value={minFs} placeholder="2" />
    </label>
    <button class="primary" onclick={() => run(0)} disabled={busy}>
      {busy ? 'Searching…' : 'Search'}
    </button>
  </div>

  {#if msg}
    <div class="toast">{msg}</div>
  {/if}

  {#if hits.length > 0}
    <div class="results-head">
      <h3>{total.toLocaleString('en-US')} results</h3>
      <div>
        <button disabled={offset === 0} onclick={() => run(Math.max(0, offset - limit))}>←</button>
        <span class="dim">{offset + 1}–{Math.min(offset + limit, total)}</span>
        <button disabled={offset + limit >= total} onclick={() => run(offset + limit)}>→</button>
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
          <th></th>
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
            <td class="actions">
              <button onclick={() => playBook(h)}>▶ Play</button>
              <button onclick={() => bookmark(h)}>★</button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</details>

<style>
  .search-books {
    margin-bottom: 16px;
  }

  .search-books summary {
    font-size: 12.5px;
    color: var(--text-dim);
    cursor: pointer;
    user-select: none;
  }

  .search-books summary:hover {
    color: var(--text);
  }

  .search-books[open] summary {
    margin-bottom: 12px;
  }

  .controls {
    --ctrl-h: 32px;
    display: flex;
    flex-wrap: wrap;
    gap: 10px 14px;
    align-items: flex-end;
    margin-bottom: 12px;
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
    font-size: 10px;
    line-height: 1;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-faint);
    white-space: nowrap;
  }

  .toast {
    color: var(--green);
    font-size: 12.5px;
    margin-bottom: 10px;
  }

  .results-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    margin-bottom: 8px;
  }

  .actions {
    display: flex;
    gap: 6px;
  }

  .actions button {
    padding: 2px 8px;
    font-size: 12px;
  }
</style>
