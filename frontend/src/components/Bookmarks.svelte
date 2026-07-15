<script lang="ts">
  import { bookmarksApi, fmtX, type Bookmark } from '../lib/api';

  interface Props {
    game: string;
    frontUrl: string;
    onPlay: (mode: string, simId: number) => void;
  }

  const { game, frontUrl, onPlay }: Props = $props();

  let bookmarks = $state<Bookmark[]>([]);

  export async function refresh() {
    bookmarks = await bookmarksApi.list(game);
  }

  $effect(() => {
    if (game) void refresh();
  });

  async function remove(id: number) {
    await bookmarksApi.remove(id);
    await refresh();
  }

  /** Replay deep-link: the game reads replay=true&game=&mode=&version=&event=&amount=. */
  function replayUrl(bm: Bookmark): string {
    const base = frontUrl.trim().replace(/\/$/, '');
    const params = new URLSearchParams({
      replay: 'true',
      rgs_url: `localhost:3601/api/rgs/${encodeURIComponent(game)}`,
      game,
      mode: bm.mode,
      version: '0',
      event: String(bm.simId),
      amount: String(1_000_000),
      currency: 'USD',
      lang: 'en',
      device: 'desktop',
    });
    return `${base}/?${params.toString()}`;
  }
</script>

<div class="panel">
  <div class="head">
    <h3>Bookmarks</h3>
    <button onclick={refresh}>↻</button>
  </div>
  {#if bookmarks.length === 0}
    <p class="dim">No bookmarks — ★ from the live feed or Analyze.</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>Mode</th>
          <th class="num">Event</th>
          <th class="num">Win</th>
          <th>Note</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each bookmarks as bm (bm.id)}
          <tr>
            <td>{bm.mode}</td>
            <td class="num">{bm.simId}</td>
            <td class="num">{fmtX(bm.payoutX100 / 100)}</td>
            <td class="dim">{bm.description}</td>
            <td class="actions">
              <button title="Force on next spin" onclick={() => onPlay(bm.mode, bm.simId)}>
                ▶ Play
              </button>
              <a
                href={replayUrl(bm)}
                target="_blank"
                rel="noreferrer"
                class="replay-link"
                title="Open in replay mode (new tab)"
              >
                Replay
              </a>
              <button class="danger" title="Delete" onclick={() => remove(bm.id)}>✕</button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    margin-bottom: 8px;
  }

  .head h3 {
    margin: 0;
  }

  .actions {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .actions button {
    padding: 2px 8px;
    font-size: 12px;
  }

  .replay-link {
    color: var(--accent);
    font-size: 12px;
    text-decoration: none;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 2px 8px;
  }

  .replay-link:hover {
    border-color: var(--accent);
  }
</style>
