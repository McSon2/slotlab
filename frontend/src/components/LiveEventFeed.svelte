<script lang="ts">
  import { onMount } from 'svelte';
  import { api, bookmarksApi, fmtX, type SpinEvent } from '../lib/api';
  import { run } from '../lib/stores.svelte';
  import Icon from './Icon.svelte';

  interface Props {
    game: string;
    onReforce?: (mode: string, simId: number) => void;
  }

  const { game, onReforce }: Props = $props();

  let feed = $state<SpinEvent[]>([]);
  let paused = $state(false);
  /** SSE link to the server: `retrying` while it's down (it comes back on its own). */
  let link = $state<'connecting' | 'live' | 'retrying'>('connecting');
  /** Row whose bookmark request is in flight. */
  let bookmarking = $state<string | null>(null);

  onMount(() => {
    let es: EventSource | null = null;
    let retry: ReturnType<typeof setTimeout> | null = null;
    let closed = false;

    function connect() {
      es = new EventSource('/api/ui/events');
      es.onopen = () => (link = 'live');
      es.onmessage = (msg) => {
        link = 'live';
        if (paused) return;
        try {
          const ev = JSON.parse(msg.data) as SpinEvent;
          if (ev.type === 'spin' && ev.game === game) {
            feed = [ev, ...feed].slice(0, 100);
          }
        } catch {
          /* non-JSON message ignored */
        }
      };
      es.onerror = () => {
        link = 'retrying';
        // The browser retries by itself unless the server answered with an
        // error status (readyState CLOSED): then reconnect by hand.
        if (es?.readyState === EventSource.CLOSED) {
          es.close();
          es = null;
          if (!closed) retry = setTimeout(connect, 3000);
        }
      };
    }

    connect();
    return () => {
      closed = true;
      if (retry) clearTimeout(retry);
      es?.close();
    };
  });

  function rowKey(ev: SpinEvent): string {
    return `${ev.sessionID}-${ev.mode}-${ev.simId}`;
  }

  async function bookmark(ev: SpinEvent) {
    bookmarking = rowKey(ev);
    const ok = await run(
      () =>
        bookmarksApi.add({
          game: ev.game,
          mode: ev.mode,
          simId: ev.simId,
          payoutX100: ev.payoutX100,
          description: `${fmtX(ev.payoutX100 / 100)} (live)`,
        }),
      { ok: `Bookmarked ${ev.mode} #${ev.simId}`, error: 'Could not add the bookmark' },
    );
    bookmarking = null;
    if (ok !== undefined) {
      window.dispatchEvent(
        new CustomEvent('slotlab:bookmarks-changed', { detail: { game: ev.game } }),
      );
    }
  }

  async function reforce(ev: SpinEvent) {
    // The page owns the force (form, "forced" chip, toast): hand it over.
    if (onReforce) {
      onReforce(ev.mode, ev.simId);
      return;
    }
    await run(() => api.setForce({ game: ev.game, mode: ev.mode, simId: ev.simId }), {
      ok: `Forced ${ev.mode} #${ev.simId} — next spin`,
      error: 'Could not force the book',
    });
  }

  function shortSession(id: string): string {
    const parts = id.split('-');
    return parts.length > 2 ? parts.slice(2).join('-') : id;
  }
</script>

<div class="panel feed">
  <div class="feed-head">
    <h3>Live spins</h3>
    <span role="status" class="link-state">
      {#if link === 'retrying'}
        <span class="pill yellow tiny">disconnected — retrying</span>
      {/if}
    </span>
    <button onclick={() => (paused = !paused)} aria-pressed={paused}>
      {paused ? 'Resume' : 'Pause'}
    </button>
    <button onclick={() => (feed = [])} disabled={feed.length === 0}>Clear feed</button>
  </div>
  {#if feed.length === 0}
    <p class="dim">No spins yet — play in a viewport.</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>Session</th>
          <th>Mode</th>
          <th class="num">Event</th>
          <th class="num">Win</th>
          <th><span class="sr-only">Actions</span></th>
        </tr>
      </thead>
      <tbody>
        {#each feed as ev, i (i)}
          <tr class:forced={ev.forced}>
            <td class="dim">{shortSession(ev.sessionID)}</td>
            <td>{ev.mode}</td>
            <td class="num">{ev.simId}</td>
            <td class="num" class:win={ev.payoutX100 > 0}>
              {fmtX(ev.payoutX100 / 100)}
              {#if ev.forced}<span class="pill yellow">forced</span>
              {:else if ev.pooled}<span class="pill" title="Drawn among the bonus books only">bonus-only</span>{/if}
            </td>
            <td class="actions">
              <button
                title="Bookmark"
                aria-label="Bookmark {ev.mode} #{ev.simId}"
                disabled={bookmarking === rowKey(ev)}
                onclick={() => bookmark(ev)}
              ><Icon name="star" size={13} /></button>
              <button
                title="Force this book again on the next spin"
                aria-label="Force {ev.mode} #{ev.simId} again on the next spin"
                onclick={() => reforce(ev)}
              ><Icon name="refresh" size={13} /></button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  .feed {
    max-height: 420px;
    overflow-y: auto;
  }

  .feed-head {
    display: flex;
    gap: var(--sp-2);
    align-items: center;
    margin-bottom: var(--sp-2);
  }

  .feed-head h3 {
    margin: 0;
  }

  /* Pushes the buttons right, holds the connection pill next to the title. */
  .link-state {
    flex: 1;
  }

  .win {
    color: var(--pass);
  }

  .actions {
    white-space: nowrap;
  }

  .actions button {
    padding: 3px 7px;
    line-height: 0;
  }

  tr.forced td {
    background: color-mix(in oklch, var(--gold) var(--tint-weak), transparent);
  }
</style>
