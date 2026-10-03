<script lang="ts">
  import { bookmarksApi, fmtX, rgsUrlFor, type Bookmark } from '../lib/api';
  import { errorText, run } from '../lib/stores.svelte';
  import ConfirmButton from './ConfirmButton.svelte';
  import Icon from './Icon.svelte';

  interface Props {
    game: string;
    frontUrl: string;
    onPlay: (mode: string, simId: number) => void;
  }

  const { game, frontUrl, onPlay }: Props = $props();

  let bookmarks = $state<Bookmark[]>([]);
  let loading = $state(false);
  let loadError = $state<string | null>(null);

  export async function refresh() {
    loading = true;
    try {
      bookmarks = await bookmarksApi.list(game);
      loadError = null;
    } catch (e) {
      loadError = errorText(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    if (game) void refresh();
  });

  // Bookmarks are added from the live feed, Search books and Analyze: each
  // announces it on `window`, the list follows without a manual refresh.
  $effect(() => {
    const onChanged = (e: Event) => {
      const changed = (e as CustomEvent<{ game?: string } | undefined>).detail?.game;
      if (!changed || changed === game) void refresh();
    };
    window.addEventListener('slotlab:bookmarks-changed', onChanged);
    return () => window.removeEventListener('slotlab:bookmarks-changed', onChanged);
  });

  async function remove(bm: Bookmark) {
    const ok = await run(() => bookmarksApi.remove(bm.id), {
      ok: `Bookmark ${bm.mode} #${bm.simId} deleted`,
      error: 'Could not delete the bookmark',
    });
    if (ok !== undefined) await refresh();
  }

  /** Replay deep-link: the game reads replay=true&game=&mode=&version=&event=&amount=. */
  function replayUrl(bm: Bookmark): string {
    const base = frontUrl.trim().replace(/\/$/, '');
    const params = new URLSearchParams({
      replay: 'true',
      rgs_url: rgsUrlFor(game),
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
    <button
      class="icon-btn"
      onclick={refresh}
      disabled={loading}
      aria-label="Refresh bookmarks"
      title="Refresh bookmarks"
    ><Icon name="refresh" size={14} /></button>
  </div>
  {#if loadError}
    <p class="field-error" role="alert">Could not load the bookmarks: {loadError}</p>
  {/if}
  {#if bookmarks.length === 0}
    {#if !loadError}
      <p class="dim">
        No bookmarks yet — use the star in the live feed, in Search books or in Analyze.
      </p>
    {/if}
  {:else}
    <table>
      <thead>
        <tr>
          <th>Mode</th>
          <th class="num">Event</th>
          <th class="num">Win</th>
          <th>Note</th>
          <th><span class="sr-only">Actions</span></th>
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
              <button
                title="Force on next spin"
                aria-label="Force {bm.mode} #{bm.simId} on the next spin"
                onclick={() => onPlay(bm.mode, bm.simId)}
              >
                <Icon name="play" size={10} /> Play
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
              <ConfirmButton
                variant="plain"
                onconfirm={() => remove(bm)}
                confirmLabel="Delete?"
                title="Delete this bookmark"
                ariaLabel="Delete bookmark {bm.mode} #{bm.simId}"
              ><Icon name="close" size={12} /></ConfirmButton>
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
    align-items: center;
    margin-bottom: var(--sp-2);
  }

  .head h3 {
    margin: 0;
  }

  .icon-btn {
    padding: 5px 7px;
    line-height: 0;
  }

  .actions {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .actions :global(button) {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 8px;
    min-height: 24px;
    font-size: var(--fs-sm);
  }

  .replay-link {
    color: var(--gold);
    font-size: var(--fs-sm);
    text-decoration: none;
    border: 1px solid var(--border);
    border-radius: var(--r-s);
    padding: 2px 8px;
  }

  .replay-link:hover {
    border-color: var(--gold);
  }
</style>
