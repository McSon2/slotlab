<script lang="ts">
  import { api, fmtPct, fmtX, fmtOneIn, type ModeTiers, type TierSpec } from '../../lib/api';
  import { run } from '../../lib/stores.svelte';

  interface Props {
    game: string;
    selectedMode: string;
    tiers: { report: ModeTiers; spec: TierSpec }[];
    ladderSource: string;
  }

  const { game, selectedMode, tiers, ladderSource }: Props = $props();

  /** Steps typed in the ladder editor, for the selected mode only. */
  let stepsDraft = $state('');
  let customTiers = $state<ModeTiers | null>(null);
  let tiersError = $state<string | null>(null);
  let measuring = $state(false);

  const currentTiers = $derived(tiers.find((t) => t.report.mode === selectedMode));

  /** The editor follows the selected mode until it is edited. */
  $effect(() => {
    const spec = currentTiers?.spec;
    stepsDraft = spec ? spec.thresholds.slice(1, -1).join(', ') : '';
    customTiers = null;
    tiersError = null;
  });

  async function measureCustomLadder() {
    if (!game || !selectedMode) return;
    const steps = stepsDraft
      .split(/[\s,]+/)
      .map((v) => Number(v.replace(/_/g, '')))
      .filter((v) => Number.isFinite(v));
    tiersError = null;
    if (!steps.length) {
      tiersError = 'Enter at least one step, e.g. 1, 2, 5, 25.';
      return;
    }
    measuring = true;
    const r = await run(
      () => api.customTiers(game, { mode: selectedMode, steps, durations: currentTiers?.spec.durations }),
      { error: 'Could not measure this ladder' },
    );
    measuring = false;
    if (r) customTiers = r.report;
  }

  /** Seconds per hour of play the ladder eats, at ~4 s of spin otherwise. */
  function shareOfSpinTime(seconds: number): number {
    return seconds / (seconds + 4);
  }
</script>

<section class="page">
      <p class="overline">
        Celebration tiers — {selectedMode}
        <span class="faint">({currentTiers?.report.source ?? ladderSource})</span>
      </p>
      {#if !currentTiers}
        <p class="dim">No ladder for this mode.</p>
      {:else}
        {@const t = customTiers ?? currentTiers.report}
        <div class="statline hero">
          <div>
            <span class="dim">Any plaque</span>
            <strong class:warn-num={t.plaqueOneIn <= 10}>
              {fmtOneIn(t.plaqueOneIn)}{#if t.plaqueOneIn <= 10}<span class="sr-only"> (too frequent)</span>{/if}
            </strong>
          </div>
          <div><span class="dim">Plaque rate</span><strong>{fmtPct(t.plaqueRate, 3)}</strong></div>
          <div>
            <span class="dim">Celebration / spin</span>
            <strong>{t.plaqueSecondsPerSpin.toFixed(2)} s</strong>
          </div>
          <div>
            <span class="dim">Share of spin time</span>
            <strong>{fmtPct(shareOfSpinTime(t.plaqueSecondsPerSpin), 1)}</strong>
          </div>
        </div>

        {#each t.warnings as w (w)}
          <div class="banner warn">{w}</div>
        {/each}

        <table class="tier-table">
          <thead>
            <tr>
              <th>Tier</th>
              <th class="num">From</th>
              <th class="num">P(≥)</th>
              <th class="num">1 in</th>
              <th class="num">Exactly</th>
              <th class="num">Hold</th>
              <th class="num">s / spin</th>
              <th class="num">RTP</th>
            </tr>
          </thead>
          <tbody>
            {#each t.rows as r (r.level)}
              <tr class:plaque={r.plaque}>
                <td>
                  <span class="tier-name">{r.name}</span>
                  {#if r.plaque}<span class="pill blue tiny">plaque</span>{/if}
                </td>
                <td class="num faint">{r.threshold >= 1 ? fmtX(r.threshold, 0) : fmtX(r.threshold, 2)}</td>
                <td class="num">{r.atLeast > 0 ? fmtPct(r.atLeast, 3) : '—'}</td>
                <td
                  class="num"
                  class:warn-num={r.plaque && r.atLeastOneIn <= 10}
                >
                  {fmtOneIn(r.atLeastOneIn)}{#if r.plaque && r.atLeastOneIn <= 10}<span class="sr-only"> (too frequent)</span>{/if}
                </td>
                <td class="num faint">{fmtPct(r.exactly, 3)}</td>
                <td class="num faint">{r.duration ? r.duration.toFixed(1) + ' s' : '—'}</td>
                <td class="num">{r.secondsPerSpin >= 0.005 ? r.secondsPerSpin.toFixed(2) : '—'}</td>
                <td class="num faint">{fmtPct(r.rtpContribution, 2)}</td>
              </tr>
            {/each}
          </tbody>
        </table>

        <div class="ladder-editor">
          <p class="overline" id="ladder-label">Try another ladder</p>
          <div class="ladder-row">
            <input
              bind:value={stepsDraft}
              aria-labelledby="ladder-label"
              aria-describedby="ladder-help"
              aria-invalid={tiersError ? 'true' : undefined}
              onkeydown={(e) => e.key === 'Enter' && !measuring && measureCustomLadder()}
              placeholder="0.1, 1, 2, 5, 25, 50, 100, 500"
              spellcheck="false"
            />
            <button onclick={measureCustomLadder} disabled={measuring} aria-busy={measuring}>
              {measuring ? 'Measuring…' : 'Measure'}
            </button>
            {#if customTiers}
              <button class="ghost" onclick={() => (customTiers = null)}>Reset</button>
            {/if}
          </div>
          {#if tiersError}<p class="field-error" role="alert">{tiersError}</p>{/if}
          <p class="note" id="ladder-help">
            Steps are multiples of the BASE bet, low bound first — the same
            eight numbers as a <code>winLevelThresholds.ts</code> row. Measuring
            here changes nothing in the game: it answers "how often would this
            plaque show up, and how many seconds would it cost", which is the
            only way to pick a threshold.
          </p>
        </div>
      {/if}
</section>

<style>
  .tier-table tr.plaque {
    background: color-mix(in oklch, var(--gold) var(--tint-weak), transparent);
  }

  .tier-name {
    font-weight: 600;
  }

  .tier-name + .pill {
    margin-left: 6px;
  }

  .warn-num {
    color: var(--warn);
  }

  .ladder-editor {
    margin-top: 20px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
  }

  .ladder-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .ladder-row input {
    flex: 1;
    max-width: 460px;
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
  }

  .statline.hero {
    padding: 4px 0 18px;
  }

  .statline.hero strong {
    font-size: var(--fs-xl);
  }

  .note {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    max-width: 780px;
    margin: 12px 0 0;
    line-height: 1.55;
  }
</style>
