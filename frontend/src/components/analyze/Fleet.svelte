<script lang="ts">
  import { onMount } from 'svelte';
  import { api, fmtPct, fmtX, fmtOneIn, type ModeStats } from '../../lib/api';
  import { app, errorText } from '../../lib/stores.svelte';
  import { fx } from '../../lib/populations';

  /**
   * The fleet answers the only question a single game cannot: is this RTP,
   * this hit rate, this ceiling normal for us? Loaded on demand — it walks
   * every registered game.
   */
  let fleet = $state<
    { game: string; mode: string; stats: ModeStats; modes: number; topCost: number }[]
  >([]);
  let fleetLoading = $state(false);
  let error = $state<string | null>(null);
  /** Games whose stats could not be read (the others still show). */
  let unread = $state<string[]>([]);

  onMount(async () => {
    fleetLoading = true;
    try {
      const games = app.games.filter((g) => g.loaded);
      const settled = await Promise.allSettled(
        games
          .map(async (g) => {
            const s = await api.gameStats(g.name);
            const cheapest = [...s.modes].sort((a, b) => a.stats.cost - b.stats.cost)[0];
            const topCost = Math.max(...s.modes.map((m) => m.stats.cost));
            return cheapest
              ? { game: g.name, mode: cheapest.mode, stats: cheapest.stats, modes: s.modes.length, topCost }
              : null;
          }),
      );
      unread = games.filter((_, i) => settled[i].status === 'rejected').map((g) => g.name);
      fleet = settled
        .map((r) => (r.status === 'fulfilled' ? r.value : null))
        .filter((x): x is NonNullable<typeof x> => x !== null)
        .sort((a, b) => b.stats.maxPayout - a.stats.maxPayout);
    } catch (e) {
      error = errorText(e);
    } finally {
      fleetLoading = false;
    }
  });
</script>

<section class="page">
  {#if error}<div class="banner error">Could not read the fleet: {error}</div>{/if}
  {#if unread.length}<div class="banner warn">Not read: {unread.join(', ')}</div>{/if}
      <p class="overline">
        The fleet <span class="faint">base mode of every registered game</span>
      </p>
      {#if fleetLoading}
        <p class="dim" role="status">Reading every game…</p>
      {:else if fleet.length === 0}
        <p class="dim">No other game registered.</p>
      {:else}
        <table>
          <thead>
            <tr>
              <th>Game</th>
              <th>Base</th>
              <th class="num">RTP</th>
              <th class="num">Hit</th>
              <th class="num">Std ÷ cost</th>
              <th class="num">Max win</th>
              <th class="num">Max win HR</th>
              <th class="num">Modes</th>
              <th class="num">Dearest</th>
            </tr>
          </thead>
          <tbody>
            {#each fleet as f (f.game)}
              <tr class:self={f.game === app.selectedGame}>
                <td>{f.game}{#if f.game === app.selectedGame}<span class="sr-only"> (this game)</span>{/if}</td>
                <td class="faint">{f.mode}</td>
                <td class="num">{fmtPct(f.stats.rtp, 2)}</td>
                <td class="num">{fmtPct(f.stats.hitRate, 2)}</td>
                <td class="num">{(f.stats.stdDev / Math.max(1, f.stats.cost)).toFixed(1)}</td>
                <td class="num">{fmtX(f.stats.maxPayout, 0)}</td>
                <td class="num faint">{fmtOneIn(f.stats.maxPayoutOneIn)}</td>
                <td class="num faint">{f.modes}</td>
                <td class="num faint">{fx(f.topCost)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        <p class="note">
          The market reference the studio works against: a 30 000× ceiling on a
          hit rate in the 5–33 % band, and a base standard deviation under 60.
          A game that stands out on one of these columns is either the pitch or
          the problem — this table is the only place that says which.
        </p>
      {/if}
</section>

<style>
  tr.self td {
    color: var(--gold);
  }

  .note {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    max-width: 780px;
    margin: var(--sp-3) 0 0;
    line-height: 1.55;
  }
</style>
