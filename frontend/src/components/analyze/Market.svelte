<script lang="ts">
  import { fx, pct } from '../../lib/populations';
  import {
    FIGURES,
    KIND_LABEL,
    inMiddleRow,
    odds,
    peersSentence,
    position,
    type FigureSpec,
    type GameMarket,
    type MarketRow,
  } from '../../lib/market';

  /**
   * The market — every mode of the game against the modes of the same kind
   * of the slots the market plays (base games of a close max win, antes of
   * the same cost, buys of a close cost): each figure's spread among them
   * (p10…p90, the middle half p25–p75, the median) and where the mode falls.
   */
  interface Props {
    game: string;
    selectedMode?: string;
    /** Read by the page (Analyze) — the findings use the same comparison. */
    data: GameMarket | null;
    loading: boolean;
    error: string | null;
    /** A new reference landed: read the comparison again. */
    onReload: () => void;
  }

  const { game, selectedMode = '', data, loading, error, onReload }: Props = $props();

  let active = $state('');

  // The pill follows the mode selected on the page (a finding opens its mode).
  $effect(() => {
    const want = selectedMode;
    const modes = data?.modes ?? [];
    if (modes.some((m) => m.mode === want)) active = want;
    else if (!modes.some((m) => m.mode === active)) active = modes[0]?.mode ?? '';
  });

  /** The reference is rebuilt every day from the 30-day ranking (server side). */
  interface Refresh {
    running: boolean;
    step: string;
    done: number;
    total: number;
    downloaded: number;
    lastError: string | null;
  }
  let refresh = $state<Refresh | null>(null);

  async function readRefresh() {
    try {
      const r = await fetch('/api/ui/market');
      const d = await r.json().catch(() => null);
      if (!r.ok || !d) return;
      const wasRunning = refresh?.running ?? false;
      refresh = d.refresh;
      // A new reference: read the comparison again.
      if (wasRunning && !d.refresh.running && game) onReload();
    } catch {
      /* the comparison above stays as it is */
    }
  }

  async function refreshNow() {
    await fetch('/api/ui/market/refresh', { method: 'POST' }).catch(() => null);
    await readRefresh();
  }

  $effect(() => {
    void readRefresh();
    const id = setInterval(() => void readRefresh(), refresh?.running ? 2000 : 60000);
    return () => clearInterval(id);
  });

  const cur = $derived(data?.modes.find((m) => m.mode === active));
  const buyish = $derived(cur?.kind === 'buy' || cur?.kind === 'bust');

  /** Positions (%) on the strip: p10 → p90 plus the mode, padded. */
  function strip(r: MarketRow, spec: FigureSpec) {
    const t = (v: number) => (spec.scale === 'log' ? Math.log10(Math.max(v, 1e-12)) : v);
    const vals = [r.p10, r.p90, r.value].map(t);
    let lo = Math.min(...vals);
    let hi = Math.max(...vals);
    const pad = (hi - lo) * 0.06 || 1;
    lo -= pad;
    hi += pad;
    const at = (v: number) => `${Math.min(100, Math.max(0, ((t(v) - lo) / (hi - lo)) * 100))}%`;
    return { at, zero: spec.scale === 'log' && !(r.value > 0) };
  }
</script>

<section class="market">
  {#if error}<div class="banner error" role="alert">Could not read the market: {error}</div>{/if}
  {#if loading && !data}<p class="dim" role="status">Reading the market…</p>{/if}
  {#if data}
    <p class="overline">The market</p>
    <p class="sub">
      Each mode against the slots the market plays — {data.rule}: {data.slots} slots, {data.size} modes,
      {pct(data.coverage)} of the turnover · {data.source}, {data.fetched}.
    </p>
    <p class="sub refresh" aria-live="polite">
      {#if refresh?.running}
        Updating the market from today's 30-day ranking… {refresh.done}/{refresh.total || '…'} lookup tables
        {#if refresh.downloaded}({refresh.downloaded} new){/if}
      {:else}
        Ranking of {data.fetched}, refreshed every day while SlotLab runs.
        {#if refresh?.lastError}<span class="err">Last update failed: {refresh.lastError}.</span>{/if}
        <button class="update" onclick={refreshNow}>Update now</button>
      {/if}
    </p>

    <div class="pills" role="tablist" aria-label="Modes">
      {#each data.modes as m (m.mode)}
        <button role="tab" aria-selected={active === m.mode} class:on={active === m.mode} onclick={() => (active = m.mode)}>
          {m.mode} <i>{KIND_LABEL[m.kind]} · {m.cost}×</i>
        </button>
      {/each}
    </div>
    {#if data.alone.length}
      <p class="faint note">No peer in the market for {data.alone.join(', ')} — a kind or a cost the market does not play.</p>
    {/if}

    {#if cur}
      <p class="peers-line">Compared with the <strong>{peersSentence(cur)}</strong>.</p>
      <div class="wrap card">
        <table>
          <thead>
            <tr>
              <th scope="col"><span class="sr-only">Figure</span></th>
              <th scope="col" class="num">{cur.mode}</th>
              <th scope="col" class="strip-h">
                <span>peers: p10 · <b>middle half</b> · p90</span>
              </th>
              <th scope="col" class="num">p25</th>
              <th scope="col" class="num">Median</th>
              <th scope="col" class="num">p75</th>
            </tr>
          </thead>
          <tbody>
            {#each cur.rows as r (r.key)}
              {@const spec = FIGURES[r.key]}
              {#if spec}
                {@const s = strip(r, spec)}
                <tr>
                  <th scope="row">{spec.label(cur)}</th>
                  <td class="num val" class:out={!inMiddleRow(r)}>
                    {spec.format(r.value)}<small>{position(r, spec)}</small>
                  </td>
                  <td class="strip-c">
                    <div class="strip" role="img" aria-label="{spec.label(cur)}: {spec.format(r.value)} — peers p25 {spec.format(r.p25)}, median {spec.format(r.median)}, p75 {spec.format(r.p75)}">
                      <span class="track" style="left: {s.at(r.p10)}; right: calc(100% - {s.at(r.p90)})"></span>
                      <span class="iqr" style="left: {s.at(r.p25)}; right: calc(100% - {s.at(r.p75)})"></span>
                      <span class="med" style="left: {s.at(r.median)}"></span>
                      <span class="dot" class:out={!inMiddleRow(r)} style="left: {s.zero ? '0%' : s.at(r.value)}"></span>
                    </div>
                  </td>
                  <td class="num dimc">{spec.format(r.p25)}</td>
                  <td class="num medc">{spec.format(r.median)}</td>
                  <td class="num dimc">{spec.format(r.p75)}</td>
                </tr>
              {/if}
            {/each}
          </tbody>
        </table>
      </div>
      <p class="foot">
        Amber: outside the peers' middle half. Odds are drawn on a log scale, rarer to the left. A slot counts once,
        through its mode closest to this one, and burst games are not slots. Every figure is in
        multiples of the mode's own {buyish ? 'price' : 'stake'} ({cur.cost}× the base bet), exactly as for the peers.
        Bonus figures (how often a bonus comes, what it pays) are not in a lookup table alone: Analyze and CrowdSim read them from the books.
      </p>

      <details class="peers">
        <summary>
          {cur.peers.length < cur.count ? `The ${cur.peers.length} most played of these ${cur.count} peers` : `The ${cur.count} peers, most played first`}
        </summary>
        <div class="wrap">
          <table class="small">
            <thead>
              {#if buyish}
                <tr><th class="num">#</th><th>Slot</th><th>Studio</th><th>Mode</th><th class="num">Price</th><th class="num">Median return</th><th class="num">Pays back</th><th class="num">Max win</th><th class="num">Volatility</th><th class="num">Turnover</th></tr>
              {:else}
                <tr><th class="num">#</th><th>Slot</th><th>Studio</th><th>Mode</th><th class="num">Max win</th><th class="num">Hit rate</th><th class="num">≥ 500× the stake</th><th class="num">Max win odds</th><th class="num">Volatility</th><th class="num">Turnover</th></tr>
              {/if}
            </thead>
            <tbody>
              {#each cur.peers as p (p.rank + p.mode)}
                <tr>
                  <td class="num dimc">{p.rank}</td>
                  <td>{p.name}</td>
                  <td class="dimc">{p.studio}</td>
                  <td class="dimc">{p.mode}</td>
                  {#if buyish}
                    <td class="num">{p.cost}×</td>
                    <td class="num">{pct(p.median)}</td>
                    <td class="num">{pct(p.p1)}</td>
                    <td class="num">{fx(p.max)} · {odds(p.pmax)}</td>
                  {:else}
                    <td class="num">{fx(p.max)}</td>
                    <td class="num">{pct(p.hit)}</td>
                    <td class="num">{odds(p.p500)}</td>
                    <td class="num">{odds(p.pmax)}</td>
                  {/if}
                  <td class="num">{p.vol.toFixed(1)}</td>
                  <td class="num dimc">{pct(p.share)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </details>
    {/if}
  {/if}
</section>

<style>
  .sub,
  .peers-line {
    margin: 2px 0 10px;
    font-size: var(--fs-sm);
    color: var(--text-dim);
    line-height: 1.5;
    max-width: 900px;
  }

  .refresh {
    margin-top: -6px;
  }

  .refresh .err {
    color: var(--fail);
  }

  .refresh .update {
    margin-left: 6px;
    padding: 1px 10px;
    border-radius: var(--r-pill);
    font-size: var(--fs-sm);
    background: var(--bg-panel);
  }

  .peers-line strong {
    color: var(--text);
    font-weight: 600;
  }

  .pills {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 10px;
  }

  .pills button {
    padding: 4px 12px;
    border-radius: var(--r-pill);
    font-size: var(--fs-sm);
    background: var(--bg-panel);
  }

  .pills button i {
    font-style: normal;
    font-size: var(--fs-2xs);
    color: var(--text-faint);
    margin-left: 4px;
  }

  .pills button.on {
    border-color: var(--gold);
    color: var(--gold);
    background: color-mix(in oklch, var(--gold) var(--tint-weak), transparent);
  }

  .note {
    font-size: var(--fs-xs);
    margin: 0 0 8px;
  }

  .card {
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    background: var(--bg-panel);
    padding: 4px 10px;
  }

  .wrap {
    overflow-x: auto;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--fs-sm);
  }

  th,
  td {
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
    text-align: left;
    vertical-align: middle;
    white-space: nowrap;
  }

  thead th {
    font-size: var(--fs-2xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-faint);
    font-weight: 500;
  }

  tbody th {
    font-weight: 400;
    color: var(--text-dim);
  }

  .num {
    text-align: right;
    font-family: var(--font-mono);
  }

  td.val small {
    display: block;
    font-family: var(--font-body);
    font-size: var(--fs-2xs);
    color: var(--text-faint);
  }

  td.val.out {
    color: var(--sv-minor);
  }

  .dimc {
    color: var(--text-faint);
  }

  .medc {
    color: var(--text);
    font-weight: 600;
  }

  .strip-h span {
    font-weight: 400;
  }

  .strip-h b {
    color: var(--text-dim);
    font-weight: 500;
  }

  .strip-c {
    width: 34%;
    min-width: 180px;
  }

  .strip {
    position: relative;
    height: 16px;
  }

  .strip .track {
    position: absolute;
    top: 7px;
    height: 2px;
    background: var(--border-strong);
  }

  .strip .iqr {
    position: absolute;
    top: 4px;
    height: 8px;
    border-radius: 2px;
    background: color-mix(in oklch, var(--series-2) 45%, transparent);
  }

  .strip .med {
    position: absolute;
    top: 1px;
    height: 14px;
    margin-left: -1px;
    border-left: 2px solid var(--text-dim);
  }

  .strip .dot {
    position: absolute;
    top: 2px;
    width: 12px;
    height: 12px;
    margin-left: -6px;
    border-radius: 50%;
    background: var(--pass);
    border: 2px solid var(--bg-panel);
    box-sizing: border-box;
  }

  .strip .dot.out {
    background: var(--sv-minor);
  }

  .foot {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    margin: 8px 0 0;
    line-height: 1.5;
    max-width: 900px;
  }

  .peers summary {
    cursor: pointer;
    margin-top: 10px;
    font-size: var(--fs-sm);
    color: var(--text-dim);
  }

  table.small {
    font-size: var(--fs-xs);
  }
</style>
