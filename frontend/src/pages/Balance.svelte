<script lang="ts">
  import {
    optimizeApi,
    fmtPct,
    fmtX,
    fmtOneIn,
    type OptimizePreview,
    type Feasibility,
  } from '../lib/api';
  import { app, refreshGames, selectedGame, notify, errorText } from '../lib/stores.svelte';
  import ConfirmButton from '../components/ConfirmButton.svelte';

  const game = $derived(selectedGame());

  let mode = $state('');
  let targetRtpPct = $state(96.5);
  let useVol = $state(false);
  let targetVol = $state(15);
  let capMostLikely = $state(true);
  // Targeted distribution anchors.
  let useZero = $state(false);
  let targetZeroPct = $state(89);
  let useMaxWin = $state(false);
  let maxWinOneIn = $state(1_000_000);
  let useTail = $state(false);
  let tailCap = $state(0.0018);

  let feas = $state<Feasibility | null>(null);
  let feasBusy = $state(false);
  let feasError = $state<string | null>(null);
  let preview = $state<OptimizePreview | null>(null);
  let busy = $state(false);
  let applying = $state(false);
  let applyMsg = $state<string | null>(null);
  let error = $state<string | null>(null);

  const VOL_PRESETS = [
    { label: 'LOW', v: 4 },
    { label: 'MEDIUM', v: 11 },
    { label: 'HIGH', v: 22 },
    { label: 'EXTREME', v: 40 },
  ];

  /** Above this, the upper RTP bound is "anything" in practice. */
  const RTP_UNBOUNDED = 10; // 1000 %

  $effect(() => {
    if (game && !game.modes.some((m) => m.name === mode)) {
      mode = game.modes[0]?.name ?? '';
    }
  });

  $effect(() => {
    // Load feasibility when the mode changes.
    if (app.selectedGame && mode) void loadFeas(app.selectedGame, mode);
  });

  /** Drops the answer of a feasibility request superseded by a newer one. */
  let feasSeq = 0;

  async function loadFeas(g: string, m: string) {
    const seq = ++feasSeq;
    preview = null;
    applyMsg = null;
    feas = null;
    feasError = null;
    feasBusy = true;
    try {
      const r = await optimizeApi.feasibility(g, m);
      if (seq !== feasSeq) return;
      feas = r.feasibility;
      // Seed the targets with the current values.
      targetRtpPct = Math.round(r.feasibility.currentRtp * 10000) / 100;
      // Current zero estimated from the selected mode's stats.
      const cur = game?.modes.find((x) => x.name === m);
      if (cur) targetZeroPct = Math.round((1 - cur.hitRate) * 1000) / 10;
    } catch (e) {
      if (seq === feasSeq) feasError = errorText(e);
    } finally {
      if (seq === feasSeq) feasBusy = false;
    }
  }

  function body() {
    return {
      targetRtp: targetRtpPct / 100,
      targetVol: useVol ? targetVol : null,
      capMostLikely,
      targetZeroRate: useZero ? targetZeroPct / 100 : null,
      maxWinOneIn: useMaxWin ? maxWinOneIn : null,
      targetTail10k: useTail ? tailCap : null,
    };
  }

  async function runPreview(e?: SubmitEvent) {
    e?.preventDefault();
    if (!app.selectedGame || !mode || busy) return;
    busy = true;
    error = null;
    applyMsg = null;
    try {
      preview = await optimizeApi.preview(app.selectedGame, mode, body());
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  async function apply() {
    if (!app.selectedGame || !mode || !preview) return;
    busy = true;
    applying = true;
    error = null;
    try {
      const r = await optimizeApi.apply(app.selectedGame, mode, body());
      if (r.error) throw new Error(r.error);
      applyMsg = `LUT written (RTP ${fmtPct(r.achievedRtp, 2)}). Backup: ${r.backup}`;
      notify(`${mode}: rebalanced LUT written`, 'ok');
      await refreshGames();
      await loadFeas(app.selectedGame, mode);
      preview = null;
    } catch (e) {
      error = errorText(e);
      notify(`Apply failed: ${errorText(e)}`, 'error');
    } finally {
      busy = false;
      applying = false;
    }
  }

  /**
   * Direction of a before → after change. The arrow shows where the value
   * went; the color says whether that is the wanted direction (`invert`
   * for metrics where lower is better, e.g. the zero-win rate).
   */
  function diff(before: number, after: number, invert = false) {
    if (Math.abs(after - before) < 1e-9) return { cls: '', arrow: '', word: '' };
    const up = after > before;
    return {
      cls: up !== invert ? 'up' : 'down',
      arrow: up ? '▲' : '▼',
      word: up ? 'increased' : 'decreased',
    };
  }

  const rtpRange = $derived.by(() => {
    if (!feas) return { text: '', title: '' };
    const title = `${fmtPct(feas.minRtp, 2)} – ${fmtPct(feas.maxRtp, 2)}`;
    const hi = feas.maxRtp > RTP_UNBOUNDED ? 'unbounded' : fmtPct(feas.maxRtp, 2);
    return { text: `${fmtPct(feas.minRtp, 2)} – ${hi}`, title };
  });
</script>

{#snippet delta(before: number, after: number, text: string, invert = false)}
  {@const d = diff(before, after, invert)}
  <td class="num {d.cls}">
    {#if d.arrow}<span class="arrow" aria-hidden="true">{d.arrow}</span>{/if}{text}{#if d.word}<span class="sr-only"> ({d.word})</span>{/if}
  </td>
{/snippet}

{#if !game}
  <div class="panel"><p class="dim">Select a loaded game.</p></div>
{:else}
  <p class="overline">Rebalance the LUT — {game.name}</p>
  <p class="sub">
    Adjust the lookup table <strong>weights</strong> to target an RTP and a
    volatility, without touching the books (directly re-publishable). SlotLab can
    only redistribute across the existing outcomes — it doesn't invent any.
  </p>

  <form onsubmit={runPreview} novalidate>
    <section class="controls">
      <label>
        <span>Mode</span>
        <select bind:value={mode}>
          {#each game.modes as m (m.name)}<option value={m.name}>{m.name}</option>{/each}
        </select>
      </label>
      <label>
        <span>Target RTP (%)</span>
        <input type="number" bind:value={targetRtpPct} min="80" max="99" step="0.1" style="width: 90px" />
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={useVol} />
        <span>Target volatility (std/cost)</span>
      </label>
      {#if useVol}
        <label>
          <span>Volatility</span>
          <input type="number" bind:value={targetVol} min="0.5" max="80" step="0.5" style="width: 80px" />
        </label>
        <div class="presets" role="group" aria-label="Volatility presets">
          {#each VOL_PRESETS as p (p.label)}
            <button
              type="button"
              class="ghost"
              aria-pressed={targetVol === p.v}
              aria-label="Set volatility to {p.v} ({p.label.toLowerCase()})"
              onclick={() => (targetVol = p.v)}
            >{p.label}</button>
          {/each}
        </div>
      {/if}
      <label class="check">
        <input type="checkbox" bind:checked={capMostLikely} />
        <span>Cap most-likely ≥ 1/1000</span>
      </label>
      <button class="primary" type="submit" disabled={busy}>
        {busy && !applying ? 'Computing…' : 'Preview'}
      </button>
    </section>

    <!-- Targeted distribution anchors -->
    <section class="controls anchors">
      <span class="anchor-label">Targeted distribution <span class="faint">(optional — pin one part, the rest rebalances the RTP)</span></span>
      <label class="check">
        <input type="checkbox" bind:checked={useZero} />
        <span>Zero-win rate</span>
      </label>
      {#if useZero}
        <label>
          <span>0 payout (%)</span>
          <input type="number" bind:value={targetZeroPct} min="0" max="99" step="0.5" style="width: 80px" />
        </label>
        <span class="faint hint">lower = more frequent but smaller wins</span>
      {/if}
      <span class="sep-v"></span>
      <label class="check">
        <input type="checkbox" bind:checked={useMaxWin} />
        <span>Max win accessibility</span>
      </label>
      {#if useMaxWin}
        <label>
          <span>Max win: 1 in</span>
          <input type="number" bind:value={maxWinOneIn} min="1000" step="100000" style="width: 130px" />
        </label>
        <div class="presets">
          <button type="button" class="ghost" onclick={() => (maxWinOneIn = Math.round(maxWinOneIn / 2))}>×2 accessible</button>
          <button type="button" class="ghost" onclick={() => (maxWinOneIn = maxWinOneIn * 2)}>÷2 accessible</button>
        </div>
      {/if}
      <span class="sep-v"></span>
      <label class="check">
        <input type="checkbox" bind:checked={useTail} />
        <span>Cap Tail P(≥10000x)</span>
      </label>
      {#if useTail}
        <label>
          <span>P(≥10000x) ≤</span>
          <input type="number" bind:value={tailCap} min="0" max="0.01" step="0.0001" style="width: 90px" />
        </label>
        <span class="faint hint">Stake: ≤ 0.002 for 2★, ≤ 0.005 for 3★</span>
      {/if}
    </section>
  </form>

  <div class="feas-zone" aria-live="polite">
    {#if feasBusy}
      <p class="faint feas-loading">Loading what {mode}'s outcomes allow…</p>
    {:else if feasError}
      <p class="field-error" role="alert">Feasibility unavailable for {mode}: {feasError}</p>
    {:else if feas}
      <div class="statline feas">
        <div><span class="dim">Current RTP</span><strong>{fmtPct(feas.currentRtp, 2)}</strong></div>
        <div><span class="dim">Current vol</span><strong>{feas.currentVol.toFixed(2)}</strong></div>
        <div>
          <span class="dim">Achievable RTP</span>
          <strong title="Exact bounds: {rtpRange.title}">{rtpRange.text}</strong>
        </div>
        <div><span class="dim">Max vol ~</span><strong>{feas.maxVol.toFixed(2)}</strong></div>
      </div>
    {/if}
  </div>

  {#if error}<div class="banner error" role="alert">{error}</div>{/if}
  {#if applyMsg}<div class="banner ok">{applyMsg}</div>{/if}

  {#if preview}
    {@const r = preview.result}
    <div class="banner" class:ok={r.ok}>
      <span>{r.message}</span>
      <span class="verdict-pills">
        <span class="pill {r.rtpHit ? 'green' : 'yellow'}">RTP {r.rtpHit ? 'hit' : 'approached'}</span>
        {#if useVol}<span class="pill {r.volHit ? 'green' : 'yellow'}">Vol {r.volHit ? 'hit' : 'bounded'}</span>{/if}
      </span>
    </div>

    <section>
      <p class="overline">Before → After — {mode}</p>
      <table class="diff">
        <thead>
          <tr><th>Metric</th><th class="num">Before</th><th class="num">After</th></tr>
        </thead>
        <tbody>
          <tr>
            <td>RTP</td>
            <td class="num">{fmtPct(preview.before.rtp, 2)}</td>
            {@render delta(preview.before.rtp, preview.after.rtp, fmtPct(preview.after.rtp, 2))}
          </tr>
          <tr>
            <td>Std / cost (volatility)</td>
            <td class="num">{preview.before.costAdjVolatility.toFixed(2)}</td>
            {@render delta(preview.before.costAdjVolatility, preview.after.costAdjVolatility, preview.after.costAdjVolatility.toFixed(2))}
          </tr>
          <tr>
            <td>Hit rate</td>
            <td class="num">{fmtPct(preview.before.hitRate)}</td>
            {@render delta(preview.before.hitRate, preview.after.hitRate, fmtPct(preview.after.hitRate))}
          </tr>
          <tr>
            <td>Zero-win rate</td>
            <td class="num">{fmtPct(preview.before.zeroRate)}</td>
            {@render delta(preview.before.zeroRate, preview.after.zeroRate, fmtPct(preview.after.zeroRate), true)}
          </tr>
          <tr>
            <td>Max win — frequency</td>
            <td class="num">{fmtOneIn(preview.before.maxPayoutOneIn)}</td>
            <td class="num">{fmtOneIn(preview.after.maxPayoutOneIn)}</td>
          </tr>
          <tr>
            <td>Break-even (P ≥ bet)</td>
            <td class="num">{fmtPct(preview.before.breakevenRate)}</td>
            {@render delta(preview.before.breakevenRate, preview.after.breakevenRate, fmtPct(preview.after.breakevenRate))}
          </tr>
          <tr>
            <td>
              Most likely
              {#if preview.after.mostLikelyOneIn < 1000}
                <span class="pill red">&lt; 1/1000 — non-compliant</span>
              {/if}
            </td>
            <td class="num">{fmtOneIn(preview.before.mostLikelyOneIn)}</td>
            <td class="num" class:fail={preview.after.mostLikelyOneIn < 1000}>{fmtOneIn(preview.after.mostLikelyOneIn)}</td>
          </tr>
          <tr>
            <td>Max win (fixed)</td>
            <td class="num">{fmtX(preview.before.maxPayout, 0)}</td>
            <td class="num">{fmtX(preview.after.maxPayout, 0)}</td>
          </tr>
          <tr>
            <td>Unique payouts</td>
            <td class="num">{preview.before.uniquePayoutCount}</td>
            <td class="num">{preview.after.uniquePayoutCount}</td>
          </tr>
        </tbody>
      </table>

      <div class="apply-row">
        <ConfirmButton
          variant="primary"
          onconfirm={apply}
          disabled={busy}
          confirmLabel="Write the {mode} LUT? (old one backed up)"
        >
          {applying ? 'Writing the LUT…' : 'Apply & write the LUT'}
        </ConfirmButton>
        <span class="faint">Auto backup in _prebalance/ · books untouched · immediate reload</span>
      </div>
    </section>
  {:else}
    <div class="empty" aria-live="polite">
      {#if busy}
        <p>Computing new weights for {mode}…</p>
      {:else}
        <p>
          <strong>Preview</strong> computes new weights and shows the before/after
          distribution — nothing is written until you <strong>Apply</strong>.
        </p>
        <p class="faint">Pick your targets above, then press Preview (or Enter).</p>
      {/if}
    </div>
  {/if}
{/if}

<style>
  .sub {
    font-size: var(--fs-sm);
    color: var(--text-dim);
    margin: calc(-1 * var(--sp-1)) 0 var(--sp-4);
    max-width: 680px;
    line-height: 1.5;
  }

  .controls {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--sp-3) var(--sp-4);
    padding-bottom: var(--sp-4);
    border-bottom: 1px solid var(--border);
    margin-bottom: var(--sp-4);
  }

  .controls label {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .controls label > span:first-child {
    font-size: var(--fs-2xs);
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-faint);
  }

  .controls label.check {
    flex-direction: row;
    align-items: center;
    gap: 6px;
    padding-bottom: 7px;
    font-size: var(--fs-sm);
    color: var(--text-dim);
  }

  .presets {
    display: flex;
    gap: var(--sp-1);
    padding-bottom: 2px;
  }

  .presets button {
    padding: 4px 9px;
    font-size: var(--fs-xs);
  }

  .presets button[aria-pressed='true'] {
    color: var(--gold);
  }

  .anchor-label {
    width: 100%;
    font-size: var(--fs-2xs);
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-faint);
    margin-bottom: 2px;
  }

  .sep-v {
    width: 1px;
    align-self: stretch;
    background: var(--border);
    margin: 0 var(--sp-1);
  }

  .hint {
    font-size: var(--fs-xs);
    padding-bottom: 7px;
  }

  .feas-zone {
    min-height: 58px;
  }

  .statline.feas {
    padding: var(--sp-3) 0 var(--sp-4);
  }

  .feas-loading {
    margin: 0;
    padding: var(--sp-4) 0;
    font-size: var(--fs-sm);
  }

  .feas-zone .field-error {
    margin: 0 0 var(--sp-4);
  }

  .verdict-pills {
    display: flex;
    gap: 6px;
    margin-left: auto;
  }

  .diff td:first-child {
    color: var(--text);
  }

  .diff .up {
    color: var(--pass);
  }

  .diff .down {
    color: var(--warn);
  }

  .arrow {
    font-size: var(--fs-2xs);
    margin-right: 5px;
  }

  .diff .fail {
    color: var(--fail);
    font-weight: 700;
  }

  .apply-row {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-top: 18px;
  }

  .empty {
    padding: var(--sp-6) var(--sp-5);
    border: 1px dashed var(--border);
    border-radius: var(--r-m);
    color: var(--text-dim);
    font-size: var(--fs-sm);
    max-width: 680px;
  }

  .empty p {
    margin: 0;
  }

  .empty p + p {
    margin-top: var(--sp-1);
  }
</style>
