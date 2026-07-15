<script lang="ts">
  import {
    optimizeApi,
    fmtPct,
    fmtX,
    fmtOneIn,
    type OptimizePreview,
    type Feasibility,
  } from '../lib/api';
  import { app, refreshGames, selectedGame } from '../lib/stores.svelte';

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
  let preview = $state<OptimizePreview | null>(null);
  let busy = $state(false);
  let applyMsg = $state<string | null>(null);
  let error = $state<string | null>(null);

  const VOL_PRESETS = [
    { label: 'LOW', v: 4 },
    { label: 'MEDIUM', v: 11 },
    { label: 'HIGH', v: 22 },
    { label: 'EXTREME', v: 40 },
  ];

  $effect(() => {
    if (game && !game.modes.some((m) => m.name === mode)) {
      mode = game.modes[0]?.name ?? '';
    }
  });

  $effect(() => {
    // Load feasibility when the mode changes.
    if (app.selectedGame && mode) void loadFeas(app.selectedGame, mode);
  });

  async function loadFeas(g: string, m: string) {
    preview = null;
    applyMsg = null;
    try {
      const r = await optimizeApi.feasibility(g, m);
      feas = r.feasibility;
      // Seed the targets with the current values.
      targetRtpPct = Math.round(r.feasibility.currentRtp * 10000) / 100;
      // Current zero estimated from the selected mode's stats.
      const cur = game?.modes.find((x) => x.name === m);
      if (cur) targetZeroPct = Math.round((1 - cur.hitRate) * 1000) / 10;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
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

  async function runPreview() {
    if (!app.selectedGame || !mode) return;
    busy = true;
    error = null;
    applyMsg = null;
    try {
      preview = await optimizeApi.preview(app.selectedGame, mode, body());
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function apply() {
    if (!app.selectedGame || !mode || !preview) return;
    if (
      !confirm(
        `Write the rebalanced LUT of ${mode} into publish_files?\n` +
          `The old one is saved in _prebalance/. Books untouched.`,
      )
    )
      return;
    busy = true;
    try {
      const r = await optimizeApi.apply(app.selectedGame, mode, body());
      if (r.error) throw new Error(r.error);
      applyMsg = `LUT written (RTP ${fmtPct(r.achievedRtp, 4)}). Backup: ${r.backup}`;
      await refreshGames();
      await loadFeas(app.selectedGame, mode);
      preview = null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function diffClass(before: number, after: number): string {
    if (Math.abs(after - before) < 1e-9) return '';
    return after > before ? 'up' : 'down';
  }
</script>

{#if !game}
  <div class="panel"><p class="dim">Select a loaded game.</p></div>
{:else}
  <p class="overline">Rebalance the LUT — {game.name}</p>
  <p class="sub">
    Adjust the lookup table <strong>weights</strong> to target an RTP and a
    volatility, without touching the books (directly re-publishable). SlotLab can
    only redistribute across the existing outcomes — it doesn't invent any.
  </p>

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
      <div class="presets">
        {#each VOL_PRESETS as p (p.label)}
          <button class="ghost" onclick={() => (targetVol = p.v)}>{p.label}</button>
        {/each}
      </div>
    {/if}
    <label class="check">
      <input type="checkbox" bind:checked={capMostLikely} />
      <span>Cap most-likely ≥ 1/1000</span>
    </label>
    <button class="primary" onclick={runPreview} disabled={busy}>
      {busy ? 'Computing…' : 'Preview'}
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
        <button class="ghost" onclick={() => (maxWinOneIn = Math.round(maxWinOneIn / 2))}>×2 accessible</button>
        <button class="ghost" onclick={() => (maxWinOneIn = maxWinOneIn * 2)}>÷2 accessible</button>
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

  {#if feas}
    <div class="statline feas">
      <div><span class="dim">Current RTP</span><strong>{fmtPct(feas.currentRtp, 3)}</strong></div>
      <div><span class="dim">Current vol</span><strong>{feas.currentVol.toFixed(2)}</strong></div>
      <div><span class="dim">Achievable RTP</span><strong>{fmtPct(feas.minRtp, 1)} – {fmtPct(feas.maxRtp, 1)}</strong></div>
      <div><span class="dim">Max vol ~</span><strong>{feas.maxVol.toFixed(1)}</strong></div>
    </div>
  {/if}

  {#if error}<div class="banner error">{error}</div>{/if}
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
            <td class="num">{fmtPct(preview.before.rtp, 4)}</td>
            <td class="num {diffClass(preview.before.rtp, preview.after.rtp)}">{fmtPct(preview.after.rtp, 4)}</td>
          </tr>
          <tr>
            <td>Std / cost (volatility)</td>
            <td class="num">{preview.before.costAdjVolatility.toFixed(2)}</td>
            <td class="num {diffClass(preview.before.costAdjVolatility, preview.after.costAdjVolatility)}">{preview.after.costAdjVolatility.toFixed(2)}</td>
          </tr>
          <tr>
            <td>Hit rate</td>
            <td class="num">{fmtPct(preview.before.hitRate)}</td>
            <td class="num {diffClass(preview.before.hitRate, preview.after.hitRate)}">{fmtPct(preview.after.hitRate)}</td>
          </tr>
          <tr>
            <td>Zero-win rate</td>
            <td class="num">{fmtPct(preview.before.zeroRate)}</td>
            <td class="num {diffClass(preview.after.zeroRate, preview.before.zeroRate)}">{fmtPct(preview.after.zeroRate)}</td>
          </tr>
          <tr>
            <td>Max win — frequency</td>
            <td class="num">{fmtOneIn(preview.before.maxPayoutOneIn)}</td>
            <td class="num">{fmtOneIn(preview.after.maxPayoutOneIn)}</td>
          </tr>
          <tr>
            <td>Break-even (P ≥ bet)</td>
            <td class="num">{fmtPct(preview.before.breakevenRate)}</td>
            <td class="num {diffClass(preview.before.breakevenRate, preview.after.breakevenRate)}">{fmtPct(preview.after.breakevenRate)}</td>
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
        <button class="primary" onclick={apply} disabled={busy}>
          Apply & write the LUT
        </button>
        <span class="faint">Auto backup in _prebalance/ · books untouched · immediate reload</span>
      </div>
    </section>
  {/if}
{/if}

<style>
  .sub {
    font-size: 12.5px;
    color: var(--text-dim);
    margin: -4px 0 18px;
    max-width: 680px;
    line-height: 1.5;
  }

  .controls {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 12px 16px;
    padding-bottom: 16px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 16px;
  }

  .controls label {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .controls label > span:first-child {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-faint);
  }

  .controls label.check {
    flex-direction: row;
    align-items: center;
    gap: 6px;
    padding-bottom: 7px;
    font-size: 12.5px;
    color: var(--text-dim);
  }

  .presets {
    display: flex;
    gap: 4px;
    padding-bottom: 2px;
  }

  .presets button {
    padding: 4px 9px;
    font-size: 11px;
  }

  .anchors {
    border-bottom: 1px solid var(--border);
  }

  .anchor-label {
    width: 100%;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-faint);
    margin-bottom: 2px;
  }

  .sep-v {
    width: 1px;
    align-self: stretch;
    background: var(--border);
    margin: 0 4px;
  }

  .hint {
    font-size: 11px;
    padding-bottom: 7px;
  }

  .statline.feas {
    padding: 12px 0 18px;
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

  .banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
</style>
