<script lang="ts">
  import { fmtPct } from '../../lib/api';
  import { fx, pct } from '../../lib/populations';
  import {
    FRUSTRATION_TITLE,
    GROUP_COLOR,
    MARKET_ROWS,
    crossings,
    inMiddle,
    oneIn,
    standing,
    deltas,
    fixTitle,
    formatFrustration,
    frustrationSentence,
    groupLabel,
    leverNow,
    moveText,
    observedLine,
    payerHow,
    payerTitle,
    type FeelReport,
    type FixOption,
    type Frustration,
    type FrustrationKey,
    type Payer,
  } from '../../lib/feel';

  /**
   * Player feel of one cohort: each frustration (exact, and as the cohort
   * lived it), where the RTP goes, and for every frustration past its line,
   * the fix — three ways to pay for it, each read as before → after.
   */
  interface Props {
    r: FeelReport;
    /** Plays a fix as a new cohort; absent on a cohort that is already one. */
    onsimulate?: (frustration: FrustrationKey | 'global', payer: Payer) => void;
    /** `frustration:payer` of the fix being simulated. */
    simulating?: string | null;
  }

  const { r, onsimulate, simulating = null }: Props = $props();

  const SEV: Record<Frustration['severity'], string> = { serious: 'Serious', minor: 'Minor', ok: 'OK' };
  const rtpPts = $derived(r.metrics.budget.reduce((a, b) => a + b, 0));
  const fixes = $derived(r.frustrations.filter((f) => f.fix));
  const flagged = $derived(r.frustrations.filter((f) => f.severity !== 'ok'));

  const SHAPE_SERIOUS = [0.25, 0.75] as const;

  /** Where a value sits on its gauge: 0 → serious × 1.5 (or the value). */
  function scale(f: Frustration) {
    const top = Math.max(f.serious * 1.5, f.value * 1.08);
    const at = (v: number) => `${Math.min(100, Math.max(0, (v / top) * 100))}%`;
    return { at, top };
  }

  const segs = (budget: number[]) =>
    r.groups.map((g, i) => ({ g, pts: budget[i] ?? 0 })).filter((s) => s.pts > 0.005);
</script>

{#snippet budgetBar(budget: number[], label: string)}
  <div class="bbar" role="img" aria-label="{label}: {segs(budget).map((s) => `${groupLabel(r, s.g)} ${s.pts.toFixed(1)} points`).join(', ')}">
    {#each segs(budget) as s (s.g)}
      <span style="width: {(s.pts / rtpPts) * 100}%; background: {GROUP_COLOR[s.g]}" title="{groupLabel(r, s.g)}: {s.pts.toFixed(1)} pts"></span>
    {/each}
  </div>
{/snippet}

<p class="intro">
  {#if r.kind === 'buy'}
    Each purchase read against its price, exact on the published lookup table — the cohort's own purchases, “lived”, sit next to it.
  {:else}
    Exact on the published lookup table (the deposits: an estimate on 12,000 of them) — the cohort's own sessions, “lived”, sit next to it.
  {/if}
  A fix only changes how often each book is drawn: the books, the stake and the RTP stay.
</p>

<div class="gauges">
  {#each r.frustrations as f (f.key)}
    {@const sc = scale(f)}
    <article class="gauge {f.severity}" class:design={f.byDesign}>
      <header>
        <span class="g-title">{FRUSTRATION_TITLE[f.key]}</span>
        <span class="sev">{f.byDesign ? 'By design' : SEV[f.severity]}</span>
      </header>
      <strong class="g-big">{formatFrustration(f.key, f.value)}</strong>
      <p class="g-text">{frustrationSentence(r, f)}.</p>
      {#if f.band}
        {@const top = Math.max(1, f.value * 1.08)}
        {@const at = (v: number) => `${Math.min(100, Math.max(0, (v / top) * 100))}%`}
        <div class="scale" role="img" aria-label="{f.value.toFixed(2)}; market band {f.band[0]}–{f.band[1]}; serious under {SHAPE_SERIOUS[0]} or above {SHAPE_SERIOUS[1]}">
          <span class="z serious" style="left: 0; width: {at(SHAPE_SERIOUS[0])}"></span>
          <span class="z minor" style="left: {at(SHAPE_SERIOUS[0])}; width: calc({at(f.band[0])} - {at(SHAPE_SERIOUS[0])})"></span>
          <span class="z ok mid" style="left: {at(f.band[0])}; width: calc({at(f.band[1])} - {at(f.band[0])})"></span>
          <span class="z minor" style="left: {at(f.band[1])}; width: calc({at(SHAPE_SERIOUS[1])} - {at(f.band[1])})"></span>
          <span class="z serious" style="left: {at(SHAPE_SERIOUS[1])}; right: 0"></span>
          <i class="dot" style="left: {at(f.value)}"></i>
        </div>
        <p class="ticks">market band {f.band[0].toFixed(2)}–{f.band[1].toFixed(2)} · serious under {SHAPE_SERIOUS[0]} or above {SHAPE_SERIOUS[1]}</p>
      {:else}
        <div
          class="scale"
          role="img"
          aria-label="{formatFrustration(f.key, f.value)}; minor from {formatFrustration(f.key, f.minor)}, serious from {formatFrustration(f.key, f.serious)}; a fix aims at {formatFrustration(f.key, f.target)}"
        >
          <span class="z ok" style="width: {sc.at(f.minor)}"></span>
          <span class="z minor" style="left: {sc.at(f.minor)}; width: calc({sc.at(f.serious)} - {sc.at(f.minor)})"></span>
          <span class="z serious" style="left: {sc.at(f.serious)}; right: 0"></span>
          <i class="target" style="left: {sc.at(f.target)}" title="a fix aims at {formatFrustration(f.key, f.target)}"></i>
          <i class="dot" style="left: {sc.at(f.value)}"></i>
        </div>
        <p class="ticks">
          <i class="k-target" aria-hidden="true"></i>aim {formatFrustration(f.key, f.target)} · minor from {formatFrustration(f.key, f.minor)} · serious from {formatFrustration(f.key, f.serious)}
        </p>
      {/if}
      {#if f.byDesign}<p class="g-obs">{pct(r.metrics.zeroShare)} of the rounds pay nothing: an all-or-nothing mode, its crumbs are its identity — read, not flagged.</p>{/if}
      {#if observedLine(r, f)}<p class="g-obs">{observedLine(r, f)}</p>{/if}
      <p class="g-lever">Today: {leverNow(r, f.key)}</p>
    </article>
  {/each}
  {#if r.kind === 'base' && !r.hasBonus}
    <article class="gauge note">
      <span class="g-title">Bonus droughts & letdowns</span>
      <p class="g-text">
        No bonus to read here: either the books are not imported (Games → Import books), or none of this
        mode's books triggers free spins.
      </p>
    </article>
  {/if}
</div>

<div class="card budget">
  <p class="overline">Where the RTP goes — {rtpPts.toFixed(2)} points</p>
  {@render budgetBar(r.metrics.budget, 'RTP budget')}
  <ul class="legend">
    {#each segs(r.metrics.budget) as s (s.g)}
      <li><i style="background: {GROUP_COLOR[s.g]}"></i>{groupLabel(r, s.g)} <b>{s.pts.toFixed(1)}</b></li>
    {/each}
  </ul>
  <p class="foot">Every fix moves points from one line to another: the total never changes. That move is what it costs.</p>
</div>

{#if !flagged.length}
  <p class="all-ok">Nothing past its line: no fix to propose with these thresholds (Edit → Player feel to tighten them).</p>
{/if}

{#snippet optionBody(o: FixOption, fixing: FrustrationKey | 'global', rec: boolean, id: string, payer: Payer)}
  {#if !o.feasible || !o.after}
    <p class="reason">Not possible: {o.reason}.</p>
  {:else}
    {#if o.note}<p class="note">{o.note}.</p>{/if}
    {#if !o.reaches}<p class="cross" role="note">Short of the aim: the average of the fixes cannot get all the way there.</p>{/if}
    {#each crossings(r, fixing, o.after) as x (x.title)}
      <p class="cross" role="note">Pushes {x.title.toLowerCase()} past its line: {x.value} (minor from {x.minor}).</p>
    {/each}
    <ul class="moves" aria-label="What is re-weighted">
      {#each o.moves as m (m.group)}
        <li><i style="background: {GROUP_COLOR[m.group]}"></i><span>{groupLabel(r, m.group)}</span><b>{moveText(m)}</b></li>
      {/each}
    </ul>
    <ul class="deltas">
      {#each deltas(r, o.after) as d (d.row.key)}
        <li class={d.kind}>
          <span class="mark" aria-hidden="true">{d.kind === 'gain' ? '▲' : d.kind === 'cost' ? '▼' : '•'}</span>
          <span class="d-label"><span class="sr-only">{d.kind === 'gain' ? 'Better: ' : d.kind === 'cost' ? 'Worse: ' : ''}</span>{d.label}</span>
          <span class="d-val">{d.before} → <b>{d.after}</b></span>
        </li>
      {/each}
    </ul>
    <div class="bb-pair">
      <span class="bb-lbl">now</span>{@render budgetBar(r.metrics.budget, 'RTP budget now')}
      <span class="bb-lbl">after</span>{@render budgetBar(o.after.budget, 'RTP budget after the fix')}
    </div>
    {#if onsimulate}
      <button class="sim" class:primary={rec} disabled={simulating != null} onclick={() => onsimulate?.(fixing, payer)}>
        {simulating === id ? 'Simulating…' : 'Play it with the cohort'}
      </button>
    {/if}
  {/if}
{/snippet}

{#if r.market}
  {@const mk = r.market}
  <section class="card market">
    <p class="overline">Where this mode stands in the market</p>
    <p class="mk-sub">
      {#if mk.kind === 'ante'}
        The {mk.count} double-chance modes costing {mk.costs[0] === mk.costs[1] ? `${mk.costs[0]}×` : `${mk.costs[0]}×–${mk.costs[1]}×`} of the slots the market plays (max win {fx(mk.window[0])}–{fx(mk.window[1])}) —
      {:else}
        The {mk.count} slots the market plays with a max win of {fx(mk.window[0])}–{fx(mk.window[1])} —
      {/if}
      {mk.rule} ({pct(mk.coverage)} of the turnover) · {mk.source}, {mk.fetched}. Every figure in multiples of the stake.
    </p>
    <div class="mk-wrap">
      <table class="mk">
        <thead>
          <tr>
            <th scope="col"><span class="sr-only">Figure</span></th>
            <th scope="col" class="num">This mode</th>
            {#if r.global?.option.after}<th scope="col" class="num">After the global fix</th>{/if}
            <th scope="col" class="num">p25</th>
            <th scope="col" class="num">Median</th>
            <th scope="col" class="num">p75</th>
          </tr>
        </thead>
        <tbody>
          {#each MARKET_ROWS as row (row.key)}
            {@const st = mk.stats.find((x) => x.key === row.key)}
            {#if st}
              <tr>
                <th scope="row">{row.label(r)}</th>
                <td class="num" class:out={!inMiddle(st, st.value)}>
                  {row.format(st.value)}<small>{standing(st, st.value, st.rank)}</small>
                </td>
                {#if r.global?.option.after}
                  <td class="num" class:out={st.after != null && !inMiddle(st, st.after)}>
                    {st.after != null ? row.format(st.after) : '—'}{#if st.after != null && st.rankAfter != null}<small>{standing(st, st.after, st.rankAfter)}</small>{/if}
                  </td>
                {/if}
                <td class="num dimc">{row.format(st.p25)}</td>
                <td class="num med">{row.format(st.median)}</td>
                <td class="num dimc">{row.format(st.p75)}</td>
              </tr>
            {/if}
          {/each}
        </tbody>
      </table>
    </div>
    <p class="foot">
      Amber: outside the peers' middle half (p25–p75). For odds (1 in N), p25 is the rarer side and p75 the more frequent one.
      Bonus figures are not in a lookup table alone: they have no market column.
    </p>
    <details class="mk-peers">
      <summary>{mk.peers.length < mk.count ? `The ${mk.peers.length} most played of these ${mk.count} peers` : `The ${mk.count} peers, most played first`}</summary>
      <div class="mk-wrap">
        <table class="mk small">
          <thead>
            <tr><th scope="col" class="num">#</th><th scope="col">Slot</th><th scope="col">Studio</th>{#if mk.kind === 'ante'}<th scope="col">Mode</th>{/if}<th scope="col" class="num">Max win</th><th scope="col" class="num">Hit rate</th><th scope="col" class="num">{mk.kind === 'ante' ? 'Wins ≥ 500× the stake' : 'Wins ≥ 500x'}</th><th scope="col" class="num">Max win odds</th><th scope="col" class="num">Volatility</th><th scope="col" class="num">Turnover</th></tr>
          </thead>
          <tbody>
            {#each mk.peers as p (p.rank + p.mode)}
              <tr>
                <td class="num dimc">{p.rank}</td><td>{p.name}</td><td class="dimc">{p.studio}</td>{#if mk.kind === 'ante'}<td class="dimc">{p.mode} · {p.cost}×</td>{/if}<td class="num">{fx(p.max)}</td><td class="num">{pct(p.hit)}</td>
                <td class="num">{p.p500 > 0 ? oneIn(1 / p.p500) : 'never'}</td><td class="num">{p.pmax > 0 ? oneIn(1 / p.pmax) : '—'}</td><td class="num">{p.vol.toFixed(1)}</td><td class="num dimc">{pct(p.share)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </details>
  </section>
{/if}

{#if r.global}
  {@const g = r.global}
  <section class="fix global">
    <header>
      <p class="overline">One fix for everything{g.parts.length ? ` · ${g.parts.map((k) => FRUSTRATION_TITLE[k]).join(' + ')}` : ''}</p>
      <h3>{g.market ? 'Closest to the market, every problem on board' : `Fix the ${g.parts.length} problems at once`}</h3>
      <p class="dim">
        RTP stays {fmtPct(r.metrics.rtp, 2)}.
        {#if g.market}
          First the market's median of the peers (the table above), then each problem's best way — as far as the market's middle half allows.
        {:else}
          The average of every problem's fix, pushed until each one reaches its aim.
        {/if}
        {#each g.parts as k, i (k)}{i ? ' · ' : ' — '}<span class:ok={g.reached.includes(k)} class:miss={!g.reached.includes(k)}>{FRUSTRATION_TITLE[k]} {g.reached.includes(k) ? '✓' : '✗'}</span>{/each}
      </p>
    </header>
    <article class="opt rec wide">
      <header>
        <strong>{payerTitle(g.parts[0], 'global')}</strong>
        <span class="badge">{!g.parts.length ? 'Market median' : g.option.reaches ? 'Reaches every aim' : `${g.reached.length} of ${g.parts.length} aims`}</span>
      </header>
      {@render optionBody(g.option, 'global', true, 'global:global', 'global')}
    </article>
  </section>
{/if}

{#each fixes as f (f.key)}
  {@const fix = f.fix!}
  <section class="fix {f.severity}">
    <header>
      <p class="overline">Fix · {FRUSTRATION_TITLE[f.key]} ({formatFrustration(f.key, f.value)} → {formatFrustration(f.key, f.target)})</p>
      <h3>{fixTitle(r, f.key, fix)}</h3>
      <p class="dim">RTP stays {fmtPct(r.metrics.rtp, 2)}. Who pays?</p>
    </header>
    <div class="options">
      {#each fix.options as o, i (o.payer)}
        {@const rec = i === fix.recommended}
        <article class="opt" class:rec class:off={!o.feasible} class:shared={o.payer === 'blend'}>
          <header>
            <strong>{payerTitle(f.key, o.payer, fix)}</strong>
            {#if rec}<span class="badge" title="Hurts the rest of the game least">Recommended</span>{/if}
          </header>
          <p class="how">{payerHow(f.key, o.payer, fix)}</p>
          {@render optionBody(o, f.key, rec, `${f.key}:${o.payer}`, o.payer)}
        </article>
      {/each}
    </div>
    {#if fix.recommended != null}
      <p class="foot">
        {#if r.kind === 'buy'}
          Recommended: the way that hurts the purchase least — the refunds, the purchases that pop and the max win, weighed together.
        {:else}
          Recommended: the way that hurts the rest of the game least — the other frustrations, the hit rate,
          the wins under the stake, the big wins and the max win, weighed together — less half of what it relieves elsewhere.
        {/if}
      </p>
    {/if}
  </section>
{/each}

<style>
  .intro {
    margin: 0 0 12px;
    font-size: var(--fs-sm);
    color: var(--text-dim);
    max-width: 820px;
    line-height: 1.5;
  }

  .gauges {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 10px;
    margin-bottom: 12px;
  }

  .gauge {
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-top: 3px solid var(--pass);
    border-radius: var(--r-m);
    background: var(--bg-panel);
    min-width: 0;
  }

  .gauge.minor { border-top-color: var(--sv-minor); }
  .gauge.serious { border-top-color: var(--sv-serious); }
  .gauge.note,
  .gauge.design { border-top-color: var(--border-strong); }

  .gauge.design .sev { color: var(--text-dim); }

  .gauge header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 8px;
  }

  .g-title {
    font-size: var(--fs-2xs);
    text-transform: uppercase;
    letter-spacing: 0.07em;
    color: var(--text-faint);
  }

  .sev {
    font-size: var(--fs-xs);
    color: var(--pass);
  }

  .gauge.minor .sev { color: var(--sv-minor); }
  .gauge.serious .sev { color: var(--sv-serious); }

  .g-big {
    font-family: var(--font-mono);
    font-size: var(--fs-xl);
    font-weight: 650;
  }

  .gauge.minor .g-big { color: var(--sv-minor); }
  .gauge.serious .g-big { color: var(--sv-serious); }

  .g-text {
    margin: 0;
    font-size: var(--fs-sm);
    line-height: 1.45;
  }

  .scale {
    position: relative;
    height: 8px;
    margin-top: 6px;
    border-radius: var(--r-s);
    overflow: visible;
  }

  .z {
    position: absolute;
    top: 0;
    bottom: 0;
  }

  .z.ok { left: 0; background: color-mix(in oklch, var(--pass) 35%, transparent); border-radius: var(--r-s) 0 0 var(--r-s); }
  .z.ok.mid { border-radius: 0; }
  .z.minor { background: color-mix(in oklch, var(--sv-minor) 40%, transparent); }
  .z.serious { background: color-mix(in oklch, var(--sv-serious) 45%, transparent); border-radius: 0 var(--r-s) var(--r-s) 0; }

  .scale .target {
    position: absolute;
    top: -3px;
    height: 14px;
    margin-left: -1px;
    border-left: 2px dashed var(--text-dim);
  }

  .scale .dot {
    position: absolute;
    top: -4px;
    width: 16px;
    height: 16px;
    margin-left: -8px;
    border-radius: 50%;
    background: var(--text);
    border: 3px solid var(--bg-panel);
    box-sizing: border-box;
  }

  .ticks {
    margin: 0;
    font-size: var(--fs-2xs);
    color: var(--text-faint);
  }

  .k-target {
    display: inline-block;
    height: 9px;
    margin-right: 5px;
    vertical-align: -1px;
    border-left: 2px dashed var(--text-dim);
  }

  .g-obs,
  .g-lever {
    margin: 0;
    font-size: var(--fs-xs);
    color: var(--text-dim);
    line-height: 1.45;
  }

  .g-lever {
    color: var(--text-faint);
  }

  .card {
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    background: var(--bg-panel);
    padding: 12px 14px;
  }

  .budget {
    margin-bottom: 14px;
  }

  .bbar {
    display: flex;
    height: 12px;
    border-radius: var(--r-s);
    overflow: hidden;
    background: var(--border);
  }

  .bbar span {
    display: block;
    height: 100%;
    min-width: 1px;
  }

  .bbar span + span {
    box-shadow: inset 1px 0 0 var(--bg-panel);
  }

  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 16px;
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
    font-size: var(--fs-xs);
    color: var(--text-dim);
  }

  .legend li {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .legend i,
  .moves i {
    width: 9px;
    height: 9px;
    border-radius: 2px;
    flex: none;
  }

  .legend b {
    font-family: var(--font-mono);
    color: var(--text);
    font-weight: 600;
  }

  .foot {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    margin: 8px 0 0;
  }

  .all-ok {
    padding: 12px 14px;
    border: 1px dashed var(--border);
    border-radius: var(--r-m);
    color: var(--text-dim);
    font-size: var(--fs-sm);
  }

  .fix {
    margin-top: 18px;
  }

  .fix h3 {
    margin: 2px 0 2px;
    font-size: var(--fs-lg);
    font-weight: 650;
  }

  .fix .dim {
    margin: 0 0 10px;
    font-size: var(--fs-sm);
  }

  .options {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 10px;
  }

  .opt {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    background: var(--bg-panel);
    min-width: 0;
  }

  .opt.rec {
    border-color: color-mix(in oklch, var(--gold) 60%, transparent);
    box-shadow: inset 0 3px 0 var(--gold);
  }

  .opt.off {
    opacity: 0.75;
    background: transparent;
  }

  .opt header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 8px;
  }

  .opt header strong {
    font-size: var(--fs-md);
  }

  .badge {
    padding: 1px 8px;
    border-radius: var(--r-pill);
    font-size: var(--fs-2xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--gold-ink);
    background: var(--gold);
    white-space: nowrap;
  }

  .how,
  .reason {
    margin: 0;
    font-size: var(--fs-xs);
    color: var(--text-dim);
    line-height: 1.45;
  }

  .reason {
    color: var(--warn);
  }

  .cross {
    margin: 0;
    padding: 5px 8px;
    border-radius: var(--r-s);
    font-size: var(--fs-xs);
    line-height: 1.4;
    color: var(--sv-minor);
    background: color-mix(in oklch, var(--sv-minor) var(--tint-weak), transparent);
  }

  .moves,
  .deltas {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: var(--fs-xs);
  }

  .moves li {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-dim);
  }

  .moves b {
    margin-left: auto;
    font-family: var(--font-mono);
    font-weight: 500;
    color: var(--text);
    white-space: nowrap;
  }

  .deltas {
    padding-top: 6px;
    border-top: 1px solid var(--border);
  }

  .deltas li {
    display: grid;
    grid-template-columns: 12px minmax(0, 1fr) auto;
    gap: 6px;
    align-items: baseline;
  }

  .mark {
    font-size: 9px;
  }

  .deltas li.gain .mark { color: var(--pass); }
  .deltas li.cost .mark { color: var(--fail); }
  .deltas li.moved .mark { color: var(--text-faint); }

  .d-label {
    color: var(--text-dim);
  }

  .d-val {
    font-family: var(--font-mono);
    color: var(--text-faint);
    white-space: nowrap;
  }

  .deltas li.gain b { color: var(--pass); }
  .deltas li.cost b { color: var(--fail); }
  .deltas li.moved b { color: var(--text); }

  .bb-pair {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 8px;
    align-items: center;
    margin-top: 2px;
  }

  .bb-pair .bbar {
    height: 8px;
  }

  .bb-lbl {
    font-size: var(--fs-2xs);
    color: var(--text-faint);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .fix.global .opt.wide {
    max-width: 760px;
  }

  .market {
    margin-bottom: 14px;
  }

  .mk-sub {
    margin: 2px 0 10px;
    font-size: var(--fs-xs);
    color: var(--text-dim);
    line-height: 1.45;
  }

  .mk-wrap {
    overflow-x: auto;
  }

  table.mk {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--fs-sm);
  }

  table.mk th,
  table.mk td {
    padding: 5px 8px;
    border-bottom: 1px solid var(--border);
    text-align: left;
    vertical-align: baseline;
    white-space: nowrap;
  }

  table.mk thead th {
    font-size: var(--fs-2xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-faint);
    font-weight: 500;
  }

  table.mk tbody th {
    font-weight: 400;
    color: var(--text-dim);
  }

  table.mk .num {
    text-align: right;
    font-family: var(--font-mono);
  }

  table.mk td small {
    display: block;
    font-family: var(--font-body);
    font-size: var(--fs-2xs);
    color: var(--text-faint);
  }

  table.mk td.out {
    color: var(--sv-minor);
  }

  table.mk td.med {
    color: var(--text);
    font-weight: 600;
  }

  table.mk .dimc {
    color: var(--text-faint);
  }

  table.mk.small {
    font-size: var(--fs-xs);
  }

  .mk-peers summary {
    cursor: pointer;
    margin-top: 8px;
    font-size: var(--fs-xs);
    color: var(--text-dim);
  }

  .fix.global .dim .ok { color: var(--pass); }
  .fix.global .dim .miss { color: var(--sv-minor); }

  .opt.shared {
    border-style: dashed;
  }

  .opt.shared.rec {
    border-style: solid;
  }

  .note {
    margin: 0;
    font-size: var(--fs-xs);
    color: var(--text-faint);
    font-style: italic;
  }

  .sim {
    align-self: flex-start;
    margin-top: 2px;
    padding: 4px 12px;
    font-size: var(--fs-sm);
  }
</style>
