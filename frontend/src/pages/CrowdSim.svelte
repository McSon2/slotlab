<script lang="ts">
  import { fmtPct } from '../lib/api';
  import { app, selectedGame } from '../lib/stores.svelte';
  import Chart from '../components/Chart.svelte';

  interface CrowdResult {
    pop: number;
    bustRate: number;
    empiricalRtp: number;
    avgSpinsPlayed: number;
    checkpoints: number[];
    popCurve: number[];
    balanceP5: number[];
    balanceP50: number[];
    balanceP95: number[];
    balanceMean: number[];
    avgMaxDrawdown: number;
    pctBelow50: number;
    pctBelow10: number;
    maxWinStreak: number;
    maxLoseStreak: number;
    medianFirstBigWinSpin: number | null;
    playerVolatility: 'low' | 'medium' | 'high';
    avgMaxPeak: number;
  }

  interface ConvergenceResult {
    checkpoints: number[];
    rtpCurve: number[];
    finalRtp: number;
  }

  let mode = $state('');
  let players = $state(10000);
  let spins = $state(1000);
  let bankroll = $state(100);
  let seed = $state(42);
  let convSpins = $state(10_000_000);

  let result = $state<CrowdResult | null>(null);
  let convergence = $state<ConvergenceResult | null>(null);
  let busy = $state(false);
  let convBusy = $state(false);
  let error = $state<string | null>(null);

  const game = $derived(selectedGame());

  $effect(() => {
    if (game && !game.modes.some((m) => m.name === mode)) {
      mode = game.modes[0]?.name ?? '';
    }
  });

  async function post<T>(url: string, body: unknown): Promise<T> {
    const r = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    const data = await r.json();
    if (!r.ok) throw new Error(data.error ?? r.statusText);
    return data;
  }

  async function run() {
    if (!app.selectedGame || !mode) return;
    busy = true;
    error = null;
    try {
      result = await post<CrowdResult>('/api/ui/crowdsim', {
        game: app.selectedGame,
        mode,
        players,
        spins,
        startBalance: bankroll,
        seed,
      });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function runConvergence() {
    if (!app.selectedGame || !mode) return;
    convBusy = true;
    error = null;
    try {
      convergence = await post<ConvergenceResult>('/api/ui/convergence', {
        game: app.selectedGame,
        mode,
        spins: convSpins,
        seed,
      });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      convBusy = false;
    }
  }

  const balanceChart = $derived.by(() => {
    if (!result) return null;
    return {
      type: 'line' as const,
      data: {
        labels: result.checkpoints.map(String),
        datasets: [
          {
            label: 'p95',
            data: result.balanceP95,
            borderColor: 'rgba(63, 185, 80, 0.7)',
            backgroundColor: 'rgba(63, 185, 80, 0.08)',
            pointRadius: 0,
            fill: false,
          },
          {
            label: 'median',
            data: result.balanceP50,
            borderColor: '#4493f8',
            backgroundColor: 'rgba(68, 147, 248, 0.15)',
            pointRadius: 0,
            fill: '-1',
          },
          {
            label: 'p5',
            data: result.balanceP5,
            borderColor: 'rgba(248, 81, 73, 0.7)',
            backgroundColor: 'rgba(248, 81, 73, 0.08)',
            pointRadius: 0,
            fill: '-1',
          },
          {
            label: 'mean',
            data: result.balanceMean,
            borderColor: '#d29922',
            borderDash: [6, 4],
            pointRadius: 0,
            fill: false,
          },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        scales: {
          x: { title: { display: true, text: 'spins' }, ticks: { maxTicksLimit: 12 } },
          y: { title: { display: true, text: 'balance (bets)' } },
        },
      },
    };
  });

  const popChart = $derived.by(() => {
    if (!result) return null;
    return {
      type: 'line' as const,
      data: {
        labels: result.checkpoints.map(String),
        datasets: [
          {
            label: 'PoP (players in profit)',
            data: result.popCurve,
            borderColor: '#4493f8',
            pointRadius: 0,
            fill: false,
          },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        scales: {
          x: { ticks: { maxTicksLimit: 12 } },
          y: { min: 0, max: 1 },
        },
      },
    };
  });

  const analyticRtp = $derived(game?.modes.find((m) => m.name === mode)?.rtp ?? 0);

  const convChart = $derived.by(() => {
    if (!convergence) return null;
    const n = convergence.checkpoints.length;
    return {
      type: 'line' as const,
      data: {
        labels: convergence.checkpoints.map((c) =>
          c >= 1e6 ? `${(c / 1e6).toFixed(1)}M` : c >= 1e3 ? `${(c / 1e3).toFixed(0)}k` : String(c),
        ),
        datasets: [
          {
            label: 'Cumulative RTP (empirical)',
            data: convergence.rtpCurve,
            borderColor: '#c9a227',
            pointRadius: 0,
            fill: false,
          },
          {
            label: 'Analytical RTP (target)',
            data: Array(n).fill(analyticRtp),
            borderColor: 'rgba(120, 200, 150, 0.55)',
            borderDash: [5, 4],
            pointRadius: 0,
            fill: false,
          },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        scales: {
          x: { title: { display: true, text: 'spins (log-spaced)' }, ticks: { maxTicksLimit: 15 } },
          y: { title: { display: true, text: 'RTP' } },
        },
      },
    };
  });

  const volPill = $derived(
    result?.playerVolatility === 'low'
      ? 'green'
      : result?.playerVolatility === 'high'
        ? 'red'
        : 'yellow',
  );
</script>

{#if !game}
  <div class="panel"><p class="dim">Select a loaded game.</p></div>
{:else}
  <div class="panel controls">
    <label>
      <span>Mode</span>
      <select bind:value={mode}>
        {#each game.modes as m (m.name)}<option value={m.name}>{m.name}</option>{/each}
      </select>
    </label>
    <label>
      <span>Players</span>
      <input type="number" bind:value={players} min="100" max="1000000" step="1000" />
    </label>
    <label>
      <span>Spins</span>
      <input type="number" bind:value={spins} min="10" max="100000" step="100" />
    </label>
    <label>
      <span>Bankroll (bets)</span>
      <input type="number" bind:value={bankroll} min="1" step="50" />
    </label>
    <label>
      <span>Seed</span>
      <input type="number" bind:value={seed} />
    </label>
    <button class="primary" onclick={run} disabled={busy}>
      {busy ? 'Simulating…' : 'Simulate cohort'}
    </button>
  </div>

  {#if error}
    <div class="panel" style="border-color: var(--red); color: var(--red);">{error}</div>
  {/if}

  {#if result}
    <div class="panel">
      <h3>Results — {mode} ({players.toLocaleString('en-US')} players × {spins.toLocaleString('en-US')} spins)</h3>
      <div class="stat-grid">
        <div><span class="dim">PoP (profit)</span><strong>{fmtPct(result.pop)}</strong></div>
        <div><span class="dim">Bust</span><strong>{fmtPct(result.bustRate)}</strong></div>
        <div><span class="dim">Empirical RTP</span><strong>{fmtPct(result.empiricalRtp, 3)}</strong></div>
        <div><span class="dim">Player volatility</span><strong><span class="pill {volPill}">{result.playerVolatility.toUpperCase()}</span></strong></div>
        <div><span class="dim">Avg max drawdown</span><strong>{fmtPct(result.avgMaxDrawdown)}</strong></div>
        <div><span class="dim">Dropped below 50 %</span><strong>{fmtPct(result.pctBelow50)}</strong></div>
        <div><span class="dim">Dropped below 10 %</span><strong>{fmtPct(result.pctBelow10)}</strong></div>
        <div><span class="dim">Avg peak</span><strong>{result.avgMaxPeak.toFixed(2)}× bankroll</strong></div>
        <div><span class="dim">Max win streak</span><strong>{result.maxWinStreak}</strong></div>
        <div><span class="dim">Max lose streak</span><strong>{result.maxLoseStreak}</strong></div>
        <div><span class="dim">1st big win (median)</span><strong>{result.medianFirstBigWinSpin ?? 'never'}</strong></div>
        <div><span class="dim">Spins played (avg)</span><strong>{result.avgSpinsPlayed.toFixed(0)}</strong></div>
      </div>
    </div>

    <div class="grid-2">
      <div class="panel">
        <h3>Balance curve (p5 / median / p95)</h3>
        {#if balanceChart}<Chart config={balanceChart} height={280} />{/if}
      </div>
      <div class="panel">
        <h3>PoP over spins</h3>
        {#if popChart}<Chart config={popChart} height={280} />{/if}
      </div>
    </div>
  {/if}

  <div class="panel controls">
    <span class="group-label">RTP convergence</span>
    <label>
      <span>Spins</span>
      <select bind:value={convSpins}>
        <option value={1_000_000}>1M</option>
        <option value={10_000_000}>10M</option>
        <option value={50_000_000}>50M</option>
      </select>
    </label>
    <button class="primary" onclick={runConvergence} disabled={convBusy}>
      {convBusy ? 'Computing…' : 'Run'}
    </button>
    {#if convergence}
      <span>
        Final RTP: <strong>{fmtPct(convergence.finalRtp, 4)}</strong>
        <span class="dim">(analytical: {fmtPct(analyticRtp, 4)}, gap {fmtPct(convergence.finalRtp - analyticRtp, 2)})</span>
      </span>
    {/if}
  </div>

  {#if convergence && convChart}
    <div>
      <Chart config={convChart} height={260} />
      <p class="note">
        High-volatility slots: the very big wins are so rare that a run,
        even at several million spins, under-samples the tail and reads
        <em>below</em> target most of the time (skewed distribution). The
        curve should approach the analytical line as you raise the spins or
        change the seed. The exact truth is the RTP in the Analyze tab.
      </p>
    </div>
  {/if}
{/if}

<style>
  .controls {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    align-items: flex-end;
    margin-bottom: 12px;
  }

  .controls label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 12px;
    color: var(--text-dim);
  }

  .controls input[type='number'] {
    width: 110px;
  }

  .group-label {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-dim);
    padding-bottom: 8px;
  }

  .stat-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 10px 16px;
  }

  .stat-grid > div {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .stat-grid strong {
    font-family: var(--mono);
    font-size: 15px;
  }

  .grid-2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
    margin-bottom: 16px;
  }

  @media (max-width: 1100px) {
    .grid-2 {
      grid-template-columns: 1fr;
    }
  }

  .panel {
    margin-bottom: 16px;
  }

  .note {
    font-size: 11.5px;
    color: var(--text-faint);
    max-width: 720px;
    margin: 10px 0 0;
    line-height: 1.55;
  }
</style>
