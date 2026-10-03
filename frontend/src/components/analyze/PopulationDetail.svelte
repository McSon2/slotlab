<script lang="ts">
  import CdfChart from './CdfChart.svelte';
  import BandChart from './BandChart.svelte';
  import Icon from '../Icon.svelte';
  import {
    fx,
    pct,
    oneIn,
    bandLabel,
    headline,
    populationFindings,
    children,
    sameTierElsewhere,
    bandComposition,
    threshold,
    quantile,
    refId,
    seriesColor,
    type ModePopulations,
    type Population,
    type PopRef,
  } from '../../lib/populations';

  interface Props {
    mp: ModePopulations;
    pop: Population;
    all: ModePopulations[];
    pinned: Set<string>;
    /** Books that open the bonus more than once (from the trigger count). */
    multiTrigger?: { share: number; books: number } | null;
    customThresholds: number[];
    onSelect: (ref: PopRef) => void;
    onTogglePin: (ref: PopRef) => void;
    onPinMany: (refs: PopRef[]) => void;
    onPlay: (mode: string, simId: number) => void | Promise<void>;
    onBookmark: (mode: string, simId: number, payout: number) => void | Promise<void>;
    onAddThreshold: (x: number) => void;
    onRemoveThreshold: (x: number) => void;
  }

  const {
    mp,
    pop,
    all,
    pinned,
    multiTrigger = null,
    customThresholds,
    onSelect,
    onTogglePin,
    onPinMany,
    onPlay,
    onBookmark,
    onAddThreshold,
    onRemoveThreshold,
  }: Props = $props();

  const cost = $derived(mp.cost > 0 ? mp.cost : 1);
  const ref = $derived({ mode: mp.mode, key: pop.key });
  const isPinned = $derived(pinned.has(refId(ref)));
  const findings = $derived(populationFindings(pop, cost));
  const kids = $derived(children(mp, pop));
  const composition = $derived(kids.length ? bandComposition(pop, kids) : []);
  const elsewhere = $derived(sameTierElsewhere(all, pop));
  const isBonusPop = $derived(pop.kind === 'bonus' || pop.kind === 'tier' || pop.kind === 'variant');

  /** Threshold rows worth reading: stop at the first one nothing reaches. */
  const thresholdRows = $derived.by(() => {
    const out = [];
    for (const t of pop.thresholds) {
      out.push(t);
      if (t.below >= 1) break;
    }
    return out;
  });

  /** Bands from the first to the last non-empty one. */
  const bandIdx = $derived.by(() => {
    const idx = pop.bands.map((b, i) => (b.share > 0 ? i : -1)).filter((i) => i >= 0);
    if (!idx.length) return [];
    return Array.from({ length: idx[idx.length - 1] - idx[0] + 1 }, (_, k) => idx[0] + k);
  });

  let thresholdDraft = $state('');
  function addThreshold() {
    const v = Number(thresholdDraft.replace(/x$/i, '').trim());
    if (Number.isFinite(v) && v > 0) {
      onAddThreshold(v);
      thresholdDraft = '';
    }
  }

  const qLabel = (q: number) => (q === 0.5 ? 'median' : q === 0.999 ? 'P99.9' : `P${Math.round(q * 100)}`);

  /** The play / bookmark action in flight (one at a time). */
  let busy = $state<string | null>(null);
  async function act(key: string, fn: () => void | Promise<void>) {
    if (busy) return;
    busy = key;
    try {
      await fn();
    } finally {
      busy = null;
    }
  }

  /** Term whose definition is shown (disclosure: hover title + keyboard/SR). */
  let openDef = $state<string | null>(null);
  const DEFS: Record<string, string> = {
    cv: 'Standard deviation ÷ average — 0 means always the same result.',
    ess: '(Σw)² / Σw² — how many books really carry the weight.',
  };

  const color = $derived(seriesColor(1));
</script>

{#snippet term(id: string, label: string)}
  <dt>
    <button
      type="button"
      class="term"
      title={DEFS[id]}
      aria-expanded={openDef === id}
      aria-controls="def-{id}"
      onclick={() => (openDef = openDef === id ? null : id)}
    >
      {label}<span class="q" aria-hidden="true">?</span>
    </button>
  </dt>
{/snippet}

{#snippet simActions(simId: number, payout: number, bookmark = true)}
  <span class="sim-actions">#{simId}
    <button
      title="Play this book"
      aria-label="Force book #{simId} for the next spin in Play"
      disabled={busy === `play:${simId}`}
      onclick={() => act(`play:${simId}`, () => onPlay(mp.mode, simId))}
    >
      <Icon name="play" size={10} />
    </button>
    {#if bookmark}
      <button
        title="Bookmark"
        aria-label="Bookmark book #{simId}"
        disabled={busy === `mark:${simId}`}
        onclick={() => act(`mark:${simId}`, () => onBookmark(mp.mode, simId, Math.round(payout * 100)))}
      >
        <Icon name="star" size={10} />
      </button>
    {/if}
  </span>
{/snippet}

<div class="detail">
  <header class="d-head">
    <div>
      <p class="d-crumb faint">{mp.mode}{pop.parent ? ` · ${pop.kind}` : ''}</p>
      <h2>{pop.label}</h2>
    </div>
    <div class="d-actions">
      <button class:ghost={!isPinned} aria-pressed={isPinned} onclick={() => onTogglePin(ref)}>
        {isPinned ? '✓ In Compare' : '+ Compare'}
      </button>
      {#if elsewhere.length > 1}
        <button class="ghost" onclick={() => onPinMany(elsewhere.map((e) => ({ mode: e.mode, key: e.pop.key })))}>
          Compare {pop.label} in {elsewhere.length} modes
        </button>
      {/if}
      {#if kids.length > 1}
        <button class="ghost" onclick={() => onPinMany(kids.map((k) => ({ mode: mp.mode, key: k.key })))}>
          Compare its {kids.length} parts
        </button>
      {/if}
    </div>
  </header>

  <p class="headline">{headline(pop)}.</p>
  {#if findings.length}
    <ul class="findings">
      {#each findings as f, i (i)}
        <li class:warn={f.level === 'warn'}>{#if f.level === 'warn'}<span class="sr-only">Warning: </span>{/if}{f.text}</li>
      {/each}
    </ul>
  {/if}

  <!-- Key numbers, in the order the questions come -->
  <div class="cards">
    <section class="card">
      <p class="overline">How often</p>
      <dl>
        <dt>Frequency</dt><dd>{pop.kind === 'all' ? 'every spin' : oneIn(pop.oneIn)}</dd>
        <dt>Per spin</dt><dd>{pct(pop.probability)}</dd>
        {#if pop.shareOfBonuses !== null && pop.kind !== 'bonus'}
          <dt>Share of bonuses</dt><dd>{pct(pop.shareOfBonuses)}</dd>
        {/if}
        <dt>RTP contribution</dt><dd>{pct(pop.rtpContribution)}</dd>
        <dt>Share of the mode's RTP</dt><dd>{pct(pop.rtpShare)}</dd>
      </dl>
    </section>
    <section class="card">
      <p class="overline">Typical vs average</p>
      <dl>
        <dt>Median</dt><dd class="big">{fx(pop.median)}</dd>
        <dt>Average win</dt><dd class="big gold">{fx(pop.mean)}</dd>
        <dt>Average ÷ median</dt><dd>{pop.meanOverMedian === null ? '—' : pop.meanOverMedian.toFixed(2) + '×'}</dd>
        {#if cost !== 1}
          <dt>Average ÷ cost</dt><dd>{(pop.mean / cost).toFixed(3)}×</dd>
          <dt>Median ÷ cost</dt><dd>{(pop.median / cost).toFixed(3)}×</dd>
        {/if}
      </dl>
    </section>
    <section class="card">
      <p class="overline">Spread</p>
      <dl>
        <dt>Min · max</dt><dd>{fx(pop.min)} · {fx(pop.max)}</dd>
        <dt>P10 – P90</dt><dd>{fx(quantile(pop, 0.1)?.value ?? 0)} – {fx(quantile(pop, 0.9)?.value ?? 0)}</dd>
        <dt>P99</dt><dd>{fx(quantile(pop, 0.99)?.value ?? 0)}</dd>
        <dt>Std dev</dt><dd>{fx(pop.stdDev)}{cost !== 1 ? ` (${(pop.stdDev / cost).toFixed(2)} ÷ cost)` : ''}</dd>
        {@render term('cv', 'Coeff. of variation')}<dd>{pop.cv.toFixed(2)}</dd>
        {#if openDef === 'cv'}<dd class="def" id="def-cv">{DEFS.cv}</dd>{/if}
      </dl>
    </section>
    <section class="card">
      <p class="overline">Outcomes</p>
      <dl>
        <dt>Pay nothing (0x)</dt>
        <dd class:warn-num={isBonusPop && pop.zeroRate > 0}>
          {pct(pop.zeroRate)}{#if isBonusPop && pop.zeroRate > 0}<span class="sr-only"> (warning)</span>{/if}
        </dd>
        <dt>Return ≥ cost ({fx(cost)})</dt><dd>{pct(pop.profitableRate)}</dd>
        <dt>Top 1 % carries</dt><dd>{pct(pop.top1Share)} of value</dd>
        <dt>Top 10 % carries</dt><dd>{pct(pop.top10Share)} of value</dd>
        <dt>Average without top 1 %</dt><dd>{fx(pop.meanWithoutTop1)}</dd>
        <dt>Max win share</dt><dd>{pct(pop.maxShare)}</dd>
      </dl>
    </section>
    <section class="card">
      <p class="overline">Sample</p>
      <dl>
        <dt>Books</dt><dd>{pop.books.toLocaleString('en-US')}</dd>
        <dt>Drawable (weight &gt; 0)</dt><dd>{pop.drawableBooks.toLocaleString('en-US')}</dd>
        {@render term('ess', 'Effective outcomes')}
        <dd class:warn-num={pop.effectiveOutcomes < 50}>
          {Math.round(pop.effectiveOutcomes).toLocaleString('en-US')}{#if pop.effectiveOutcomes < 50}<span class="sr-only"> (warning: few)</span>{/if}
        </dd>
        {#if openDef === 'ess'}<dd class="def" id="def-ess">{DEFS.ess}</dd>{/if}
        {#if multiTrigger && pop.kind === 'bonus'}
          <dt>Open 2+ bonuses</dt><dd>{pct(multiTrigger.share)}</dd>
        {/if}
      </dl>
    </section>
  </div>

  <!-- The distribution itself -->
  <div class="charts">
    <section>
      <p class="overline">Share of results below x</p>
      <CdfChart
        series={[{ label: pop.label, color, points: pop.cdf, median: pop.median, mean: pop.mean, max: pop.max }]}
        {cost}
      />
    </section>
    <section>
      <p class="overline">Where results fall vs where the value comes from</p>
      <BandChart series={[{ label: pop.label, color, bands: pop.bands }]} />
    </section>
  </div>

  <div class="tables">
    <section>
      <p class="overline">Thresholds</p>
      <table>
        <thead>
          <tr>
            <th class="num">x</th>
            <th class="num">Below</th>
            <th class="num">At least</th>
            <th class="num">Value from ≥ x</th>
          </tr>
        </thead>
        <tbody>
          {#each thresholdRows as t (t.x)}
            {@const custom = customThresholds.some((c) => Math.abs(c - t.x) < 1e-9)}
            <tr class:cost-row={Math.abs(t.x - cost) < 1e-9 && cost !== 1}>
              <td class="num">
                {fx(t.x)}
                {#if custom}
                  <button class="x-rm" title="Remove" aria-label="Remove the {fx(t.x)} threshold" onclick={() => onRemoveThreshold(t.x)}>
                    <Icon name="close" size={11} />
                  </button>
                {/if}
                {#if Math.abs(t.x - cost) < 1e-9 && cost !== 1}<span class="faint"> cost</span>{/if}
              </td>
              <td class="num">{pct(t.below)}<span class="bar"><span style="width: {t.below * 100}%"></span></span></td>
              <td class="num">{pct(t.atLeast)}</td>
              <td class="num faint">{pct(t.valueAtLeast)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      <div class="thr-add">
        <input
          bind:value={thresholdDraft}
          aria-label="Add a threshold (x base bet)"
          placeholder="another x, e.g. 9"
          onkeydown={(e) => e.key === 'Enter' && addThreshold()}
        />
        <button onclick={addThreshold}>Measure</button>
      </div>
    </section>

    <section>
      <p class="overline">Percentiles <span class="faint">— replay a result at that exact win</span></p>
      <table>
        <thead>
          <tr><th>Rank</th><th class="num">Win</th><th>Book</th></tr>
        </thead>
        <tbody>
          <tr>
            <td>min</td><td class="num">{fx(pop.min)}</td><td></td>
          </tr>
          {#each pop.quantiles as q (q.q)}
            <tr class:median-row={q.q === 0.5}>
              <td>{qLabel(q.q)}</td>
              <td class="num">{fx(q.value)}</td>
              <td>
                {@render simActions(q.simId, q.value)}
              </td>
            </tr>
          {/each}
          <tr>
            <td>max</td><td class="num">{fx(pop.max)}</td>
            <td>
              {#if pop.topPayouts[0]}
                {@const top = pop.topPayouts[0]}
                {@render simActions(top.simIds[0], top.payout, false)}
              {/if}
            </td>
          </tr>
        </tbody>
      </table>
    </section>
  </div>

  {#if kids.length > 0}
    <section class="block">
      <p class="overline">Breakdown</p>
      <table>
        <thead>
          <tr>
            <th>Part</th>
            <th class="num">Frequency</th>
            <th class="num">Share here</th>
            <th class="num">Median</th>
            <th class="num">Average</th>
            <th class="num">Avg ÷ med</th>
            <th class="num">&lt; 10x</th>
            <th class="num">&lt; 100x</th>
            <th class="num">CV</th>
            <th class="num">Top 1 %</th>
            <th class="num">RTP share</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each kids as k (k.key)}
            {@const kPinned = pinned.has(refId({ mode: mp.mode, key: k.key }))}
            <tr>
              <td><button class="link" onclick={() => onSelect({ mode: mp.mode, key: k.key })}>{k.label}</button></td>
              <td class="num">{oneIn(k.oneIn)}</td>
              <td class="num">{pct(k.probability / pop.probability)}</td>
              <td class="num">{fx(k.median)}</td>
              <td class="num">{fx(k.mean)}</td>
              <td class="num">{k.meanOverMedian === null ? '—' : k.meanOverMedian.toFixed(1) + '×'}</td>
              <td class="num">{pct(threshold(k, 10)?.below ?? 0)}</td>
              <td class="num">{pct(threshold(k, 100)?.below ?? 0)}</td>
              <td class="num">{k.cv.toFixed(2)}</td>
              <td class="num">{pct(k.top1Share)}</td>
              <td class="num">{pct(k.rtpShare)}</td>
              <td>
                <button
                  class="tiny"
                  class:ghost={!kPinned}
                  aria-pressed={kPinned}
                  aria-label="{kPinned ? 'Remove' : 'Add'} {k.label} {kPinned ? 'from' : 'to'} Compare"
                  title={kPinned ? 'In Compare — click to remove' : 'Add to Compare'}
                  onclick={() => onTogglePin({ mode: mp.mode, key: k.key })}
                >
                  {kPinned ? '✓' : '+'}
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      <p class="note">
        Shares here are of this population; RTP share is of the whole {mp.mode} mode.
      </p>
    </section>
  {/if}

  {#if elsewhere.length > 1}
    <section class="block">
      <p class="overline">{pop.label} in every mode</p>
      <table>
        <thead>
          <tr>
            <th>Mode</th>
            <th class="num">Frequency</th>
            <th class="num">Median</th>
            <th class="num">Average</th>
            <th class="num">P90</th>
            <th class="num">&lt; 10x</th>
            <th class="num">CV</th>
            <th class="num">Retrigger</th>
          </tr>
        </thead>
        <tbody>
          {#each elsewhere as e (e.mode)}
            <tr class:row-active={e.mode === mp.mode}>
              <td><button class="link" onclick={() => onSelect({ mode: e.mode, key: e.pop.key })}>{e.mode}</button></td>
              <td class="num">{oneIn(e.pop.oneIn)}</td>
              <td class="num">{fx(e.pop.median)}</td>
              <td class="num">{fx(e.pop.mean)}</td>
              <td class="num">{fx(quantile(e.pop, 0.9)?.value ?? 0)}</td>
              <td class="num">{pct(threshold(e.pop, 10)?.below ?? 0)}</td>
              <td class="num">{e.pop.cv.toFixed(2)}</td>
              <td class="num">{e.pop.retriggers ? pct(e.pop.retriggers.rate) : '—'}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </section>
  {/if}

  {#if pop.retriggers}
    {@const r = pop.retriggers}
    <section class="block">
      <p class="overline">Retriggers</p>
      <div class="statline">
        <div><span class="dim">Retrigger at least once</span><strong>{pct(r.rate)}</strong></div>
        <div><span class="dim">Average retriggers</span><strong>{r.mean.toFixed(2)}</strong></div>
        <div><span class="dim">Max</span><strong>{r.max}</strong></div>
        <div><span class="dim">Avg win with / without</span><strong>{r.avgWith === null ? '—' : fx(r.avgWith)} / {r.avgWithout === null ? '—' : fx(r.avgWithout)}</strong></div>
        <div><span class="dim">Value from retriggered</span><strong>{pct(r.valueShareWith)}</strong></div>
      </div>
      {#if r.rows.length > 1}
        <table>
          <thead>
            <tr>
              <th>Retriggers</th>
              <th class="num">Share</th>
              <th class="num">Median</th>
              <th class="num">Average</th>
              <th class="num">Share of value</th>
              <th class="num">Books</th>
            </tr>
          </thead>
          <tbody>
            {#each r.rows as g (g.key)}
              <tr>
                <td>{g.key}</td>
                <td class="num">{pct(g.share)}<span class="bar"><span style="width: {g.share * 100}%"></span></span></td>
                <td class="num">{fx(g.median)}</td>
                <td class="num">{fx(g.avg)}</td>
                <td class="num">{pct(g.valueShare)}</td>
                <td class="num faint">{g.books.toLocaleString('en-US')}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </section>
  {/if}

  {#if pop.spins || pop.bonusOnly}
    <section class="block">
      <p class="overline">Inside the bonus</p>
      <div class="statline">
        {#if pop.spins}
          <div><span class="dim">Free spins awarded</span><strong>{pop.spins.meanInitial.toFixed(2)}</strong></div>
          <div><span class="dim">Free spins played (with retriggers)</span><strong>{pop.spins.meanFinal.toFixed(2)}</strong></div>
        {/if}
        {#if pop.bonusOnly}
          <div><span class="dim">Bonus alone — average</span><strong>{fx(pop.bonusOnly.mean)}</strong></div>
          <div><span class="dim">Bonus alone — median</span><strong>{fx(pop.bonusOnly.median)}</strong></div>
          <div><span class="dim">Share of the book's win</span><strong>{pct(pop.bonusOnly.shareOfPayout)}</strong></div>
        {/if}
      </div>
      {#if pop.spins && pop.spins.rows.length > 1}
        <table>
          <thead>
            <tr><th>Spins awarded</th><th class="num">Share</th><th class="num">Median</th><th class="num">Average</th><th class="num">Share of value</th></tr>
          </thead>
          <tbody>
            {#each pop.spins.rows as g (g.key)}
              <tr>
                <td>{g.key}</td>
                <td class="num">{pct(g.share)}</td>
                <td class="num">{fx(g.median)}</td>
                <td class="num">{fx(g.avg)}</td>
                <td class="num">{pct(g.valueShare)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
      <p class="note">
        Spins are those of the first bonus of the book. “Bonus alone” is the book's
        <code>freeGameWins</code> — the win of the free spins without the base spin that
        opened them.
      </p>
    </section>
  {/if}

  <section class="block">
    <p class="overline">Bands</p>
    <table class="band-table">
      <thead>
        <tr>
          <th>Band (x bet)</th>
          <th class="num">Results</th>
          <th class="num">Cumulative</th>
          <th class="num">Value</th>
          <th class="num">Avg in band</th>
          <th class="num">Books</th>
          {#each kids as k, j (k.key)}
            <th class="num" class:split-first={j === 0}>{k.label}</th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each bandIdx as i (i)}
          {@const b = pop.bands[i]}
          <tr class:empty={b.share === 0}>
            <td>{bandLabel(b)}</td>
            <td class="num">{b.share > 0 ? pct(b.share) : '—'}<span class="bar"><span style="width: {b.share * 100}%"></span></span></td>
            <td class="num faint">{pct(b.cumShare)}</td>
            <td class="num">{b.valueShare > 0 ? pct(b.valueShare) : '—'}<span class="bar gold"><span style="width: {b.valueShare * 100}%"></span></span></td>
            <td class="num faint">{b.share > 0 ? fx(b.avg) : '—'}</td>
            <td class="num faint">{b.books.toLocaleString('en-US')}</td>
            {#each kids as k, j (k.key)}
              {@const perSpin = k.probability * (k.bands[i]?.share ?? 0)}
              <td class="num faint" class:split-first={j === 0}>
                {b.share > 0 ? pct(composition[i]?.[j] ?? 0) : '—'}{#if b.share > 0 && perSpin > 0}<span class="per-spin">{oneIn(1 / perSpin)}</span>{/if}
              </td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
    {#if kids.length}
      <p class="note">The last columns split each band between the parts of this population — they add up to 100 % of the band. Under each share: how often that part lands in the band, per spin of the mode.</p>
    {/if}
  </section>

  {#if pop.topPayouts.length}
    <section class="block">
      <p class="overline">Top wins</p>
      <table>
        <thead>
          <tr><th class="num">Win</th><th class="num">Frequency (per spin)</th><th class="num">Share here</th><th class="num">Books</th><th>Replay</th></tr>
        </thead>
        <tbody>
          {#each pop.topPayouts as tp (tp.payout)}
            <tr>
              <td class="num">{fx(tp.payout)}</td>
              <td class="num">{oneIn(tp.oneIn)}</td>
              <td class="num">{pct(tp.share)}</td>
              <td class="num">{tp.books}</td>
              <td>
                {#each tp.simIds as sid (sid)}
                  {@render simActions(sid, tp.payout)}
                {/each}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </section>
  {/if}

  <p class="note">
    Every number is weighted by the published lookup table — the game as it is
    played — and wins are in multiples of the base bet{cost !== 1 ? ` (this mode costs ${fx(cost)})` : ''}.
    A bonus is a book with a <code>freeSpinTrigger</code>; its tier is the number of
    positions of its first trigger, whatever criteria opened it.
  </p>
</div>

<style>
  .detail {
    min-width: 0;
  }

  .d-head {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 16px;
    flex-wrap: wrap;
    margin-bottom: 10px;
  }

  .d-crumb {
    margin: 0;
    font-size: var(--fs-xs);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  h2 {
    margin: 2px 0 0;
    font-size: var(--fs-xl);
    font-weight: 680;
  }

  .d-actions {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }

  .headline {
    font-size: var(--fs-lg);
    margin: 6px 0 8px;
    color: var(--text);
    max-width: 900px;
  }

  .findings {
    margin: 0 0 18px;
    padding-left: 18px;
    font-size: var(--fs-sm);
    color: var(--text-dim);
    line-height: 1.6;
    max-width: 900px;
  }

  .findings li.warn {
    color: var(--warn);
  }

  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: 10px;
    margin-bottom: 22px;
  }

  .card {
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    background: var(--bg-panel);
    padding: 10px 12px 8px;
  }

  .card .overline {
    margin-bottom: 6px;
  }

  dl {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 3px 10px;
    margin: 0;
    font-size: var(--fs-sm);
  }

  dt {
    color: var(--text-dim);
  }

  .term {
    padding: 0;
    border: none;
    background: none;
    font: inherit;
    color: inherit;
    text-align: left;
    text-decoration: underline dotted var(--border-strong);
    text-underline-offset: 3px;
  }

  .term:hover:not(:disabled) {
    background: none;
    color: var(--text);
  }

  .term .q {
    margin-left: 4px;
    font-size: var(--fs-2xs);
    color: var(--text-faint);
  }

  dd.def {
    grid-column: 1 / -1;
    text-align: left;
    font-family: var(--font-body);
    font-size: var(--fs-xs);
    color: var(--text-faint);
    padding: 2px 0 4px;
  }

  dd {
    margin: 0;
    text-align: right;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }

  dd.big {
    font-size: var(--fs-md);
  }

  dd.gold {
    color: var(--gold);
  }

  .warn-num {
    color: var(--warn);
  }

  .charts,
  .tables {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 28px;
    align-items: start;
    margin-bottom: 24px;
  }

  @media (max-width: 1100px) {
    .charts,
    .tables {
      grid-template-columns: 1fr;
    }
  }

  .block {
    margin-bottom: 24px;
  }

  .bar {
    display: block;
    height: 3px;
    margin-top: 3px;
    background: var(--row-line);
    border-radius: 2px;
    overflow: hidden;
  }

  .bar > span {
    display: block;
    height: 100%;
    background: var(--info);
  }

  .bar.gold > span {
    background: var(--gold);
  }

  .band-table .per-spin {
    display: block;
    font-size: 0.85em;
    opacity: 0.75;
  }

  .band-table .split-first {
    border-left: 1px solid var(--border);
  }

  tr.empty td {
    opacity: 0.45;
  }

  tr.median-row td {
    color: var(--gold);
  }

  tr.cost-row td {
    background: color-mix(in oklch, var(--gold) var(--tint-weak), transparent);
  }

  .row-active td:first-child button {
    color: var(--gold);
    font-weight: 640;
  }

  .thr-add {
    display: flex;
    gap: 6px;
    margin-top: 8px;
  }

  .thr-add input {
    width: 160px;
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
  }

  .x-rm {
    display: inline-flex;
    align-items: center;
    padding: 2px 3px;
    margin-left: 4px;
    vertical-align: middle;
  }

  button.link {
    background: none;
    border: none;
    padding: 0;
    color: var(--text);
    text-decoration: underline;
    text-decoration-color: var(--border-strong);
    text-underline-offset: 3px;
    cursor: pointer;
  }

  button.link:hover {
    color: var(--gold);
  }

  button.tiny {
    padding: 0 6px;
    font-size: var(--fs-xs);
  }

  .sim-actions {
    display: inline-flex;
    gap: 2px;
    align-items: center;
    margin-right: 8px;
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
  }

  .sim-actions button {
    display: inline-flex;
    align-items: center;
    padding: 3px 5px;
  }

  .note {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    max-width: 820px;
    margin: 10px 0 0;
    line-height: 1.55;
  }
</style>
