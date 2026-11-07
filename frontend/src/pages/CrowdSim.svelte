<script lang="ts">
  import { app, selectedGame, errorText } from '../lib/stores.svelte';
  import { fmtPct } from '../lib/api';
  import { fx, pct } from '../lib/populations';
  import CohortView from '../components/crowdsim/CohortView.svelte';
  import Findings from '../components/crowdsim/Findings.svelte';
  import MutedList from '../components/MutedList.svelte';
  import { isSimKey, mutedGroups, splitSim } from '../lib/muted';
  import { mutes } from '../lib/muted.svelte';
  import {
    readCohort,
    cohortMode,
    compareCohorts,
    metricOf,
    shareOf,
    isBuy,
    mainMode,
    kpis,
    type Cohort,
    type SimResponse,
    type SimSeverity,
  } from '../lib/crowdsim';
  import { threshold } from '../lib/populations';
  import { DEFAULT_FEEL, cohortLabel, oneIn, type FeelParams, type FrustrationKey, type Payer } from '../lib/feel';

  /**
   * CrowdSim — N players × M spins on one or several modes (and optionally a
   * mixed session), bet = 1 base bet. What they lived, read automatically,
   * then every cohort in detail.
   */
  const game = $derived(selectedGame());

  let modes = $state<string[]>([]);
  let players = $state(10_000);
  let spins = $state(1_000);
  let unlimited = $state(true);
  let bankroll = $state(500);
  let seed = $state(42);
  let mixOn = $state(false);
  let shares = $state<Record<string, number>>({});
  /** What counts as a drought, a letdown, a big win (Player feel). */
  let feel = $state<FeelParams>({ ...DEFAULT_FEEL });

  let result = $state<SimResponse | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  /** `overview`, `compare`, or a cohort's name. */
  let tab = $state('overview');
  /** Section a cohort opens on (the overview's « See the fixes » opens Player feel). */
  let cohortSection = $state<'lived' | 'feel'>('lived');
  let editing = $state(true);
  /** Request of the run on screen: a fix is played with the same sample. */
  let lastBody: Record<string, unknown> | null = null;
  /** `frustration:payer` of the fix being played. */
  let simulating = $state<string | null>(null);

  // The selection follows the game: its cheapest mode by default.
  let forGame = '';
  $effect(() => {
    if (!game || game.name === forGame) return;
    forGame = game.name;
    const cheapest = [...game.modes].sort((a, b) => a.cost - b.cost)[0];
    modes = cheapest ? [cheapest.name] : [];
    shares = Object.fromEntries(game.modes.map((m) => [m.name, m.cost <= 2 ? 95 : 5]));
    result = null;
  });

  function toggleMode(name: string) {
    modes = modes.includes(name) ? modes.filter((m) => m !== name) : [...modes, name];
  }

  const totalRounds = $derived(players * spins * (modes.length + (mixOn && modes.length >= 2 ? 1 : 0)));
  const tooBig = $derived(totalRounds > 2_000_000_000);

  async function run() {
    if (!app.selectedGame || modes.length === 0 || tooBig || busy) return;
    busy = true;
    error = null;
    try {
      const body = {
        game: app.selectedGame,
        modes,
        mix: mixOn && modes.length >= 2 ? modes.map((m) => ({ mode: m, share: Math.max(0, shares[m] ?? 0) })) : [],
        players,
        spins,
        bankroll: unlimited ? null : bankroll,
        seed,
        feel: { ...feel },
      };
      result = await post(body);
      lastBody = body;
      tab = 'overview';
      cohortSection = 'lived';
      editing = false;
    } catch (e) {
      error = `Simulation failed: ${errorText(e)}`;
    } finally {
      busy = false;
    }
  }

  async function post(body: Record<string, unknown>): Promise<SimResponse> {
    const r = await fetch('/api/ui/crowdsim', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
    // An error body is not always JSON (proxy, crash): keep the status then.
    const data = await r.json().catch(() => null);
    if (!r.ok) throw new Error(data?.error ?? `${r.status} ${r.statusText || 'request failed'}`);
    if (!data) throw new Error('Empty response from the simulator');
    return data;
  }

  /**
   * Plays a player-feel fix of a cohort's mode as one more cohort — same
   * players, spins, balance and seed — then opens the comparison.
   */
  async function simulateFix(c: Cohort, frustration: FrustrationKey | 'global', payer: Payer) {
    const mode = c.modes[0]?.mode;
    if (!result || !lastBody || !mode || simulating) return;
    simulating = `${frustration}:${payer}`;
    error = null;
    try {
      const data = await post({ ...lastBody, modes: [], mix: [], fixes: [{ mode, frustration, payer }] });
      const added = data.scenarios;
      result = { ...result, scenarios: [...result.scenarios.filter((x) => !added.some((a) => a.name === x.name)), ...added] };
      tab = 'compare';
    } catch (e) {
      error = `Fix simulation failed: ${errorText(e)}`;
    } finally {
      simulating = null;
    }
  }

  function openCohort(name: string, section: 'lived' | 'feel' = 'lived') {
    cohortSection = section;
    tab = name;
  }

  function onSubmit(e: SubmitEvent) {
    e.preventDefault();
    void run();
  }

  const cohortCount = $derived(modes.length + (mixOn && modes.length >= 2 ? 1 : 0));

  const cohorts = $derived(result?.scenarios ?? []);
  $effect(() => {
    if (app.selectedGame) void mutes.load(app.selectedGame);
  });
  /** Per cohort, the readings not muted for this game (`lib/muted.ts`); the muted ones leave every count. */
  const readings = $derived(
    cohorts.map((c) => {
      const s = splitSim(readCohort(c), mutes.list, cohortMode(c));
      return { c, lines: s.shown, muted: s.muted.map((l) => ({ l, mode: cohortMode(c) })) };
    }),
  );
  const mutedAll = $derived(readings.flatMap((r) => r.muted));
  const mutedSim = $derived(mutedGroups(mutes.list, mutedAll, (x) => ({ rule: x.l.key, modes: [x.mode], title: x.l.title }), true, (m) => isSimKey(m.rule)));
  const comparisons = $derived(compareCohorts(cohorts));
  const count = (lines: { severity: SimSeverity }[], s: SimSeverity) => lines.filter((l) => l.severity === s).length;

  /** The comparison table: one row per question, one column per cohort. */
  type Row = { label: string; get: (c: Cohort) => string };
  const compareRows: Row[] = [
    { label: 'RTP (math)', get: (c) => `${fmtPct(c.rtp)} (${fmtPct(c.theoreticalRtp)})` },
    { label: 'Players in profit', get: (c) => pct(shareOf(c, 'profit') ?? 0) },
    { label: 'Median session RTP', get: (c) => fmtPct(metricOf(c, 'rtp')?.p50 ?? 0) },
    { label: 'Session RTP P5 – P95', get: (c) => `${fmtPct(metricOf(c, 'rtp')?.p5 ?? 0)} – ${fmtPct(metricOf(c, 'rtp')?.p95 ?? 0)}` },
    { label: 'Median net (bets)', get: (c) => Math.round(metricOf(c, 'net')?.p50 ?? 0).toLocaleString('en-US') },
    { label: 'Best win, median player', get: (c) => fx(metricOf(c, 'best')?.p50 ?? 0) },
    { label: 'Never win 100x', get: (c) => pct(shareOf(c, 'never100') ?? 0) },
    { label: 'Bonuses per player (median)', get: (c) => (c.hasBonus && !isBuy(c) ? String(Math.round(metricOf(c, 'bonuses')?.p50 ?? 0)) : '—') },
    { label: 'Never a bonus', get: (c) => (c.hasBonus && !isBuy(c) ? pct(shareOf(c, 'noBonus') ?? 0) : '—') },
    { label: 'Spins to the first bonus (median)', get: (c) => (c.hasBonus && !isBuy(c) ? String(Math.round(metricOf(c, 'firstBonus')?.p50 ?? 0)) : '—') },
    {
      label: 'Median bonus paid',
      get: (c) => {
        const b = mainMode(c)?.populations.find((p) => p.kind === 'bonus') ?? mainMode(c)?.populations.find((p) => p.kind === 'tier');
        return b ? fx(b.median) : '—';
      },
    },
    {
      label: 'Bonuses under 10x',
      get: (c) => {
        const b = mainMode(c)?.populations.find((p) => p.kind === 'bonus') ?? mainMode(c)?.populations.find((p) => p.kind === 'tier');
        return b ? pct(threshold(b, 10)?.below ?? 0) : '—';
      },
    },
    { label: 'Longest dry run (median)', get: (c) => (isBuy(c) ? '—' : `${Math.round(metricOf(c, 'dead')?.p50 ?? 0)} spins`) },
    { label: 'Hit the max win', get: (c) => pct(shareOf(c, 'maxWin') ?? 0) },
    // Player feel, exact on each cohort's lookup table (a fix cohort: the fixed one).
    { label: 'Feel · hit rate', get: (c) => (c.feel ? pct(c.feel.metrics.hitRate) : '—') },
    { label: 'Feel · bonus', get: (c) => (c.feel?.metrics.bonusOneIn ? oneIn(c.feel.metrics.bonusOneIn) : '—') },
    {
      label: 'Feel · drought sessions',
      get: (c) => (c.feel?.metrics.drought != null ? `${pct(c.feel.metrics.drought)} (lived ${c.feel.observed.droughtSessions ? pct(c.feel.observed.droughts / c.feel.observed.droughtSessions) : '—'})` : '—'),
    },
    { label: 'Feel · median bonus', get: (c) => (c.feel?.metrics.bonusMedian != null ? fx(c.feel.metrics.bonusMedian) : '—') },
    {
      label: 'Feel · letdown chance',
      get: (c) => (c.feel?.metrics.letdown != null ? `${pct(c.feel.metrics.letdown)} (lived ${c.feel.observed.multiBonusSessions ? pct(c.feel.observed.letdowns / c.feel.observed.multiBonusSessions) : '—'})` : '—'),
    },
    { label: 'Feel · broke before the bonus', get: (c) => (c.feel?.metrics.broke != null ? `${pct(c.feel.metrics.broke)} (lived ${c.feel.observed.players ? pct(c.feel.observed.broke / c.feel.observed.players) : '—'})` : '—') },
    { label: 'Feel · wins ≥ big line', get: (c) => (c.feel?.kind === 'base' ? `${oneIn(c.feel.metrics.bigOneIn)} (≥ ${fx(c.feel.bigX)})` : '—') },
    { label: 'Feel · sessions without a big win', get: (c) => (c.feel?.kind === 'base' ? `${pct(c.feel.metrics.neverBig)} (${c.feel.params.reachSpins.toLocaleString('en-US')} spins)` : '—') },
    { label: 'Feel · median ÷ average bonus', get: (c) => (c.feel?.metrics.bonusRatio != null ? c.feel.metrics.bonusRatio.toFixed(2) : '—') },
    { label: 'Feel · purchases under the crumbs line', get: (c) => (c.feel?.metrics.crumbs != null ? pct(c.feel.metrics.crumbs) : '—') },
    { label: 'Feel · median purchase', get: (c) => (c.feel?.metrics.buyMedian != null ? `${pct(c.feel.metrics.buyMedian)} of the price` : '—') },
    { label: 'Feel · purchases refunded', get: (c) => (c.feel?.metrics.refund != null ? pct(c.feel.metrics.refund) : '—') },
    { label: 'Feel · max win', get: (c) => (c.feel ? oneIn(c.feel.metrics.maxOneIn) : '—') },
  ];

  const n0 = (v: number) => Math.round(v).toLocaleString('en-US');
</script>

{#if !game}
  <div class="panel"><p class="dim">Select a loaded game.</p></div>
{:else}
  {#if result && !editing}
    <div class="setup-sum">
      <span class="sum-modes">{modes.join(' · ')}{mixOn && modes.length >= 2 ? ' + mixed' : ''}</span>
      <span class="faint">{n0(players)} players × {n0(spins)} spins · {unlimited ? 'unlimited balance' : `balance ${n0(bankroll)}`} · seed {seed}</span>
      <button class="ghost" onclick={() => (editing = true)}>Edit</button>
      <button class="primary" onclick={run} disabled={busy}>{busy ? 'Simulating…' : 'Run again'}</button>
      <button class="ghost" onclick={() => { seed = seed + 1; run(); }} disabled={busy} title="Same settings, another sample">New sample</button>
    </div>
  {:else}
  <form class="setup panel" onsubmit={onSubmit} novalidate>
    <p class="intro">
      Pick one mode or several — each becomes a cohort of players, simulated on the published
      lookup table (bet = 1 base bet). Select two or more modes to also simulate a
      <strong>mixed session</strong> that plays them by share (e.g. 97 % base spins, 3 % bonus buys).
      Bonus figures need the imported books.
    </p>
    <div class="row">
      <span class="lbl" id="cs-modes-lbl">Modes</span>
      <div class="chips" role="group" aria-labelledby="cs-modes-lbl">
        {#each game.modes as m (m.name)}
          <button type="button" class="chip" class:on={modes.includes(m.name)} aria-pressed={modes.includes(m.name)} onclick={() => toggleMode(m.name)}>
            {m.name} <i>{m.cost}x</i>
          </button>
        {/each}
      </div>
    </div>
    <div class="row fields">
      <div class="field">
        <label class="lbl" for="cs-players">Players</label>
        <div class="ctl">
          <input id="cs-players" type="number" bind:value={players} min="1" max="200000" step="1000" />
          <span class="quick">
            {#each [1000, 10000, 50000] as v (v)}
              <button type="button" class="ghost preset" class:on={players === v} aria-label="Set players to {n0(v)}" onclick={() => (players = v)}>{v / 1000}k</button>
            {/each}
          </span>
        </div>
      </div>
      <div class="field">
        <label class="lbl" for="cs-spins">Spins per player</label>
        <div class="ctl">
          <input id="cs-spins" type="number" bind:value={spins} min="1" max="100000" step="100" />
          <span class="quick">
            {#each [100, 500, 1000, 5000] as v (v)}
              <button type="button" class="ghost preset" class:on={spins === v} aria-label="Set spins per player to {n0(v)}" onclick={() => (spins = v)}>{v >= 1000 ? `${v / 1000}k` : v}</button>
            {/each}
          </span>
        </div>
      </div>
      <div class="field" role="group" aria-labelledby="cs-bal-lbl">
        <span class="lbl" id="cs-bal-lbl">Balance</span>
        <div class="ctl">
          <label class="inline"><input type="checkbox" bind:checked={unlimited} /> unlimited</label>
          {#if !unlimited}
            <input type="number" bind:value={bankroll} min="1" step="50" aria-label="Starting balance, in bets" />
            <span class="dim">bets</span>
          {/if}
        </div>
      </div>
      <div class="field">
        <label class="lbl" for="cs-seed">Seed</label>
        <div class="ctl">
          <input id="cs-seed" type="number" bind:value={seed} class="seed" />
        </div>
      </div>
    </div>
    {#if modes.length >= 2}
      <div class="row">
        <label class="inline">
          <input type="checkbox" bind:checked={mixOn} />
          <span>Also simulate a <strong>mixed session</strong> — each round picks its mode by share:</span>
        </label>
        {#if mixOn}
          {#each modes as m (m)}
            <label class="inline share">{m} <input type="number" min="0" step="1" bind:value={shares[m]} aria-label="Share of {m} rounds, in %" /> %</label>
          {/each}
        {/if}
      </div>
    {/if}
    <details class="feel-set">
      <summary>
        <span class="lbl">Player feel</span>
        <span class="faint">
          drought = {n0(feel.droughtSpins)} spins with ≤ {feel.droughtBonuses} bonus · letdown = {feel.letdownBonuses}+ bonuses in {n0(feel.sessionSpins)} spins, all under {feel.weakUnder}x · broke = {n0(feel.deposit)}-stake deposit · big win ≥ {feel.bigAt}x in {n0(feel.reachSpins)} spins · crumbs &lt; {Math.round(feel.crumbsUnder * 100)} % of the price
        </span>
      </summary>
      <div class="row fields">
        <div class="field" role="group" aria-labelledby="cs-drought-lbl">
          <span class="lbl" id="cs-drought-lbl">Bonus drought</span>
          <div class="ctl">
            <input type="number" bind:value={feel.droughtSpins} min="10" max="100000" step="100" aria-label="Drought: session length, in spins" />
            <span class="dim">spins with at most</span>
            <input type="number" bind:value={feel.droughtBonuses} min="0" max="50" step="1" class="tiny" aria-label="Drought: most bonuses in the session" />
            <span class="dim">bonus</span>
          </div>
        </div>
        <div class="field" role="group" aria-labelledby="cs-letdown-lbl">
          <span class="lbl" id="cs-letdown-lbl">Bonus letdown</span>
          <div class="ctl">
            <input type="number" bind:value={feel.letdownBonuses} min="1" max="50" step="1" class="tiny" aria-label="Letdown: least bonuses in the session" />
            <span class="dim">or more bonuses in</span>
            <input type="number" bind:value={feel.sessionSpins} min="10" max="100000" step="50" aria-label="Letdown: session length, in spins" />
            <span class="dim">spins, all under</span>
            <input type="number" bind:value={feel.weakUnder} min="1" step="1" class="tiny" aria-label="Letdown: a weak bonus pays under this multiple of the stake" />
            <span class="dim">x</span>
          </div>
        </div>
        <div class="field">
          <label class="lbl" for="cs-big">Big win from</label>
          <div class="ctl">
            <input id="cs-big" type="number" bind:value={feel.bigAt} min="10" step="50" />
            <span class="dim">x the stake, looked for over</span>
            <input type="number" bind:value={feel.reachSpins} min="100" step="1000" aria-label="Big wins out of reach: session length, in spins" />
            <span class="dim">spins</span>
          </div>
        </div>
        <div class="field">
          <label class="lbl" for="cs-deposit">Broke before the bonus</label>
          <div class="ctl">
            <span class="dim">a deposit of</span>
            <input id="cs-deposit" type="number" bind:value={feel.deposit} min="2" step="10" />
            <span class="dim">stakes</span>
          </div>
        </div>
        <div class="field" role="group" aria-labelledby="cs-buy-lbl">
          <span class="lbl" id="cs-buy-lbl">Purchases</span>
          <div class="ctl">
            <span class="dim">crumbs under</span>
            <input type="number" value={Math.round(feel.crumbsUnder * 100)} oninput={(e) => (feel.crumbsUnder = Number(e.currentTarget.value) / 100)} min="1" max="95" step="5" class="tiny" aria-label="A crumb returns under this % of the price" />
            <span class="dim">% of the price · pop at</span>
            <input type="number" bind:value={feel.popAt} min="1.5" step="1" class="tiny" aria-label="A purchase pops at this multiple of its price" />
            <span class="dim">× · streak</span>
            <input type="number" bind:value={feel.buyStreak} min="1" max="100" step="1" class="tiny" aria-label="Purchases in a row never refunded" />
          </div>
        </div>
        <button type="button" class="ghost preset reset" onclick={() => (feel = { ...DEFAULT_FEEL })}>Defaults</button>
      </div>
    </details>
    <div class="row run">
      <button class="primary" type="submit" disabled={busy || modes.length === 0 || tooBig}>
        {busy ? 'Simulating…' : `Simulate ${cohortCount} cohort${cohortCount > 1 ? 's' : ''}`}
      </button>
      <span class="faint" class:too-big={tooBig}>{n0(totalRounds)} rounds · bet = 1 base bet{tooBig ? ' — too many, keep it under 2 billion' : ''}{modes.length === 0 ? ' · select at least one mode' : ''}</span>
      {#if result}<button type="button" class="ghost" onclick={() => (editing = false)}>Cancel</button>{/if}
    </div>
  </form>
  {/if}

  {#if error}<div class="banner error" role="alert">{error}</div>{/if}

  {#if result && cohorts.length}
    <div class="view-tabs" role="tablist" aria-label="Cohorts">
      <button role="tab" aria-selected={tab === 'overview'} class:active={tab === 'overview'} onclick={() => (tab = 'overview')}>Overview</button>
      {#each readings as { c, lines } (c.name)}
        {@const nSerious = count(lines, 'serious')}
        {@const nMinor = count(lines, 'minor')}
        <button role="tab" aria-selected={tab === c.name} class:active={tab === c.name} class:fix-tab={!!c.fix} onclick={() => openCohort(c.name)}>
          {cohortLabel(c.name, c.fix)}
          {#if nSerious}
            <span class="flag serious" title="{nSerious} serious finding{nSerious > 1 ? 's' : ''}">{nSerious}<span class="sr-only"> serious</span></span>
          {:else if nMinor}
            <span class="flag minor" title="{nMinor} minor finding{nMinor > 1 ? 's' : ''}">{nMinor}<span class="sr-only"> minor</span></span>
          {/if}
        </button>
      {/each}
      {#if cohorts.length > 1}<button role="tab" aria-selected={tab === 'compare'} class:active={tab === 'compare'} onclick={() => (tab = 'compare')}>Compare</button>{/if}
    </div>

    {#if tab === 'overview'}
      <div class="ov-cards">
        {#each readings as { c, lines } (c.name)}
          {@const issues = lines.filter((l) => l.severity !== 'lived')}
          <section class="ov-card" class:serious={count(lines, 'serious') > 0} class:minor={!count(lines, 'serious') && count(lines, 'minor') > 0}>
            <header>
              <strong>{cohortLabel(c.name, c.fix)}</strong>
              <span class="faint">{c.mix.length > 1 ? c.mix.map(([m, sh]) => `${m} ${pct(sh)}`).join(' · ') : `${c.modes[0]?.cost ?? 1}x a spin`}</span>
              <span class="tally">
                {#if count(lines, 'serious')}<span class="t serious">{count(lines, 'serious')} serious</span>{/if}
                {#if count(lines, 'minor')}<span class="t minor">{count(lines, 'minor')} minor</span>{/if}
                {#if !issues.length}<span class="t ok">nothing to flag</span>{/if}
              </span>
            </header>
            <div class="ov-kpis">
              {#each kpis(c).slice(1) as k (k.label)}
                <div><span>{k.label}</span><b>{k.value}</b></div>
              {/each}
            </div>
            <Findings lines={issues.length ? issues : lines.slice(0, 2)} mode={cohortMode(c)} />
            <div class="ov-actions">
              {#if c.feel?.frustrations.some((f) => f.fix)}
                <button class="ov-open primary" onclick={() => openCohort(c.name, 'feel')}>See the fixes and what they cost →</button>
              {/if}
              <button class="ov-open" onclick={() => openCohort(c.name)}>Open {cohortLabel(c.name, c.fix)} →</button>
            </div>
          </section>
        {/each}
      </div>
      <MutedList groups={mutedSim} count={mutedAll.length} />
      {#each comparisons as sentence (sentence)}<p class="cmp">{sentence}</p>{/each}
    {:else if tab === 'compare'}
      <div class="wrap card-like">
        <table class="compare">
          <thead>
            <tr><th><span class="sr-only">Question</span></th>{#each cohorts as c (c.name)}<th class="num"><button class="link" onclick={() => openCohort(c.name)}>{cohortLabel(c.name, c.fix)}</button></th>{/each}</tr>
          </thead>
          <tbody>
            {#each compareRows as r (r.label)}
              <tr><td class="dim">{r.label}</td>{#each cohorts as c (c.name)}<td class="num">{r.get(c)}</td>{/each}</tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#each comparisons as sentence (sentence)}<p class="cmp">{sentence}</p>{/each}
    {:else}
      {@const c = cohorts.find((x) => x.name === tab)}
      {#if c}
        {#key c.name}<CohortView {c} initial={cohortSection} onsimulate={(fr, p) => simulateFix(c, fr, p)} {simulating} />{/key}
      {/if}
    {/if}
  {/if}
{/if}

<style>
  .setup {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    margin-bottom: 18px;
  }

  .intro {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--text-dim);
    max-width: 760px;
    line-height: 1.55;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 18px;
    align-items: flex-end;
  }

  .lbl {
    font-size: var(--fs-2xs);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-faint);
  }

  .row > .lbl {
    align-self: center;
  }

  /* Every field: micro-label on top, one control line below — so all the
     inputs sit on the same baseline, presets or not. */
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }

  .ctl {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 30px;
  }

  .row input[type='number'] {
    width: 110px;
  }

  .row input.seed {
    width: 80px;
  }

  .quick {
    display: flex;
    gap: 3px;
  }

  button.preset {
    min-height: 26px;
    padding: 2px 8px;
    font-size: var(--fs-xs);
    font-family: var(--font-mono);
    border-color: var(--border);
  }

  button.preset.on {
    color: var(--gold);
    border-color: color-mix(in oklch, var(--gold) 45%, transparent);
  }

  .inline {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 6px;
    font-size: var(--fs-sm);
  }

  .inline input[type='number'],
  .ctl input[type='number'][aria-label] {
    width: 80px;
  }

  .share input {
    width: 64px !important;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
  }

  .chip {
    padding: 3px 10px;
    font-size: var(--fs-sm);
    border-radius: var(--r-pill);
    background: var(--bg-panel);
  }

  .chip i {
    font-style: normal;
    font-family: var(--font-mono);
    font-size: var(--fs-2xs);
    color: var(--text-faint);
    margin-left: 3px;
  }

  .chip.on {
    border-color: var(--gold);
    color: var(--gold);
    background: color-mix(in oklch, var(--gold) var(--tint-weak), transparent);
  }

  .run {
    align-items: center;
  }

  .too-big {
    color: var(--fail);
  }

  .setup-sum {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    margin-bottom: 14px;
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    background: var(--bg-panel);
    font-size: var(--fs-sm);
  }

  .sum-modes {
    font-family: var(--font-mono);
    font-weight: 650;
    color: var(--gold);
  }

  .setup-sum .faint {
    margin-right: auto;
  }

  /* Findings count on a cohort tab: a number, not just a colored dot.
     Serious = filled, minor = outlined. */
  .flag {
    display: inline-block;
    margin-left: 6px;
    min-width: 16px;
    padding: 0 5px;
    border-radius: var(--r-pill);
    font-family: var(--font-mono);
    font-size: var(--fs-2xs);
    line-height: 16px;
    text-align: center;
    vertical-align: 1px;
    border: 1px solid currentColor;
  }

  .flag.serious {
    color: var(--sv-serious);
    background: color-mix(in oklch, var(--sv-serious) var(--tint-strong), transparent);
  }

  .flag.minor {
    color: var(--sv-minor);
    background: transparent;
  }

  .ov-cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(480px, 1fr));
    gap: var(--sp-3);
    margin-top: var(--sp-1);
  }

  @media (max-width: 1000px) {
    .ov-cards {
      grid-template-columns: 1fr;
    }
  }

  .ov-card {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-top: 3px solid var(--sv-context);
    border-radius: var(--r-m);
    background: var(--bg-panel);
    overflow: hidden;
  }

  .ov-card.serious { border-top-color: var(--sv-serious); }
  .ov-card.minor { border-top-color: var(--sv-minor); }

  .ov-card header {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    padding: 11px 14px 4px;
  }

  .ov-card header strong {
    font-size: var(--fs-lg);
  }

  .ov-card header .faint {
    font-size: var(--fs-xs);
  }

  .tally {
    margin-left: auto;
    display: flex;
    gap: 10px;
    font-size: var(--fs-xs);
  }

  .t.serious { color: var(--sv-serious); }
  .t.minor { color: var(--sv-minor); }
  .t.ok { color: var(--pass); }

  .ov-kpis {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));
    gap: var(--sp-2);
    padding: 8px 14px 12px;
    border-bottom: 1px solid var(--border);
  }

  .ov-kpis div {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .ov-kpis span {
    font-size: var(--fs-2xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-faint);
  }

  .ov-kpis b {
    font-family: var(--font-mono);
    font-size: var(--fs-lg);
    font-weight: 650;
  }

  .ov-actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
    margin: 6px 12px 10px;
  }

  .ov-open {
    padding: 3px 10px;
    font-size: var(--fs-sm);
  }

  .feel-set {
    border-top: 1px solid var(--border);
    padding-top: var(--sp-2);
  }

  .feel-set summary {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 10px;
    cursor: pointer;
    font-size: var(--fs-xs);
  }

  .feel-set[open] summary {
    margin-bottom: var(--sp-2);
  }

  .feel-set .ctl input[type='number'] {
    width: 76px;
  }

  .feel-set .ctl input.tiny[type='number'] {
    width: 52px;
  }

  button.reset {
    align-self: flex-end;
  }

  .fix-tab {
    font-style: italic;
  }

  .card-like {
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    background: var(--bg-panel);
    padding: 4px 8px;
  }

  .wrap {
    overflow-x: auto;
  }

  .compare th {
    white-space: nowrap;
  }

  button.link {
    background: none;
    border: none;
    padding: 0;
    color: var(--text-dim);
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    cursor: pointer;
  }

  button.link:hover {
    color: var(--gold);
  }

  .cmp {
    font-size: var(--fs-sm);
    color: var(--text-dim);
    margin: var(--sp-2) 0 0;
  }
</style>
