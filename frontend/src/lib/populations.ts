/**
 * Populations of a mode (cf. `slotlab-core/src/population.rs`) and what the
 * Analyze page derives from them: the tree, formatting, and the findings —
 * plain sentences computed from the numbers, never written by hand.
 */

export interface Quantile {
  q: number;
  value: number;
  simId: number;
}

export interface Threshold {
  x: number;
  below: number;
  atLeast: number;
  valueAtLeast: number;
}

export interface Band {
  start: number;
  /** `null`: open-ended last band. `start === end === 0`: exact zero. */
  end: number | null;
  books: number;
  /** Effective books of the band, (Σw)² / Σw². */
  effective: number;
  share: number;
  cumShare: number;
  valueShare: number;
  avg: number;
}

export interface CdfPoint {
  x: number;
  below: number;
}

export interface GroupRow {
  key: number;
  books: number;
  share: number;
  avg: number;
  median: number;
  valueShare: number;
}

export interface Retriggers {
  rate: number;
  mean: number;
  max: number;
  avgWith: number | null;
  avgWithout: number | null;
  valueShareWith: number;
  rows: GroupRow[];
}

export interface Spins {
  meanInitial: number;
  meanFinal: number;
  rows: GroupRow[];
}

export interface BonusOnly {
  mean: number;
  median: number;
  shareOfPayout: number;
}

export interface PopTopPayout {
  payout: number;
  oneIn: number;
  share: number;
  books: number;
  simIds: number[];
}

export type PopKind = 'all' | 'nobonus' | 'bonus' | 'tier' | 'variant' | 'criteria';

export interface Population {
  key: string;
  kind: PopKind;
  label: string;
  parent: string | null;
  tier: number | null;
  criteria: string | null;
  books: number;
  drawableBooks: number;
  effectiveOutcomes: number;
  probability: number;
  oneIn: number;
  shareOfBonuses: number | null;
  rtpContribution: number;
  rtpShare: number;
  mean: number;
  stdDev: number;
  cv: number;
  min: number;
  median: number;
  max: number;
  maxShare: number;
  meanOverMedian: number | null;
  quantiles: Quantile[];
  zeroRate: number;
  profitableRate: number;
  top1Share: number;
  top10Share: number;
  meanWithoutTop1: number;
  thresholds: Threshold[];
  bands: Band[];
  cdf: CdfPoint[];
  topPayouts: PopTopPayout[];
  retriggers: Retriggers | null;
  spins: Spins | null;
  bonusOnly: BonusOnly | null;
  /** Wins ≥ 100x (mode max win excluded) and how many are round; null: none. */
  roundAmounts: RoundAmounts | null;
}

/** Round amounts of a population — `slotlab-core/src/population.rs`. */
export interface RoundAmounts {
  /** Per spin of the mode: a win ≥ 100x, max win excluded. */
  atLeast100: number;
  /** Share of those that are an exact multiple of 100x. */
  share100: number;
  /** Per spin of the mode: a win ≥ 1,000x, max win excluded. */
  atLeast1000: number;
  /** Share of those that are an exact multiple of 1,000x. */
  share1000: number;
  /** Most frequent round amount (≥ 100x) and its share of the ≥ 100x wins. */
  topValue: number | null;
  topShare: number;
}

/** Paying symbols revealed per round, weighted by the lookup table. */
export interface SymbolMix {
  low: number;
  high: number;
}

/** What the player sees of the paying symbols: outside the bonus, then in the free spins. */
export interface SymbolsSeen {
  base: SymbolMix;
  freeSpins: SymbolMix;
}

/**
 * Reels slowing down for the missing scatter on the paid spin (the books' first
 * `reveal.anticipation`), as shares of the rounds, weighted by the lookup table.
 * BUBBLE HEX 2026-10-09: the « nearmiss » feature line only counts the criteria that
 * FORCE two scatters — ordinary spins that land them anticipate too.
 */
export interface AnticipationSeen {
  all: number;
  /** Anticipation and no bonus: the near miss the player feels, forced or natural. */
  withoutBonus: number;
  /** Share of the bonus rounds announced by an anticipation; null without bonuses. */
  bonusAnnounced: number | null;
}

export interface ModePopulations {
  mode: string;
  cost: number;
  imported: boolean;
  populations: Population[];
  /** Null: no readable board in the books, or symbol names neither H* / L* nor mapped by the front. */
  symbols?: SymbolsSeen | null;
  /** Null: no book of the mode anticipates (or its format does not say). */
  anticipation?: AnticipationSeen | null;
}

/** A population anywhere in the game: what Compare pins. */
export interface PopRef {
  mode: string;
  key: string;
}

export const refId = (r: PopRef) => `${r.mode}::${r.key}`;

// ── Formatting ───────────────────────────────────────────────────────────────

/** Bet multiple with a precision that follows the magnitude. */
export function fx(v: number): string {
  if (!isFinite(v)) return '—';
  const a = Math.abs(v);
  // Whole numbers read as thresholds (10x, 100x), not as measures (10.0x).
  const d = a === 0 || (Number.isInteger(v) && a >= 1) ? 0 : a < 10 ? 2 : a < 100 ? 1 : 0;
  return v.toLocaleString('en-US', { minimumFractionDigits: d, maximumFractionDigits: d }) + 'x';
}

/**
 * Percentage with enough digits to tell a rare share from zero. RTP and hit
 * rates use `fmtPct` (always 2 decimals) instead.
 */
export function pct(v: number): string {
  if (!isFinite(v)) return '—';
  const p = v * 100;
  if (p === 0) return '0 %';
  const a = Math.abs(p);
  const d = a >= 10 ? 1 : a >= 1 ? 1 : a >= 0.1 ? 2 : a >= 0.01 ? 3 : 4;
  return p.toFixed(d) + ' %';
}

export function oneIn(v: number): string {
  if (!isFinite(v)) return 'never';
  if (v < 10) return '1 / ' + v.toFixed(2);
  return '1 / ' + Math.round(v).toLocaleString('en-US');
}

export function bandLabel(b: { start: number; end: number | null }): string {
  if (b.start === 0 && b.end === 0) return '0';
  const k = (v: number) => (v >= 1000 ? `${v / 1000}k` : `${v}`);
  if (b.end === null) return `${k(b.start)}+`;
  return `${k(b.start)}–${k(b.end)}`;
}

export function threshold(p: Population, x: number): Threshold | undefined {
  return p.thresholds.find((t) => Math.abs(t.x - x) < 1e-9);
}

export function quantile(p: Population, q: number): Quantile | undefined {
  return p.quantiles.find((v) => Math.abs(v.q - q) < 1e-9);
}

/**
 * P(win < x) read on the population's CDF (1-1.5-2-3-5-7 per decade), exact on
 * a grid point, interpolated in log x between two — for a threshold the grid
 * does not hold (a tenth of a bonus average).
 */
export function cdfBelow(p: Population, x: number): number {
  const t = p.thresholds.find((v) => Math.abs(v.x - x) < 1e-9);
  if (t) return t.below;
  const pts = p.cdf;
  if (!pts.length) return 0;
  if (x <= pts[0].x) return x <= 0 ? 0 : pts[0].below * (x / pts[0].x);
  if (x >= pts[pts.length - 1].x) return pts[pts.length - 1].below;
  let i = 1;
  while (i < pts.length && pts[i].x < x) i++;
  const a = pts[i - 1];
  const b = pts[i];
  if (Math.abs(b.x - a.x) < 1e-12) return b.below;
  const u = (Math.log(x) - Math.log(a.x)) / (Math.log(b.x) - Math.log(a.x));
  return a.below + (b.below - a.below) * u;
}

// ── Tree ─────────────────────────────────────────────────────────────────────

export interface TreeGroup {
  title: string;
  items: { pop: Population; depth: number }[];
}

/** The populations of a mode, laid out as the Explore sidebar reads them. */
export function tree(mp: ModePopulations): TreeGroup[] {
  const pops = mp.populations;
  const groups: TreeGroup[] = [];
  const whole = pops.filter((p) => p.kind === 'all' || p.kind === 'nobonus');
  groups.push({ title: 'Mode', items: whole.map((pop) => ({ pop, depth: pop.kind === 'all' ? 0 : 1 })) });

  const bonus: TreeGroup['items'] = [];
  const all = pops.find((p) => p.kind === 'bonus');
  if (all) bonus.push({ pop: all, depth: 0 });
  for (const t of pops.filter((p) => p.kind === 'tier')) {
    bonus.push({ pop: t, depth: all ? 1 : 0 });
    for (const v of pops.filter((p) => p.kind === 'variant' && p.parent === t.key)) {
      bonus.push({ pop: v, depth: all ? 2 : 1 });
    }
  }
  if (bonus.length) groups.push({ title: 'Bonuses', items: bonus });

  const crit = pops.filter((p) => p.kind === 'criteria');
  if (crit.length) groups.push({ title: 'Outcome types (criteria)', items: crit.map((pop) => ({ pop, depth: 0 })) });
  return groups;
}

/** Direct sub-populations worth a breakdown table under a population. */
export function children(mp: ModePopulations, p: Population): Population[] {
  const pops = mp.populations;
  switch (p.kind) {
    case 'all':
      return pops.filter((c) => c.kind === 'nobonus' || c.kind === 'bonus' || (c.kind === 'tier' && !pops.some((b) => b.kind === 'bonus')));
    case 'bonus':
      return pops.filter((c) => c.kind === 'tier');
    case 'tier':
      return pops.filter((c) => c.kind === 'variant' && c.parent === p.key);
    default:
      return [];
  }
}

/** The same tier in every mode that has it — Base vs Hunt vs a buy. */
export function sameTierElsewhere(all: ModePopulations[], p: Population): { mode: string; pop: Population }[] {
  if (p.kind !== 'tier') return [];
  return all
    .map((mp) => ({ mode: mp.mode, pop: mp.populations.find((c) => c.kind === 'tier' && c.key === p.key) }))
    .filter((r): r is { mode: string; pop: Population } => !!r.pop);
}

/**
 * Composition of each band of a population by its sub-populations: the
 * share of the band's spins that each child accounts for. Exact — every
 * population is cut on the same band grid, so
 * P(child ∩ band) = child.probability × child.band.share.
 */
export function bandComposition(parent: Population, kids: Population[]): number[][] {
  return parent.bands.map((b, i) => {
    const whole = parent.probability * b.share;
    return kids.map((k) => (whole > 0 ? (k.probability * (k.bands[i]?.share ?? 0)) / whole : 0));
  });
}

// ── Findings ─────────────────────────────────────────────────────────────────

/** A sentence about one population (Explore); game-wide findings live in `findings.ts`. */
export interface Finding {
  level: 'info' | 'warn';
  text: string;
}

const STEPS = [10, 20, 50, 100];

/** The headline of a population: what the typical result is, next to the mean. */
export function headline(p: Population): string {
  const half = `Half pay under ${fx(p.median)}`;
  if (p.median === 0) {
    return `${pct(p.zeroRate)} pay nothing — average ${fx(p.mean)}`;
  }
  const ratio = p.meanOverMedian ?? 1;
  if (ratio >= 1.5) return `${half}, while the average is ${fx(p.mean)} (${ratio.toFixed(1)}× the median)`;
  return `${half} — average ${fx(p.mean)}, close to the median`;
}

/** Sentences about one population, from its own numbers. */
export function populationFindings(p: Population, cost: number): Finding[] {
  const out: Finding[] = [];
  const ratio = p.meanOverMedian;
  if (ratio !== null && ratio >= 2) {
    out.push({
      level: ratio >= 4 ? 'warn' : 'info',
      text: `The average (${fx(p.mean)}) is ${ratio.toFixed(1)}× the median (${fx(p.median)}): a few large results carry it.`,
    });
  }
  if (p.top1Share >= 0.2) {
    // A whole base game lives on its top 1 % (market base games: 72–82 %, median 77 %);
    // only a bonus or a buy is skewed at 40 %.
    const whole = p.kind === 'all' || p.kind === 'nobonus';
    out.push({
      level: p.top1Share >= (whole ? 0.85 : 0.4) ? 'warn' : 'info',
      text:
        `The top 1 % carries ${pct(p.top1Share)} of the value; without it the average would be ${fx(p.meanWithoutTop1)}.` +
        (whole && cost < 10 ? ' Market base games: 72–82 % (median 77 %) — see Market for the peers of this mode.' : ''),
    });
  }
  const unders = STEPS.map((x) => threshold(p, x)).filter((t): t is Threshold => !!t && t.below > 0 && t.below < 1);
  if (unders.length && p.kind !== 'all' && p.kind !== 'nobonus') {
    out.push({
      level: 'info',
      text: 'Ends under ' + unders.map((t) => `${fx(t.x)}: ${pct(t.below)}`).join(' · ') + '.',
    });
  }
  if (p.kind !== 'all' && p.kind !== 'nobonus' && p.kind !== 'criteria' && p.zeroRate > 0) {
    out.push({ level: 'warn', text: `${pct(p.zeroRate)} of these bonuses pay nothing.` });
  }
  if (cost > 1 && p.kind !== 'nobonus') {
    out.push({
      level: 'info',
      text:
        `${pct(p.profitableRate)} return at least the cost (${fx(cost)})` +
        (p.median > 0 ? `; the median is ${(p.median / cost).toFixed(2)}× the cost.` : '.'),
    });
  }
  const r = p.retriggers;
  if (r && r.rate > 0 && r.avgWith !== null) {
    const lift = r.avgWithout ? r.avgWith / r.avgWithout : null;
    out.push({
      level: 'info',
      text:
        `${pct(r.rate)} retrigger (${r.mean.toFixed(2)} on average, up to ${r.max}); they carry ${pct(r.valueShareWith)} of the value` +
        (lift ? ` and pay ${lift.toFixed(1)}× those that don't (${fx(r.avgWith)} vs ${fx(r.avgWithout!)}).` : '.'),
    });
  }
  if (p.effectiveOutcomes < 50 && p.drawableBooks > 0) {
    out.push({
      level: 'warn',
      text: `Only ${p.effectiveOutcomes.toFixed(0)} effective outcomes (${p.drawableBooks} drawable books): this population rests on very few distinct results.`,
    });
  }
  return out;
}

// ── Chart colours (design tokens → canvas) ───────────────────────────────────

/**
 * A canvas can't read `var(--token)`, and Chart.js parses colours itself (for
 * hover and alpha) without understanding oklch — so tokens are resolved once,
 * painted on a 1×1 canvas and read back as plain rgb().
 */
const resolved = new Map<string, string>();
let probe: CanvasRenderingContext2D | null | undefined;

function toRgb(color: string): string | null {
  if (probe === undefined) {
    const c = document.createElement('canvas');
    c.width = c.height = 1;
    probe = c.getContext('2d', { willReadFrequently: true });
  }
  if (!probe) return null;
  probe.clearRect(0, 0, 1, 1);
  probe.fillStyle = '#000';
  probe.fillStyle = color; // an unparsable colour leaves the previous one
  probe.fillRect(0, 0, 1, 1);
  const [r, g, b, a] = probe.getImageData(0, 0, 1, 1).data;
  return a >= 255 ? `rgb(${r}, ${g}, ${b})` : `rgba(${r}, ${g}, ${b}, ${(a / 255).toFixed(3)})`;
}

/** A colour token (`--series-2`, `--text-dim`…) as an rgb() string. */
export function tokenColor(token: string, fallback = 'rgb(139, 155, 144)'): string {
  const hit = resolved.get(token);
  if (hit) return hit;
  if (typeof document === 'undefined') return fallback;
  const raw = getComputedStyle(document.documentElement).getPropertyValue(token).trim();
  const rgb = raw ? toRgb(raw) : null;
  if (!rgb) return fallback;
  resolved.set(token, rgb);
  return rgb;
}

/** `rgb(r, g, b)` → `rgba(r, g, b, alpha)`. */
export function withAlpha(rgb: string, alpha: number): string {
  const m = /rgba?\((\d+),\s*(\d+),\s*(\d+)/.exec(rgb);
  return m ? `rgba(${m[1]}, ${m[2]}, ${m[3]}, ${alpha})` : rgb;
}

/** The colours every chart shares: axis text, grid lines, markers. */
export function chartColors() {
  return {
    text: tokenColor('--text-dim'),
    grid: withAlpha(tokenColor('--border-strong'), 0.35),
    gridFaint: withAlpha(tokenColor('--border-strong'), 0.18),
    /** Results / value: the two bars of a single-population band chart. */
    results: tokenColor('--series-2'),
    value: tokenColor('--series-1'),
    median: tokenColor('--series-3'),
    mean: tokenColor('--series-4'),
    cost: tokenColor('--series-8'),
  };
}

// ── Compare ──────────────────────────────────────────────────────────────────

const SERIES_COUNT = 8;

/** Colour of the i-th compared population (`--series-1..8`, resolved). */
export function seriesColor(i: number): string {
  return tokenColor(`--series-${(i % SERIES_COUNT) + 1}`);
}
