<script lang="ts">
  import { pct } from '../../lib/populations';
  import { formatValue, type SimMetric } from '../../lib/crowdsim';

  /**
   * One per-player metric: the median in big, and a bar from P5 to P95 with
   * the middle half shaded, the median and the mean marked — a median near
   * the left end and a mean far right is a spread carried by a few players.
   */
  interface Props {
    m: SimMetric;
    players: number;
  }

  const { m, players }: Props = $props();

  const span = $derived(m.p95 - m.p5);
  const at = (v: number) => (span > 0 ? Math.min(100, Math.max(0, ((v - m.p5) / span) * 100)) : 50);
</script>

<div class="rc">
  <p class="label">{m.label}</p>
  <p class="med">{formatValue(m.p50, m.unit)}<span> median</span></p>
  <div
    class="bar"
    role="img"
    title="P5 → P95, shaded: P25 → P75"
    aria-label="Middle half of the players: {formatValue(m.p25, m.unit)} to {formatValue(m.p75, m.unit)}; mean {formatValue(m.mean, m.unit)}"
  >
    <span class="iqr" style="left: {at(m.p25)}%; width: {Math.max(1, at(m.p75) - at(m.p25))}%"></span>
    <span class="tick median" style="left: {at(m.p50)}%"></span>
    <span class="tick mean" style="left: {at(m.mean)}%"></span>
  </div>
  <div class="ends">
    <span>P5 {formatValue(m.p5, m.unit)}</span>
    <span>P95 {formatValue(m.p95, m.unit)}</span>
  </div>
  <p class="foot">
    mean {formatValue(m.mean, m.unit)} · max {formatValue(m.max, m.unit)}
    {#if m.players < players}<br />{pct(m.players / players)} of the players{/if}
  </p>
</div>

<style>
  .rc {
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    background: var(--bg-panel);
    padding: 11px 13px 10px;
    min-width: 0;
  }

  .label {
    margin: 0;
    font-size: var(--fs-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-faint);
  }

  .med {
    margin: 4px 0 10px;
    font-family: var(--font-mono);
    font-size: var(--fs-lg);
    font-weight: 650;
    color: var(--text);
  }

  .med span {
    font-family: var(--font-body);
    font-size: var(--fs-xs);
    font-weight: 400;
    color: var(--text-faint);
    margin-left: 4px;
  }

  .bar {
    position: relative;
    height: 8px;
    border-radius: var(--r-s);
    background: var(--border);
  }

  .iqr {
    position: absolute;
    top: 0;
    bottom: 0;
    border-radius: var(--r-s);
    background: color-mix(in oklch, var(--series-2) 55%, transparent);
  }

  .tick {
    position: absolute;
    top: -3px;
    width: 2px;
    height: 14px;
    margin-left: -1px;
    border-radius: 1px;
  }

  .tick.median {
    background: var(--gold);
  }

  .tick.mean {
    background: none;
    border-left: 2px dotted var(--series-4);
  }

  .ends {
    display: flex;
    justify-content: space-between;
    margin-top: 5px;
    font-family: var(--font-mono);
    font-size: var(--fs-2xs);
    color: var(--text-dim);
  }

  .foot {
    margin: 6px 0 0;
    font-size: var(--fs-2xs);
    color: var(--text-faint);
    line-height: 1.45;
  }
</style>
