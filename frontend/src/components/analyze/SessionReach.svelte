<script lang="ts">
  import { api, type SessionReach } from '../../lib/api';
  import { oneIn, pct } from '../../lib/populations';
  import { errorText } from '../../lib/stores.svelte';
  import { storageGetJson, storageSetJson } from '../../lib/storage';

  /**
   * « Sur N spins, combien de sessions n'atteignent jamais X ? » — la session dont
   * le joueur repart sans rien de gros. Exact (lu dans la LUT, spins indépendants),
   * pour n'importe quel nombre de spins et n'importe quel multiplicateur.
   */
  let { game, mode }: { game: string; mode: string } = $props();

  const KEY = 'slotlab.analyze.reach';
  let spins = $state(6000);
  let x = $state(500);
  let result = $state<SessionReach | null>(null);
  let error = $state<string | null>(null);

  const saved = storageGetJson<{ spins?: number; x?: number }>(KEY, {});
  if (typeof saved?.spins === 'number' && saved.spins > 0) spins = saved.spins;
  if (typeof saved?.x === 'number' && saved.x > 0) x = saved.x;

  $effect(() => {
    const g = game;
    const m = mode;
    const n = Math.round(spins);
    const t = x;
    if (!g || !m || !(n > 0) || !(t > 0)) return;
    const timer = setTimeout(async () => {
      storageSetJson(KEY, { spins: n, x: t });
      try {
        result = await api.sessionReach(g, m, n, t);
        error = null;
      } catch (e) {
        error = errorText(e);
      }
    }, 200);
    return () => clearTimeout(timer);
  });

  const fmtN = (v: number) => Math.round(v).toLocaleString('en-US');
</script>

<section class="page reach">
  <p class="overline">Best win in a session</p>
  <div class="panel body">
    <div class="inputs">
      <label>
        <span>Spins</span>
        <input type="number" min="1" step="100" bind:value={spins} />
      </label>
      <label>
        <span>Win of at least</span>
        <span class="with-unit"><input type="number" min="0.1" step="any" bind:value={x} /><i>x</i></span>
      </label>
    </div>
    {#if error}
      <p class="field-error" role="alert">Could not compute: {error}</p>
    {:else if result}
      <div class="answer" aria-live="polite">
        <strong class="big">{pct(result.never)}</strong>
        <p>
          of {mode} sessions of {fmtN(result.spins)} spins never pay {result.x}x or more —
          their best win stays below.
        </p>
      </div>
      <p class="faint detail">
        {#if result.oneIn}
          {oneIn(result.oneIn)} spins pays {result.x}x+ · {result.expected.toFixed(result.expected < 10 ? 2 : 1)} per
          session on average · {pct(1 - result.never)} of sessions see at least one
        {:else}
          No spin of this mode ever pays {result.x}x.
        {/if}
      </p>
    {/if}
  </div>
</section>

<style>
  .reach {
    margin-bottom: 18px;
  }

  .body {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 14px 28px;
  }

  .inputs {
    display: flex;
    gap: 14px;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: var(--fs-2xs);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-faint);
  }

  input {
    width: 104px;
    font-family: var(--font-mono);
  }

  .with-unit {
    display: flex;
    align-items: center;
    gap: 5px;
  }

  .with-unit i {
    font-style: normal;
    font-family: var(--font-mono);
    color: var(--text-dim);
    text-transform: none;
    letter-spacing: 0;
  }

  .answer {
    display: flex;
    align-items: baseline;
    gap: 12px;
    flex: 1 1 320px;
  }

  .big {
    font-family: var(--font-mono);
    font-size: var(--fs-xl);
    font-weight: 680;
    color: var(--gold);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .answer p {
    margin: 0;
    font-size: var(--fs-base);
    color: var(--text-dim);
    line-height: 1.45;
  }

  .detail {
    flex-basis: 100%;
    margin: 0;
    font-size: var(--fs-xs);
    font-family: var(--font-mono);
  }

  .field-error {
    margin: 0;
  }
</style>
