<script lang="ts">
  import { api, fmtPct, type KeyStat, type MechFacet, type ModeMechanics } from '../../lib/api';
  import { errorText } from '../../lib/stores.svelte';

  /**
   * Mechanics — two levels:
   *   Key stats   the few numbers that matter, declared by the game next to its
   *               math (`slotlab.mechanics.json`: plain names + planned split),
   *               read in the books and weighted by the LUT: planned vs seen.
   *   All fields  (advanced, folded) the raw survey below.
   *
   * The raw survey — what the events of the books show the player, for any slot:
   * every field of every event (fish type, winning symbol, level, multiplier…)
   * as a split weighted by the LUT, next to the split of the raw books. The
   * config draws with its own weights; the LUT then picks among the books, so
   * only the weighted split is what players see — a large gap means the LUT
   * bends the mechanic (a bonus that is too rich publishes its poorest books).
   */
  interface Props {
    game: string;
    selectedMode: string;
  }

  const { game, selectedMode }: Props = $props();

  type Scope = 'all' | 'bonus' | 'nobonus';
  const SCOPES: { id: Scope; label: string }[] = [
    { id: 'all', label: 'All rounds' },
    { id: 'bonus', label: 'Bonus rounds' },
    { id: 'nobonus', label: 'No bonus' },
  ];
  /** A LUT that shifts a share by this much is worth a look. */
  const DRIFT_WARN = 5;

  let scope = $state<Scope>('all');
  let filter = $state('');
  let data = $state<ModeMechanics | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let seq = 0;

  $effect(() => {
    if (game && selectedMode) void load(game, selectedMode, scope);
  });

  async function load(g: string, mode: string, sc: Scope) {
    const mine = ++seq;
    loading = true;
    error = null;
    try {
      const r = await api.mechanics(g, mode, sc);
      if (mine === seq) data = r;
    } catch (e) {
      if (mine === seq) {
        data = null;
        error = errorText(e);
      }
    } finally {
      if (mine === seq) loading = false;
    }
  }

  /** Facets grouped by event type, the most frequent events first. */
  const groups = $derived.by(() => {
    if (!data) return [];
    const q = filter.trim().toLowerCase();
    const events = data.facets.find((f) => f.event === '#events');
    const freq = new Map(events?.values.map((v) => [v.value, v.perRound]) ?? []);
    const byEvent = new Map<string, MechFacet[]>();
    for (const f of data.facets) {
      if (f.event === '#events') continue;
      if (q && !`${f.event}.${f.path}`.toLowerCase().includes(q)) continue;
      const list = byEvent.get(f.event) ?? [];
      list.push(f);
      byEvent.set(f.event, list);
    }
    return [...byEvent.entries()]
      .map(([event, facets]) => ({
        event,
        perRound: freq.get(event) ?? 0,
        facets: facets.sort((a, b) => a.path.localeCompare(b.path)),
      }))
      .sort((a, b) => b.perRound - a.perRound || a.event.localeCompare(b.event));
  });

  const eventsFacet = $derived(data?.facets.find((f) => f.event === '#events'));
  const drifting = $derived(
    (data?.facets ?? []).filter((f) => f.event !== '#events' && f.driftPoints >= DRIFT_WARN).length,
  );

  function share(v: number, total: number): number {
    return total > 0 ? v / total : 0;
  }

  /** Quantile of the per-round count, on the LUT. */
  function countQuantile(dist: [number, number][], q: number): number {
    let acc = 0;
    for (const [n, p] of dist) {
      acc += p;
      if (acc >= q - 1e-12) return n;
    }
    return dist.length ? dist[dist.length - 1][0] : 0;
  }

  function countMax(dist: [number, number][]): number {
    const seen = dist.filter(([, p]) => p > 0);
    return seen.length ? seen[seen.length - 1][0] : 0;
  }

  function scopeWord(k: KeyStat): string {
    return k.scope === 'bonus' ? 'bonus' : k.scope === 'nobonus' ? 'spin without bonus' : 'spin';
  }

  function statusText(k: KeyStat): string {
    if (k.status === 'info') return '';
    const gap = (k.worstGap ?? 0).toFixed(1);
    return k.status === 'ok' ? `As planned (≤ ${gap} pts)` : `Off plan by ${gap} pts`;
  }

  function fmtNum(v: number): string {
    if (!isFinite(v)) return '—';
    if (v >= 100) return Math.round(v).toLocaleString('en-US');
    return v.toFixed(v >= 10 ? 1 : 2);
  }

  const HUES = [88, 220, 25, 155, 300, 60, 190, 340, 120, 260];
  function color(i: number): string {
    return `oklch(0.72 0.13 ${HUES[i % HUES.length]})`;
  }
</script>

<section class="page">
  <p class="overline">Key stats — {selectedMode}</p>
  <p class="note">
    What players actually see, measured on the books weighted by the LUT, next to what the math
    planned. A gap means the LUT bends the mechanic — it picks among the simulated rounds, and
    keeps the cheap ones when a feature is too rich.
  </p>

  {#if data?.keyStatsError}
    <div class="banner error">Could not read the key stats: {data.keyStatsError}</div>
  {:else if data && !data.keyStatsFile}
    <div class="banner info">
      No key stats for this game yet. They live in <code>slotlab.mechanics.json</code>, next to the
      game's math (<code>math/games/&lt;game&gt;/</code>) — ask Claude to write them for this slot.
    </div>
  {:else if data && data.keyStats.length === 0}
    <p class="dim">None of the game's key stats exist in this mode.</p>
  {/if}

  {#each data?.keyStats ?? [] as k (k.title)}
    <div class="key panel" class:off={k.status === 'off'}>
      <div class="key-head">
        <h3>{k.title}</h3>
        {#if k.status !== 'info'}
          <span class="pill tiny" class:green={k.status === 'ok'} class:yellow={k.status === 'off'}>
            {statusText(k)}
          </span>
        {/if}
      </div>
      {#if k.perRoundLabel !== ''}
      <p class="key-sum">
        <strong>{fmtNum(k.perRound)}</strong>
        {k.perRoundLabel ?? `per ${scopeWord(k)}`}
        {#if k.count}
          <span class="dim">
            · half of the {scopeWord(k)}s get {k.count[0]} or fewer · 9 in 10 get {k.count[1]} or fewer ·
            max {k.count[2]}</span
          >
        {/if}
        {#if k.roundsWith < 0.999}
          <span class="dim">· happens in {fmtPct(k.roundsWith, 1)} of {scopeWord(k)}s</span>
        {/if}
      </p>
      {/if}
      <table class="key-rows">
        <thead>
          <tr>
            <th></th>
            <th class="bar-col" aria-label="Share seen in game"></th>
            <th class="num">Seen in game</th>
            {#if k.rows.some((r) => r.planned !== null)}
              <th class="num">Planned</th>
              <th class="num">Gap</th>
            {/if}
          </tr>
        </thead>
        <tbody>
          {#each k.rows as r (r.value)}
            {@const gap = r.planned === null ? null : (r.seen - r.planned) * 100}
            <tr>
              <td class="label">{r.label}</td>
              <td class="bar-col">
                <div class="meter">
                  <span class="fill" style="width: {Math.min(100, r.seen * 100)}%"></span>
                  {#if r.planned !== null}
                    <span class="mark" style="left: {Math.min(100, r.planned * 100)}%" title="Planned"></span>
                  {/if}
                </div>
              </td>
              <td class="num"><strong>{fmtPct(r.seen, 1)}</strong></td>
              {#if k.rows.some((x) => x.planned !== null)}
                <td class="num faint">{r.planned === null ? '—' : fmtPct(r.planned, 1)}</td>
                <td class="num" class:warn-num={gap !== null && Math.abs(gap) > k.tolerance}>
                  {gap === null ? '' : Math.abs(gap) < 0.05 ? '0' : (gap > 0 ? '+' : '') + gap.toFixed(1) + ' pts'}
                </td>
              {/if}
            </tr>
          {/each}
        </tbody>
      </table>
      {#if k.note}<p class="key-note faint">{k.note}</p>{/if}
    </div>
  {/each}

  <details class="advanced">
    <summary>All event fields (advanced)</summary>
  <p class="note">
    Every field of every event, as the books name it. <em>In game</em> = weighted by the LUT;
    <em>Sim</em> = the raw books before weighting; <em>Δ pts</em> = the difference. Mostly
    technical fields (anticipation, win levels…) — useful to find a field to promote to a key stat.
  </p>

  <div class="toolbar">
    <div class="seg" role="radiogroup" aria-label="Rounds">
      {#each SCOPES as s (s.id)}
        <button
          role="radio"
          aria-checked={scope === s.id}
          class:active={scope === s.id}
          onclick={() => (scope = s.id)}>{s.label}</button
        >
      {/each}
    </div>
    <input
      class="filter"
      bind:value={filter}
      placeholder="Filter fields (e.g. fish, symbol, multiplier)"
      aria-label="Filter fields"
      spellcheck="false"
    />
  </div>

  {#if error}
    <div class="banner error">{error}</div>
  {:else if !data}
    <p class="dim">{loading ? 'Reading the books…' : 'No data.'}</p>
  {:else}
    <div class="statline" aria-busy={loading}>
      <div><span class="dim">Rounds in scope</span><strong>{fmtPct(data.probability, 2)}</strong></div>
      <div><span class="dim">Books</span><strong>{data.books.toLocaleString('en-US')}</strong></div>
      <div>
        <span class="dim">Events / round</span><strong>{fmtNum(eventsFacet?.perRound ?? 0)}</strong>
      </div>
      <div>
        <span class="dim">Bent by the LUT (≥ {DRIFT_WARN} pts)</span>
        <strong class:warn-num={drifting > 0}>{drifting}</strong>
      </div>
    </div>

    {#if data.books === 0}
      <p class="dim">No round in this scope.</p>
    {/if}

    {#each groups as g (g.event)}
      <div class="event">
        <h3>
          <code>{g.event}</code>
          <span class="faint">{fmtNum(g.perRound)} per round</span>
        </h3>
        {#each g.facets as f (f.path)}
          {@const total = f.perRound}
          {@const totalRaw = f.perRoundRaw}
          <div class="facet panel">
            <div class="facet-head">
              <code class="path">{f.path}</code>
              <span class="dim">
                {fmtNum(f.perRound)} / round · in {fmtPct(f.roundsWith, 1)} of rounds · per round: median
                {countQuantile(f.countDist, 0.5)}, p90 {countQuantile(f.countDist, 0.9)}, max {countMax(f.countDist)}
              </span>
              {#if f.driftPoints >= DRIFT_WARN}
                <span class="pill yellow tiny" title="Largest gap between the in-game share and the raw books">
                  LUT shifts {f.driftPoints.toFixed(1)} pts
                </span>
              {/if}
            </div>
            <div class="bar" aria-hidden="true">
              {#each f.values as v, i (v.value)}
                {@const s = share(v.perRound, total)}
                {#if s > 0}
                  <span style="flex: {s}; background: {color(i)}" title="{v.value}: {fmtPct(s, 1)}"></span>
                {/if}
              {/each}
            </div>
            <table class="values">
              <thead>
                <tr>
                  <th>Value</th>
                  <th class="num">In game</th>
                  <th class="num">Sim</th>
                  <th class="num">Δ pts</th>
                  <th class="num">Per round</th>
                  <th class="num">Rounds with ≥ 1</th>
                </tr>
              </thead>
              <tbody>
                {#each f.values as v, i (v.value)}
                  {@const s = share(v.perRound, total)}
                  {@const r = share(v.perRoundRaw, totalRaw)}
                  {@const d = (s - r) * 100}
                  <tr>
                    <td><span class="swatch" style="background: {color(i)}"></span><code>{v.value}</code></td>
                    <td class="num"><strong>{fmtPct(s, 1)}</strong></td>
                    <td class="num faint">{fmtPct(r, 1)}</td>
                    <td class="num" class:warn-num={Math.abs(d) >= DRIFT_WARN}>
                      {Math.abs(d) < 0.05 ? '0' : (d > 0 ? '+' : '') + d.toFixed(1)}
                    </td>
                    <td class="num">{fmtNum(v.perRound)}</td>
                    <td class="num faint">{fmtPct(v.roundsWith, 1)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/each}
      </div>
    {:else}
      {#if data.books > 0}<p class="dim">No field matches “{filter}”.</p>{/if}
    {/each}
  {/if}
  </details>
</section>

<style>
  .note {
    color: var(--text-dim);
    font-size: var(--fs-sm);
    max-width: 80ch;
    margin: 0 0 12px;
  }
  .toolbar {
    display: flex;
    gap: 12px;
    align-items: center;
    flex-wrap: wrap;
    margin-bottom: 12px;
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    overflow: hidden;
  }
  .seg button {
    border: 0;
    border-radius: 0;
    background: transparent;
  }
  .seg button.active {
    background: var(--bg-raised);
    color: var(--gold);
  }
  .filter {
    flex: 1;
    min-width: 220px;
    max-width: 420px;
  }
  .event {
    margin-top: 20px;
  }
  .event h3 {
    font-size: var(--fs-md);
    margin: 0 0 8px;
    display: flex;
    gap: 10px;
    align-items: baseline;
  }
  .facet {
    margin-bottom: 10px;
  }
  .facet-head {
    display: flex;
    gap: 10px;
    align-items: baseline;
    flex-wrap: wrap;
    margin-bottom: 8px;
    font-size: var(--fs-sm);
  }
  .path {
    font-size: var(--fs-md);
    color: var(--text);
  }
  .bar {
    display: flex;
    height: 10px;
    border-radius: var(--r-pill);
    overflow: hidden;
    gap: 1px;
    background: var(--bg-deep);
    margin-bottom: 8px;
  }
  .values {
    font-size: var(--fs-sm);
  }
  .swatch {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    margin-right: 8px;
  }
  .warn-num {
    color: var(--warn);
  }
  .key {
    margin-bottom: 12px;
  }
  .key.off {
    border-color: oklch(0.76 0.15 65 / 0.45);
  }
  .key-head {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
  }
  .key-head h3 {
    font-size: var(--fs-lg);
    margin: 0;
  }
  .key-sum {
    margin: 6px 0 10px;
  }
  .key-sum strong {
    font-size: var(--fs-md);
    color: var(--gold);
  }
  .key-rows td.label {
    white-space: nowrap;
  }
  .bar-col {
    width: 40%;
  }
  .meter {
    position: relative;
    height: 10px;
    border-radius: var(--r-pill);
    background: var(--bg-deep);
  }
  .meter .fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: var(--r-pill);
    background: var(--gold);
  }
  .meter .mark {
    position: absolute;
    top: -3px;
    bottom: -3px;
    width: 2px;
    margin-left: -1px;
    background: var(--text);
  }
  .key-note {
    font-size: var(--fs-xs);
    margin: 8px 0 0;
  }
  .advanced {
    margin-top: 28px;
  }
  .advanced summary {
    cursor: pointer;
    color: var(--text-dim);
    font-size: var(--fs-md);
    margin-bottom: 10px;
  }
</style>
