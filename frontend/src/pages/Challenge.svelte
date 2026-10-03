<script lang="ts">
  import { untrack } from 'svelte';
  import {
    challengeApi,
    fmtPct,
    fmtOneIn,
    type ChallengeReport,
    type ChallengeVerdict,
  } from '../lib/api';
  import { app, selectedGame, errorText } from '../lib/stores.svelte';

  const game = $derived(selectedGame());

  let mode = $state('');
  let minBet = $state(0.05);
  let awardsText = $state('15, 35, 50, 88, 100');
  let targetsText = $state('');

  let report = $state<ChallengeReport | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  // Custom single-challenge simulator.
  let simAward = $state(50);
  let simTarget = $state(888);
  let simMinBet = $state(0.05);
  let sim = $state<ChallengeReport | null>(null);
  let simBusy = $state(false);
  let simError = $state<string | null>(null);

  $effect(() => {
    if (game && !game.modes.some((m) => m.name === mode)) {
      // Challenges are ground in the base game → default to cheapest mode.
      const cheapest = [...game.modes].sort((a, b) => a.cost - b.cost)[0];
      mode = cheapest?.name ?? '';
    }
  });

  // Recompute when the game or the mode changes; the other fields are
  // applied with Recalculate / Enter (not on every keystroke).
  $effect(() => {
    if (app.selectedGame && mode) untrack(() => void analyze());
  });

  function parseNums(text: string): number[] {
    return text
      .split(/[,\s]+/)
      .map((s) => Number(s))
      .filter((n) => isFinite(n) && n > 0);
  }

  async function analyze() {
    if (!app.selectedGame || !mode) return;
    busy = true;
    error = null;
    try {
      report = await challengeApi.analyze(app.selectedGame, {
        mode,
        minBet,
        awards: parseNums(awardsText),
        targets: parseNums(targetsText),
      });
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  async function runSim() {
    if (!app.selectedGame || !mode) return;
    simBusy = true;
    simError = null;
    try {
      sim = await challengeApi.analyze(app.selectedGame, {
        mode,
        minBet: simMinBet,
        awards: [simAward],
        targets: [simTarget],
      });
    } catch (e) {
      simError = errorText(e);
    } finally {
      simBusy = false;
    }
  }

  function submit(fn: () => Promise<void>) {
    return (e: SubmitEvent) => {
      e.preventDefault();
      void fn();
    };
  }

  /**
   * Verdict scale, on the boost (award ÷ expected wager, in RTP points) —
   * the zones are multiples of the house edge: a boost below the edge leaves
   * the hunter losing. Color is never the only cue: each verdict also has a
   * marker glyph (and sweet spots are bold, dead cells dim).
   */
  const VERDICTS: Record<
    ChallengeVerdict,
    { label: string; lo: number; hi: number; hint: string; mark: string; cls: string }
  > = {
    dead: {
      label: 'Dead',
      lo: 0,
      hi: 1,
      hint: 'boost below your edge: the hunter still loses — pure showcase',
      mark: '○',
      cls: 'v-dead',
    },
    thin: {
      label: 'Borderline',
      lo: 1,
      hi: 1.5,
      hint: 'boost just above your edge: only some hunters bite',
      mark: '◐',
      cls: 'v-thin',
    },
    sweet: {
      label: 'Sweet spot',
      lo: 1.5,
      hi: 3,
      hint: 'hunted AND largely funded by your edge',
      mark: '●',
      cls: 'v-sweet',
    },
    generous: {
      label: 'Generous',
      lo: 3,
      hi: 5,
      hint: 'hunted hard, but the claim is cheap — you subsidise it',
      mark: '▲',
      cls: 'v-generous',
    },
    giveaway: {
      label: 'Giveaway',
      lo: 5,
      hi: Infinity,
      hint: 'sniped almost instantly, the award is pure cost',
      mark: '▲▲',
      cls: 'v-giveaway',
    },
  };

  /** A zone's boost range for this edge, e.g. "5.25–10.5 %". */
  function zoneRange(v: { lo: number; hi: number }, edge: number): string {
    const pct = (k: number) => fmtBoostNum(k * edge);
    if (v.lo === 0) return `< ${pct(v.hi)} %`;
    if (!isFinite(v.hi)) return `> ${pct(v.lo)} %`;
    return `${pct(v.lo)}–${pct(v.hi)} %`;
  }

  function fmtUsd(v: number): string {
    if (!isFinite(v)) return '∞';
    return (
      '$' +
      v.toLocaleString('en-US', {
        maximumFractionDigits: v < 100 ? 2 : 0,
      })
    );
  }

  function fmtInt(v: number): string {
    if (!isFinite(v)) return '∞';
    return Math.round(v).toLocaleString('en-US');
  }

  /** Boost as a bare percentage number: 0.0042 → "0.42", 2.05 → "205". */
  function fmtBoostNum(b: number): string {
    const pct = b * 100;
    const digits = pct >= 100 ? 0 : pct >= 10 ? 1 : 2;
    return pct.toLocaleString('en-US', {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    });
  }

  /** "+0.42 %" — what the award adds to the hunter's RTP. */
  function fmtBoost(b: number): string {
    if (b <= 0) return '—';
    return `+${fmtBoostNum(b)} %`;
  }

  function suggestedVerdict(boost: number, edge: number): ChallengeVerdict {
    const k = boost / edge;
    return k < 1.5 ? 'thin' : k <= 3 ? 'sweet' : 'generous';
  }

  const simT = $derived(sim?.targets[0]);
  const simA = $derived(sim?.awards[0]);
  const simBoost = $derived(sim?.boostMatrix[0]?.[0] ?? 0);
  const simVerdict = $derived(sim ? VERDICTS[sim.verdictMatrix[0][0]] : null);
  /** Operator net on a lone expected grind: edge income − award. */
  const simLoneNet = $derived(
    simT && simA ? simT.hunterGrindCost - simA.award : 0,
  );
</script>

{#if !game}
  <div class="panel"><p class="dim">Select a loaded game.</p></div>
{:else}
  <p class="overline">Challenge calculator — {game.name}</p>
  <p class="sub">
    Size Stake challenges (“reach <strong>N×</strong> at a bet ≥ min bet → fixed
    award, claimMax 1”) from the real published distribution. Boost = award ÷
    expected wager — the RTP the award adds to the hunter's grind; it has to beat
    your edge to make the hunt +EV. Your break-even = award ÷ edge. A round's multiplier =
    win ÷ total bet (a bonus buy dilutes its multiplier → only the base game can
    claim).
  </p>

  <form class="controls" onsubmit={submit(analyze)} novalidate>
    <label>
      <span>Mode</span>
      <select bind:value={mode}>
        {#each game.modes as m (m.name)}
          <option value={m.name}>{m.name} ({m.cost}×)</option>
        {/each}
      </select>
    </label>
    <label>
      <span>Min bet ($)</span>
      <input type="number" bind:value={minBet} min="0.01" step="0.01" style="width: 80px" />
    </label>
    <label class="grow">
      <span>Awards ($, list)</span>
      <input type="text" bind:value={awardsText} placeholder="15, 35, 50, 88, 100" />
    </label>
    <label class="grow">
      <span>Targets (×)</span>
      <input type="text" bind:value={targetsText} placeholder="auto — standard ladder" />
    </label>
    <button class="primary" type="submit" disabled={busy}>
      {busy ? 'Computing…' : 'Recalculate'}
    </button>
  </form>

  {#if error}
    <p class="field-error section-error" role="alert">{error}</p>
  {/if}

  {#if report}
    <div class="statline stats">
      <div><span class="dim">RTP</span><strong>{fmtPct(report.rtp, 2)}</strong></div>
      <div><span class="dim">Edge</span><strong>{fmtPct(report.houseEdge, 2)}</strong></div>
      <div><span class="dim">Max win</span><strong>{fmtInt(report.maxMult)}×</strong></div>
      <div><span class="dim">Max win odds</span><strong>{fmtOneIn(report.maxMultOneIn)}</strong></div>
      <div><span class="dim">Min bet</span><strong>{fmtUsd(report.minBet)}</strong></div>
    </div>

    <p class="overline">Boost matrix — targets × awards (min bet {fmtUsd(report.minBet)})</p>
    <div class="panel table-wrap">
      <table>
        <thead>
          <tr>
            <th>Target</th>
            <th class="num">Odds</th>
            <th class="num">Expected grind</th>
            <th class="num">Grind cost</th>
            {#each report.awards as a (a.award)}
              <th class="num award-col">{fmtUsd(a.award)}</th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each report.targets as t, ti (t.target)}
            <tr>
              <td class="target-cell">{fmtInt(t.target)}×</td>
              <td class="num dim">
                {t.probability > 0 ? fmtOneIn(t.oneIn) : 'never'}
              </td>
              <td class="num dim">
                {t.probability > 0
                  ? `${fmtInt(t.expectedRounds)} spins · ${fmtUsd(t.expectedTurnover)}`
                  : '—'}
              </td>
              <td class="num dim">{t.probability > 0 ? fmtUsd(t.hunterGrindCost) : '—'}</td>
              {#each report.awards as a, ai (a.award)}
                {@const boost = report.boostMatrix[ti][ai]}
                {@const v = VERDICTS[report.verdictMatrix[ti][ai]]}
                <td class="num ev-cell {v.cls}" title="{v.label} — {v.hint}">
                  <span class="mark" aria-hidden="true">{v.mark}</span>{fmtBoost(boost)}<span class="sr-only">, {v.label}</span>
                </td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <div class="legend">
      <ul class="legend-items" aria-label="Verdict legend">
        {#each Object.values(VERDICTS) as v (v.label)}
          <li class="legend-item {v.cls}" title={v.hint}>
            <span class="mark" aria-hidden="true">{v.mark}</span>
            <span class="legend-label">{v.label}</span>
            <span class="faint legend-range">{zoneRange(v, report.houseEdge)}</span>
          </li>
        {/each}
      </ul>
      <span class="dim legend-note">
        The hunt pays only when the boost beats your {fmtPct(report.houseEdge, 2)} edge.
        Sweet spot = hunted, but the grind is long → your edge funds a large part of the award.
      </span>
    </div>

    <p class="overline">Recommendations per award</p>
    <div class="cards">
      {#each report.awards as a (a.award)}
        <div class="panel card">
          <div class="card-head">
            <span class="card-award">{fmtUsd(a.award)}</span>
            {#if a.suggested}
              <span class="card-target">→ {fmtInt(a.suggested.target)}×</span>
            {:else}
              <span class="card-target dim">no viable target ≥ 100×</span>
            {/if}
          </div>
          {#if a.suggested}
            {@const sv = VERDICTS[suggestedVerdict(a.suggested.boost, report.houseEdge)]}
            <div class="kv">
              <span>Odds</span><span>{fmtOneIn(a.suggested.oneIn)}</span>
            </div>
            <div class="kv">
              <span>Boost</span>
              <span class={sv.cls} title="{sv.label} — {sv.hint}">
                <span class="mark" aria-hidden="true">{sv.mark}</span>{fmtBoost(a.suggested.boost)}<span class="sr-only">, {sv.label}</span>
              </span>
            </div>
          {/if}
          <div class="kv">
            <span>Break-even</span>
            <span>{fmtInt(a.breakEvenRounds)} spins · {fmtUsd(a.breakEvenTurnover)}</span>
          </div>
          <p class="card-note dim">
            Hardest target whose boost still reaches 1.5 × your edge
            ({fmtBoostNum(1.5 * report.houseEdge)} %) — maximum grind per claim.
          </p>
        </div>
      {/each}
    </div>
  {/if}

  <p class="overline">Challenge simulator</p>
  <form class="controls" onsubmit={submit(runSim)} novalidate>
    <label>
      <span>Award ($)</span>
      <input type="number" bind:value={simAward} min="1" step="1" style="width: 90px" />
    </label>
    <label>
      <span>Target (×)</span>
      <input type="number" bind:value={simTarget} min="1" step="1" style="width: 100px" />
    </label>
    <label>
      <span>Min bet ($)</span>
      <input type="number" bind:value={simMinBet} min="0.01" step="0.01" style="width: 80px" />
    </label>
    <button class="primary" type="submit" disabled={simBusy}>
      {simBusy ? 'Computing…' : 'Evaluate'}
    </button>
  </form>

  {#if simError}
    <p class="field-error section-error" role="alert">{simError}</p>
  {/if}

  {#if sim && simT && simA && simVerdict}
    <div class="panel sim">
      <div class="sim-verdict {simVerdict.cls}">
        <span class="mark" aria-hidden="true">{simVerdict.mark}</span>
        <strong>{simVerdict.label}</strong> — {simVerdict.hint}
      </div>
      <div class="sim-grid">
        <div class="kv"><span>Odds of reaching {fmtInt(simT.target)}×</span>
          <span>{simT.probability > 0 ? fmtOneIn(simT.oneIn) : 'never (max ' + fmtInt(sim.maxMult) + '×)'}</span></div>
        <div class="kv"><span>Boost</span><span>{fmtBoost(simBoost)} (edge {fmtPct(sim.houseEdge, 2)})</span></div>
        <div class="kv"><span>Expected grind (1 hunter)</span>
          <span>{simT.probability > 0 ? `${fmtInt(simT.expectedRounds)} spins · ${fmtUsd(simT.expectedTurnover)} turnover` : '—'}</span></div>
        <div class="kv"><span>Grind cost for the hunter</span>
          <span>{simT.probability > 0 ? fmtUsd(simT.hunterGrindCost) : '—'}</span></div>
        <div class="kv"><span>Your break-even</span>
          <span>{fmtInt(simA.breakEvenRounds)} spins · {fmtUsd(simA.breakEvenTurnover)}</span></div>
        <div class="kv"><span>Net on a “solo” claim</span>
          <span class={simLoneNet >= 0 ? 'pos' : 'neg'}>
            {simLoneNet >= 0 ? '+' : '−'}{fmtUsd(Math.abs(simLoneNet))}
          </span></div>
      </div>
      <p class="dim sim-note">
        “Solo claim” = a single hunter grinds exactly their expectation, then
        claims. Several hunters in parallel / over-grinding → any turnover beyond
        your break-even is net profit.
      </p>
    </div>
  {/if}
{/if}

<style>
  .sub {
    font-size: var(--fs-sm);
    color: var(--text-dim);
    margin: calc(-1 * var(--sp-1)) 0 var(--sp-4);
    max-width: 720px;
    line-height: 1.5;
  }

  .controls {
    display: flex;
    align-items: flex-end;
    flex-wrap: wrap;
    gap: var(--sp-3) var(--sp-4);
    margin-bottom: var(--sp-4);
  }

  .controls label {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  /* Same uppercase micro-label as Balance / CrowdSim. */
  .controls label > span:first-child {
    font-size: var(--fs-2xs);
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-faint);
  }

  .controls label.grow {
    flex: 1;
    min-width: 180px;
  }

  .section-error {
    margin: calc(-1 * var(--sp-2)) 0 var(--sp-4);
  }

  .stats {
    margin-bottom: var(--sp-4);
  }

  .table-wrap {
    overflow-x: auto;
    padding: 0;
  }

  .award-col {
    border-left: 1px solid var(--border);
  }

  .target-cell {
    font-weight: 640;
  }

  .ev-cell {
    border-left: 1px solid var(--border);
  }

  .mark {
    display: inline-block;
    margin-right: 5px;
    font-size: var(--fs-2xs);
    font-family: var(--font-body);
    opacity: 0.85;
  }

  /* Verdict scale: color + marker + weight (never color alone). */
  .v-dead { color: var(--text-faint); }
  .v-thin { color: var(--gold); }
  .v-sweet { color: var(--pass); }
  .v-generous { color: var(--warn); }
  .v-giveaway { color: var(--fail); }

  .ev-cell.v-sweet,
  .legend-item.v-sweet { font-weight: 700; }
  .ev-cell.v-generous,
  .ev-cell.v-giveaway,
  .ev-cell.v-thin { font-weight: 560; }

  .legend {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--sp-2) var(--sp-4);
    margin: var(--sp-3) 0 var(--sp-6);
    font-size: var(--fs-sm);
  }

  .legend-items {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2) var(--sp-4);
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .legend-item {
    display: inline-flex;
    align-items: baseline;
    gap: 4px;
    font-size: var(--fs-xs);
    font-weight: 560;
  }

  .legend-item .mark {
    margin-right: 0;
  }

  .legend-range {
    font-family: var(--font-mono);
    font-weight: 400;
    font-size: var(--fs-2xs);
  }

  .legend-note {
    font-size: var(--fs-sm);
  }

  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: var(--sp-3);
    margin-bottom: var(--sp-6);
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .card-head {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    margin-bottom: var(--sp-1);
  }

  .card-award {
    font-size: var(--fs-lg);
    font-weight: 700;
    color: var(--gold);
  }

  .card-target {
    font-size: var(--fs-md);
    font-weight: 640;
  }

  .kv {
    display: flex;
    justify-content: space-between;
    gap: var(--sp-3);
    font-size: var(--fs-sm);
  }

  .kv > span:first-child {
    color: var(--text-dim);
  }

  .card-note {
    font-size: var(--fs-xs);
    margin: var(--sp-1) 0 0;
  }

  .sim {
    max-width: 620px;
  }

  .sim-verdict {
    font-size: var(--fs-base);
    margin-bottom: var(--sp-3);
  }

  .sim-grid {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }

  .sim-note {
    font-size: var(--fs-xs);
    margin: var(--sp-3) 0 0;
  }

  .pos { color: var(--pass); font-weight: 640; }
  .neg { color: var(--fail); font-weight: 640; }
</style>
