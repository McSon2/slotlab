<script lang="ts">
  import { onMount } from 'svelte';
  import { api, bookmarksApi, fmtX, type SpinEvent } from '../lib/api';

  interface Props {
    game: string;
    onReforce?: (mode: string, simId: number) => void;
  }

  const { game, onReforce }: Props = $props();

  let feed = $state<SpinEvent[]>([]);
  let paused = $state(false);

  onMount(() => {
    const es = new EventSource('/api/ui/events');
    es.onmessage = (msg) => {
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
    return () => es.close();
  });

  async function bookmark(ev: SpinEvent) {
    await bookmarksApi.add({
      game: ev.game,
      mode: ev.mode,
      simId: ev.simId,
      payoutX100: ev.payoutX100,
      description: `${fmtX(ev.payoutX100 / 100)} (live)`,
    });
  }

  async function reforce(ev: SpinEvent) {
    await api.setForce({ game: ev.game, mode: ev.mode, simId: ev.simId });
    onReforce?.(ev.mode, ev.simId);
  }

  function shortSession(id: string): string {
    const parts = id.split('-');
    return parts.length > 2 ? parts.slice(2).join('-') : id;
  }
</script>

<div class="panel feed">
  <div class="feed-head">
    <h3>Live spins</h3>
    <button onclick={() => (paused = !paused)}>
      {paused ? 'Resume' : 'Pause'}
    </button>
    <button onclick={() => (feed = [])}>Clear</button>
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
          <th></th>
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
              {#if ev.forced}<span class="pill yellow">forced</span>{/if}
            </td>
            <td class="actions">
              <button title="Bookmark" onclick={() => bookmark(ev)}>★</button>
              <button title="Re-force this book" onclick={() => reforce(ev)}>⟳</button>
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
    gap: 8px;
    align-items: baseline;
    margin-bottom: 8px;
  }

  .feed-head h3 {
    flex: 1;
    margin: 0;
  }

  .win {
    color: var(--green);
  }

  .actions button {
    padding: 2px 8px;
    font-size: 12px;
  }

  tr.forced td {
    background: rgba(210, 153, 34, 0.05);
  }
</style>
