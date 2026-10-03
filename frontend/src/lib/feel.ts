/**
 * Player feel — types of the `feel` block of a CrowdSim cohort
 * (cf. `slotlab-core/src/feel.rs`) and the words that read it: each
 * frustration in the player's terms, each fix as « who pays », each figure as
 * before → after.
 */

import { fx, pct } from './populations';

export interface FeelParams {
  droughtSpins: number;
  droughtBonuses: number;
  sessionSpins: number;
  letdownBonuses: number;
  weakUnder: number;
  bigAt: number;
  /** Broke: a deposit, in stakes. */
  deposit: number;
  /** Reach: sessions of this many spins. */
  reachSpins: number;
  /** Crumbs: a purchase under this share of its price… */
  crumbsUnder: number;
  /** …and one that pops, at this multiple of its price. */
  popAt: number;
  /** Purchases in a row never refunded. */
  buyStreak: number;
}

export const DEFAULT_FEEL: FeelParams = {
  droughtSpins: 1000,
  droughtBonuses: 1,
  sessionSpins: 300,
  letdownBonuses: 3,
  weakUnder: 20,
  bigAt: 500,
  deposit: 100,
  reachSpins: 6000,
  crumbsUnder: 0.25,
  popAt: 10,
  buyStreak: 5,
};

export interface FeelMetrics {
  rtp: number;
  volatility: number;
  hitRate: number;
  /** Share of the wins paying less than the stake. */
  winsUnderStake: number;
  bonusOneIn: number | null;
  bonusMean: number | null;
  bonusMedian: number | null;
  /** Median ÷ average bonus (market 0.42–0.60). */
  bonusRatio: number | null;
  /** Share of the bonuses reaching the big line. */
  bigBonusShare: number | null;
  weakShare: number | null;
  drought: number | null;
  letdown: number | null;
  letdownSessions: number | null;
  dryMedian: number;
  dryP95: number;
  /** Players losing the deposit before their first bonus (estimated). */
  broke: number | null;
  bigOneIn: number | null;
  oneIn100: number | null;
  oneIn500: number | null;
  oneIn1000: number | null;
  oneIn5000: number | null;
  /** Share of `reachSpins`-spin sessions that never win `bigX`. */
  neverBig: number;
  maxWin: number;
  maxOneIn: number | null;
  zeroShare: number;
  // A bought round, against its price.
  buyMedian: number | null;
  crumbs: number | null;
  nearZero: number | null;
  refund: number | null;
  popOneIn: number | null;
  pop50OneIn: number | null;
  neverRefunded: number | null;
  /** RTP points per group, `FeelReport.groups` order. */
  budget: number[];
}

export type FrustrationKey = 'drought' | 'letdown' | 'dry' | 'broke' | 'reach' | 'crumbs' | 'shape';
export type Payer = 'base' | 'big' | 'bonus' | 'more' | 'rarer' | 'top' | 'middle' | 'typical' | 'frequency' | 'blend' | 'global';
export type FeelSeverity = 'serious' | 'minor' | 'ok';

export interface Move {
  group: string;
  mass: number;
  mean: number;
}

export interface FixOption {
  payer: Payer;
  feasible: boolean;
  /** Brings its frustration(s) to the aim (a blend can fall short). */
  reaches: boolean;
  /** How a blend was made. */
  note: string | null;
  reason: string | null;
  moves: Move[];
  after: FeelMetrics | null;
  /** Harm minus half the relief brought elsewhere; `null` when not playable. */
  sideCost: number | null;
}

export interface Fix {
  lever: 'bonusOneIn' | 'weakShare' | 'hitRate' | 'broke' | 'bigOneIn' | 'crumbs' | 'bonusRatio';
  from: number;
  to: number;
  options: FixOption[];
  recommended: number | null;
}

export interface Frustration {
  key: FrustrationKey;
  severity: FeelSeverity;
  /** Read, not flagged: an all-or-nothing mode's crumbs are its design. */
  byDesign: boolean;
  /** Two-sided reading (shape): the healthy band. */
  band: [number, number] | null;
  value: number;
  observed: number | null;
  sample: number;
  minor: number;
  serious: number;
  target: number;
  fix: Fix | null;
}

export interface Observed {
  droughtSessions: number;
  droughts: number;
  sessions: number;
  multiBonusSessions: number;
  letdowns: number;
  playersDrought: number;
  playersLetdown: number;
  dryMedian: number | null;
  players: number;
  broke: number;
  undecided: number;
  reachSessions: number;
  reachMisses: number;
  rounds: number;
  crumbsRounds: number;
  bonusRatio: number | null;
  bonuses: number;
}

/** One fix for everything. */
export interface GlobalFix {
  parts: FrustrationKey[];
  reached: FrustrationKey[];
  option: FixOption;
  /** Aimed at the market's median of the peers. */
  market: boolean;
}

export type MarketKey = 'hit' | 'winsUnderStake' | 'p100' | 'p500' | 'p1000' | 'p5000' | 'pmax' | 'never500' | 'never1000' | 'vol';

export interface MarketStat {
  key: MarketKey;
  p25: number;
  median: number;
  p75: number;
  value: number;
  after: number | null;
  /** Share of the peers under the value. */
  rank: number;
  rankAfter: number | null;
}

export interface MarketPeer {
  rank: number;
  name: string;
  studio: string;
  mode: string;
  cost: number;
  max: number;
  hit: number;
  p500: number;
  pmax: number;
  vol: number;
  share: number;
}

/** Where a mode stands among the market's games of a close max win. */
export interface MarketView {
  source: string;
  rule: string;
  fetched: string;
  coverage: number;
  /** The peers' kind (`base` | `ante`), max wins and costs (from, to). */
  kind: string;
  costs: [number, number];
  window: [number, number];
  count: number;
  stats: MarketStat[];
  peers: MarketPeer[];
}

export interface FeelReport {
  /** A base game, or a bought (costly) round. */
  kind: 'base' | 'buy';
  global: GlobalFix | null;
  params: FeelParams;
  spins: number;
  /** Thresholds in base bets: the letdown line (base) or the crumbs line (buy)… */
  weakX: number;
  /** …the big-win line (base) or the pop line (buy). */
  bigX: number;
  hasBonus: boolean;
  groups: string[];
  metrics: FeelMetrics;
  observed: Observed;
  frustrations: Frustration[];
  market: MarketView | null;
}

export interface FixRef {
  mode: string;
  frustration: FrustrationKey | 'global';
  payer: Payer;
}

const n0 = (v: number) => Math.round(v).toLocaleString('en-US');
const share = (v: number | null) => (v == null ? '—' : pct(v));
const times = (v: number | null) => (v == null ? '—' : fx(v));

/** « 1 in 4,984 », « 1 in 3.82M » — long odds read at a glance. */
export function oneIn(v: number | null): string {
  if (v == null || !isFinite(v)) return 'never';
  if (v >= 1e6) return `1 in ${(v / 1e6).toPrecision(3)}M`;
  if (v >= 1e5) return `1 in ${Math.round(v / 1e3)}k`;
  return `1 in ${n0(v)}`;
}

/** « 25 % of the price », « 10× the price ». */
const ofPrice = (v: number) => (v < 1 ? `${Math.round(v * 100)} % of the price` : `${v}× the price`);

// ── Frustrations ─────────────────────────────────────────────────────────────

export const FRUSTRATION_TITLE: Record<FrustrationKey, string> = {
  drought: 'Bonus droughts',
  letdown: 'Bonus letdowns',
  dry: 'Dry spells',
  broke: 'Broke before the bonus',
  reach: 'Big wins out of reach',
  crumbs: 'Purchases that refund nothing',
  shape: 'Bonus shape',
};

/** « one bonus or none », « 2 bonuses or fewer ». */
function atMost(m: number): string {
  return m === 0 ? 'no bonus' : m === 1 ? 'one bonus or none' : `${m} bonuses or fewer`;
}

/** The frustration in one sentence, with its figure. */
export function frustrationSentence(r: FeelReport, f: Frustration): string {
  const p = r.params;
  switch (f.key) {
    case 'drought':
      return `${pct(f.value)} of the ${n0(p.droughtSpins)}-spin sessions bring ${atMost(p.droughtBonuses)}`;
    case 'letdown':
      return `When ${n0(p.sessionSpins)} spins bring ${p.letdownBonuses}+ bonuses, ${pct(f.value)} of the time they all pay under ${fx(r.weakX)}`;
    case 'dry':
      return `The median player's longest run without a win: ${n0(f.value)} spins out of ${n0(r.spins)}`;
    case 'broke':
      return `${pct(f.value)} of the players who deposit ${n0(p.deposit)} stakes lose it all before their first bonus`;
    case 'reach':
      return `${pct(f.value)} of the ${n0(p.reachSpins)}-spin sessions never win ${fx(r.bigX)}`;
    case 'crumbs':
      return `${pct(f.value)} of the purchases return under ${ofPrice(p.crumbsUnder)}`;
    case 'shape': {
      const m = r.metrics;
      const head = `The typical bonus pays ${f.value.toFixed(2)} × the average one (${times(m.bonusMedian)} for ${times(m.bonusMean)})`;
      if (f.band && f.value < f.band[0]) return `${head}: the value hides in rare huge bonuses the typical session never meets`;
      if (f.band && f.value > f.band[1]) return `${head}: the bonus never surprises — « always 15-25x »`;
      return `${head}, inside the market band`;
    }
  }
}

/** What drives it, today. */
export function leverNow(r: FeelReport, key: FrustrationKey): string {
  const m = r.metrics;
  switch (key) {
    case 'drought':
      return `1 bonus in ${n0(m.bonusOneIn ?? 0)} spins`;
    case 'letdown':
      return `${share(m.weakShare)} of the bonuses pay under ${fx(r.weakX)} · median ${times(m.bonusMedian)}`;
    case 'dry':
      return `hit rate ${pct(m.hitRate)}`;
    case 'broke':
      return `1 bonus in ${n0(m.bonusOneIn ?? 0)} spins · the base game carries ${baseRtp(r).toFixed(1)} RTP points`;
    case 'reach':
      return `a win ≥ ${fx(r.bigX)} ${oneIn(m.bigOneIn).replace('1 in', '1 spin in')}`;
    case 'crumbs':
      return `median purchase ${share(m.buyMedian)} of the price · ${share(m.refund)} pay it back · ${n0(r.params.buyStreak)} in a row never refunded: ${share(m.neverRefunded)}`;
    case 'shape':
      return `median ${times(m.bonusMedian)} · average ${times(m.bonusMean)} · ${share(m.bigBonusShare)} of the bonuses reach ${fx(r.bigX)}`;
  }
}

/** RTP points carried by the spins without bonus. */
export function baseRtp(r: FeelReport): number {
  return r.groups.reduce((a, g, i) => (['dead', 'base', 'baseBig'].includes(g) ? a + (r.metrics.budget[i] ?? 0) : a), 0);
}

/** The observed figure, as the cohort lived it. */
export function observedLine(r: FeelReport, f: Frustration): string {
  const o = r.observed;
  if (f.observed == null) {
    if (f.key === 'dry') return '';
    if (f.key === 'reach') return `sessions longer than the run (${n0(r.spins)} spins): nothing observed`;
    return 'sessions longer than the run: nothing observed';
  }
  switch (f.key) {
    case 'drought':
      return `lived: ${pct(f.observed)} of ${n0(o.droughtSessions)} sessions · ${pct(o.playersDrought)} of the players at least once`;
    case 'letdown':
      return `lived: ${pct(f.observed)} of ${n0(o.multiBonusSessions)} sessions with ${r.params.letdownBonuses}+ bonuses · ${pct(o.playersLetdown)} of the players at least once`;
    case 'dry':
      return `lived: ${n0(f.observed)} spins for the median of ${n0(o.players)} players`;
    case 'broke':
      return (
        `lived: ${pct(f.observed)} of ${n0(o.players)} players` +
        (o.undecided > 0.02 * o.players ? ` (${pct(o.undecided / o.players)} neither broke nor bonused in ${n0(r.spins)} spins)` : '') +
        ' · estimate on 12,000 deposits'
      );
    case 'reach':
      return `lived: ${pct(f.observed)} of ${n0(o.reachSessions)} sessions`;
    case 'crumbs':
      return `lived: ${pct(f.observed)} of ${n0(o.rounds)} purchases`;
    case 'shape':
      return `lived: ${f.observed.toFixed(2)} over the ${n0(o.bonuses)} bonuses the cohort opened`;
  }
}

export function formatFrustration(key: FrustrationKey, v: number): string {
  return key === 'dry' ? `${n0(v)} spins` : key === 'shape' ? v.toFixed(2) : pct(v);
}

/** Past its line: over `minor`, or outside the band of a two-sided reading. */
export function pastLine(f: Frustration, v: number): boolean {
  return f.band ? v < f.band[0] || v > f.band[1] : v >= f.minor;
}

// ── Fixes ────────────────────────────────────────────────────────────────────

export function fixTitle(r: FeelReport, key: FrustrationKey, fix: Fix): string {
  switch (key) {
    case 'drought':
      return `More bonuses: 1 in ${n0(fix.from)} → 1 in ${n0(fix.to)} spins`;
    case 'letdown':
      return `Fewer weak bonuses: ${pct(fix.from)} → ${pct(fix.to)} pay under ${fx(r.weakX)}`;
    case 'dry':
      return `More wins: hit rate ${pct(fix.from)} → ${pct(fix.to)}`;
    case 'broke':
      return `Fewer players broke before the bonus: ${pct(fix.from)} → ${pct(fix.to)}`;
    case 'reach':
      return `Big wins within reach: ≥ ${fx(r.bigX)} ${oneIn(fix.from)} → ${oneIn(fix.to)} spins`;
    case 'crumbs':
      return `Fewer crumbs: ${pct(fix.from)} → ${pct(fix.to)} of the purchases under ${ofPrice(r.params.crumbsUnder)}`;
    case 'shape':
      return fix.to > fix.from
        ? `A typical bonus closer to the average one: ${fix.from.toFixed(2)} → ${fix.to.toFixed(2)} × the average`
        : `A bonus that can surprise: ${fix.from.toFixed(2)} → ${fix.to.toFixed(2)} × the average`;
  }
}

const PAYER: Record<FrustrationKey, Partial<Record<Payer, { title: string; how: string }>>> = {
  drought: {
    base: { title: 'The base game pays', how: 'Fewer base wins make room for the extra bonuses.' },
    big: { title: 'The big wins pay', how: 'Wins at the top become rarer; each bonus keeps its value.' },
    bonus: { title: 'Each bonus pays less', how: 'More bonuses, each one smaller: the bonus keeps its share of the RTP.' },
  },
  letdown: {
    base: { title: 'The base game pays', how: 'Weak bonuses become normal ones; fewer base wins pay for it.' },
    big: { title: 'The big wins pay', how: 'Weak bonuses become normal ones; the wins at the top pay for it.' },
    bonus: { title: 'Rarer bonuses', how: 'Weak bonuses become normal ones; the bonus comes less often.' },
  },
  dry: {
    base: { title: 'Smaller base wins', how: 'More base wins, each one smaller: the base game keeps its RTP.' },
    big: { title: 'The big wins pay', how: 'More base wins; the wins at the top pay for them.' },
    bonus: { title: 'Rarer bonuses', how: 'More base wins; the bonus comes less often.' },
  },
  broke: {
    bonus: { title: 'Smaller bonuses feed the base game', how: 'More base wins keep the balance alive; every bonus pays a little less.' },
    big: { title: 'The big wins feed the base game', how: 'More base wins keep the balance alive; the wins at the top pay for them.' },
    more: { title: 'More bonuses, each smaller', how: 'The first bonus comes sooner; each bonus pays less.' },
  },
  reach: {
    bonus: { title: 'All-or-nothing bonuses', how: 'More bonuses reach the big line; the typical bonus pays for it, and some fall weak.' },
    base: { title: 'The base game pays', how: 'Big wins more often; fewer base wins pay for them.' },
    rarer: { title: 'Rarer bonuses', how: 'Big wins more often; the bonus comes less often.' },
  },
  crumbs: {
    top: { title: 'The pops pay', how: 'Crumbs lifted into the middle; the purchases that pop big become rarer.' },
    middle: { title: 'The middle pays', how: 'Crumbs lifted into the middle, which pays a little less: fewer refunds, the pops untouched.' },
  },
  // Raising the ratio: the biggest bonuses give their RTP to…
  shape: {
    typical: { title: 'Into the typical bonus', how: 'The biggest bonuses become rarer; their RTP lifts the weak bonuses into the middle. The bonus keeps its RTP and its frequency.' },
    base: { title: 'Into the base game', how: 'The biggest bonuses become rarer; their RTP goes to more base wins.' },
    frequency: { title: 'Into more bonuses', how: 'The biggest bonuses become rarer; their RTP pays for more frequent bonuses.' },
  },
};

/** Lowering the ratio (a flat bonus): more big bonuses, paid by… */
const SHAPE_LOWER: Partial<Record<Payer, { title: string; how: string }>> = {
  typical: { title: 'All-or-nothing bonuses', how: 'More big bonuses, paid by the middle of the bonus: some fall weak.' },
  base: { title: 'The base game pays', how: 'More big bonuses; fewer base wins pay for them.' },
  frequency: { title: 'Rarer bonuses', how: 'More big bonuses; the bonus comes less often.' },
};

const SHARED: Partial<Record<Payer, { title: string; how: string }>> = {
  blend: { title: 'Everyone pays a share', how: 'The average of the ways above, pushed until the aim is reached: each payer carries part of the cost.' },
  global: { title: 'One fix for everything', how: 'The average of every problem’s fix, pushed until each problem reaches its aim.' },
};

const payerOf = (key: FrustrationKey, payer: Payer, fix?: Fix) =>
  SHARED[payer] ?? (key === 'shape' && fix && fix.to < fix.from ? SHAPE_LOWER : PAYER[key])[payer];
export const payerTitle = (key: FrustrationKey, payer: Payer, fix?: Fix) => payerOf(key, payer, fix)?.title ?? payer;
export const payerHow = (key: FrustrationKey, payer: Payer, fix?: Fix) => payerOf(key, payer, fix)?.how ?? '';

/** Names of the budget groups, thresholds filled in. */
export function groupLabel(r: FeelReport, g: string): string {
  if (r.kind === 'buy') {
    const p = r.params;
    switch (g) {
      case 'weakBonus':
        return `Purchases < ${ofPrice(p.crumbsUnder)}`;
      case 'bonus':
        return `${Math.round(p.crumbsUnder * 100)} % – ${p.popAt}× the price`;
      case 'bigBonus':
        return `Purchases ≥ ${p.popAt}× the price`;
    }
  }
  switch (g) {
    case 'dead':
      return 'Losing spins';
    case 'base':
      return 'Base wins';
    case 'baseBig':
      return `Base wins ≥ ${fx(r.bigX)}`;
    case 'weakBonus':
      return `Bonuses < ${fx(r.weakX)}`;
    case 'bonus':
      return `Bonuses ${fx(r.weakX)}–${fx(r.bigX)}`;
    case 'bigBonus':
      return `Bonuses ≥ ${fx(r.bigX)}`;
    case 'maxWin':
      return `Max win (${fx(r.metrics.maxWin)})`;
    default:
      return g;
  }
}

/** Design token of each budget group (chart segments, legend). */
export const GROUP_COLOR: Record<string, string> = {
  dead: 'var(--series-8)',
  base: 'var(--series-2)',
  baseBig: 'var(--series-7)',
  weakBonus: 'var(--series-4)',
  bonus: 'var(--series-1)',
  bigBonus: 'var(--series-5)',
  maxWin: 'var(--series-6)',
};

/** « 24 % less often », « 87 % more often, 31 % smaller ». */
export function moveText(m: Move): string {
  const pctOf = (v: number) => `${Math.max(1, Math.round(Math.abs(v) * 100))} %`;
  const parts: string[] = [];
  if (Math.abs(m.mass - 1) > 0.005) parts.push(`${pctOf(m.mass - 1)} ${m.mass > 1 ? 'more' : 'less'} often`);
  if (Math.abs(m.mean - 1) > 0.005) parts.push(`${pctOf(m.mean - 1)} ${m.mean > 1 ? 'bigger' : 'smaller'}`);
  return parts.join(', ');
}

// ── Before → after ───────────────────────────────────────────────────────────

export interface MetricRow {
  key: keyof FeelMetrics;
  /** The frustration this figure is, if it is one. */
  frustration?: FrustrationKey;
  label: (r: FeelReport) => string;
  format: (v: number | null) => string;
  /** Which way is better for the player; `null`: neither. */
  better: 'lower' | 'higher' | null;
}

const BASE_ROWS: MetricRow[] = [
  {
    key: 'drought',
    frustration: 'drought',
    label: (r) => `${n0(r.params.droughtSpins)}-spin sessions with ${atMost(r.params.droughtBonuses)}`,
    format: share,
    better: 'lower',
  },
  {
    key: 'letdown',
    frustration: 'letdown',
    label: (r) => `${r.params.letdownBonuses}+ bonuses in ${n0(r.params.sessionSpins)} spins, all under ${fx(r.weakX)}`,
    format: share,
    better: 'lower',
  },
  {
    key: 'dryMedian',
    frustration: 'dry',
    label: (r) => `Longest run without a win (median, ${n0(r.spins)} spins)`,
    format: (v) => (v == null ? '—' : `${n0(v)} spins`),
    better: 'lower',
  },
  {
    key: 'broke',
    frustration: 'broke',
    label: (r) => `Broke before the first bonus (${n0(r.params.deposit)}-stake deposit)`,
    format: share,
    better: 'lower',
  },
  {
    key: 'neverBig',
    frustration: 'reach',
    label: (r) => `Never ${fx(r.bigX)} in ${n0(r.params.reachSpins)} spins`,
    format: share,
    better: 'lower',
  },
  { key: 'bonusOneIn', label: () => 'Bonus', format: oneIn, better: 'lower' },
  { key: 'bonusMedian', label: () => 'Median bonus', format: times, better: 'higher' },
  { key: 'bonusMean', label: () => 'Average bonus', format: times, better: 'higher' },
  { key: 'bonusRatio', frustration: 'shape', label: () => 'Median ÷ average bonus (market 0.42–0.60)', format: (v) => (v == null ? '—' : v.toFixed(2)), better: null },
  { key: 'bigBonusShare', label: (r) => `Bonuses reaching ${fx(r.bigX)}`, format: share, better: 'higher' },
  { key: 'weakShare', label: (r) => `Bonuses under ${fx(r.weakX)}`, format: share, better: 'lower' },
  { key: 'hitRate', label: () => 'Hit rate', format: share, better: 'higher' },
  { key: 'winsUnderStake', label: () => 'Wins paying less than the stake', format: share, better: 'lower' },
  { key: 'bigOneIn', label: (r) => `Wins ≥ ${fx(r.bigX)}`, format: oneIn, better: 'lower' },
  { key: 'maxOneIn', label: (r) => `Max win (${fx(r.metrics.maxWin)})`, format: oneIn, better: 'lower' },
  { key: 'volatility', label: () => 'Volatility (std ÷ stake)', format: (v) => (v == null ? '—' : v.toFixed(1)), better: null },
];

const BUY_ROWS: MetricRow[] = [
  { key: 'crumbs', frustration: 'crumbs', label: (r) => `Purchases under ${ofPrice(r.params.crumbsUnder)}`, format: share, better: 'lower' },
  { key: 'nearZero', label: () => 'Purchases under 10 % of the price', format: share, better: 'lower' },
  { key: 'buyMedian', label: () => 'Median purchase (share of the price)', format: share, better: 'higher' },
  { key: 'refund', label: () => 'Purchases that pay the price back', format: share, better: 'higher' },
  { key: 'neverRefunded', label: (r) => `${n0(r.params.buyStreak)} purchases in a row, none refunded`, format: share, better: 'lower' },
  { key: 'popOneIn', label: (r) => `Purchases ≥ ${r.params.popAt}× the price`, format: oneIn, better: 'lower' },
  { key: 'pop50OneIn', label: () => 'Purchases ≥ 50× the price', format: oneIn, better: 'lower' },
  { key: 'maxOneIn', label: (r) => `Max win (${fx(r.metrics.maxWin)})`, format: oneIn, better: 'lower' },
  { key: 'volatility', label: () => 'Volatility (std ÷ price)', format: (v) => (v == null ? '—' : v.toFixed(1)), better: null },
];

export const metricRows = (r: FeelReport) => (r.kind === 'buy' ? BUY_ROWS : BASE_ROWS);

export interface Delta {
  row: MetricRow;
  label: string;
  before: string;
  after: string;
  /** `gain` / `cost` for the player, `moved` when neither applies. */
  kind: 'gain' | 'cost' | 'moved';
  /** Relative size of the change, to rank. */
  size: number;
}

const num = (m: FeelMetrics, k: keyof FeelMetrics) => {
  const v = m[k];
  return typeof v === 'number' ? v : null;
};

/** Every figure that moves by more than 1 %, gains first. */
export function deltas(r: FeelReport, after: FeelMetrics): Delta[] {
  const out: Delta[] = [];
  for (const row of metricRows(r)) {
    const b = num(r.metrics, row.key);
    const a = num(after, row.key);
    if (b == null && a == null) continue;
    let size: number;
    let up: boolean;
    if (b == null || a == null) {
      size = 1;
      up = a != null;
    } else {
      if (b === a) continue;
      size = Math.abs(a - b) / Math.max(Math.abs(b), 1e-12);
      up = a > b;
    }
    if (size < 0.01) continue;
    // The shape reads toward its band: up when under it, down when above.
    const band = row.key === 'bonusRatio' ? r.frustrations.find((f) => f.key === 'shape')?.band : null;
    const better = band && b != null ? (b < band[0] ? 'higher' : b > band[1] ? 'lower' : null) : row.better;
    const kind: Delta['kind'] = better == null ? 'moved' : (better === 'higher') === up ? 'gain' : 'cost';
    out.push({ row, label: row.label(r), before: row.format(b), after: row.format(a), kind, size });
  }
  const rank = { gain: 0, cost: 1, moved: 2 };
  return out.sort((x, y) => rank[x.kind] - rank[y.kind] || y.size - x.size);
}

/** The other frustrations a fix pushes past their line (they were under it). */
export function crossings(r: FeelReport, fixing: FrustrationKey | 'global', after: FeelMetrics): { title: string; value: string; minor: string }[] {
  const out: { title: string; value: string; minor: string }[] = [];
  for (const row of metricRows(r)) {
    const f = row.frustration && r.frustrations.find((x) => x.key === row.frustration);
    if (!f || f.key === fixing || f.byDesign) continue;
    const b = num(r.metrics, row.key);
    const a = num(after, row.key);
    if (b == null || a == null || pastLine(f, b) || !pastLine(f, a)) continue;
    out.push({ title: FRUSTRATION_TITLE[f.key], value: formatFrustration(f.key, a), minor: formatFrustration(f.key, f.minor) });
  }
  return out;
}

// ── Market ───────────────────────────────────────────────────────────────────

const oddsOf = (p: number) => (p > 0 ? oneIn(1 / p) : 'never');

/** The mode's stake in base bets (3 for a 3x double chance). */
const stakeOf = (r: FeelReport) => (r.params.bigAt > 0 ? r.bigX / r.params.bigAt : 1);

/** « 500x », or for a costlier stake « 500× the stake (1,500x) »: the market reads in stakes, the cards above in base bets. */
function rung(r: FeelReport, t: number): string {
  const c = stakeOf(r);
  return Math.abs(c - 1) < 1e-9 ? fx(t) : `${n0(t)}× the stake (${fx(t * c)})`;
}

export const MARKET_ROWS: { key: MarketKey; label: (r: FeelReport) => string; format: (v: number) => string }[] = [
  { key: 'hit', label: () => 'Hit rate', format: (v) => pct(v) },
  { key: 'winsUnderStake', label: () => 'Wins under the stake (of the wins)', format: (v) => pct(v) },
  { key: 'p100', label: (r) => `Wins ≥ ${rung(r, 100)}`, format: oddsOf },
  { key: 'p500', label: (r) => `Wins ≥ ${rung(r, 500)}`, format: oddsOf },
  { key: 'p1000', label: (r) => `Wins ≥ ${rung(r, 1000)}`, format: oddsOf },
  { key: 'p5000', label: (r) => `Wins ≥ ${rung(r, 5000)}`, format: oddsOf },
  { key: 'pmax', label: (r) => `Max win (${fx(r.metrics.maxWin)})`, format: oddsOf },
  { key: 'never500', label: (r) => `6,000 spins without ${rung(r, 500)}`, format: (v) => pct(v) },
  { key: 'never1000', label: (r) => `6,000 spins without ${rung(r, 1000)}`, format: (v) => pct(v) },
  { key: 'vol', label: () => 'Volatility (std ÷ stake)', format: (v) => v.toFixed(1) },
];

/** Inside the peers' middle half (p25–p75)? */
export const inMiddle = (s: MarketStat, v: number) => v >= Math.min(s.p25, s.p75) && v <= Math.max(s.p25, s.p75);

/** « 1,000-spin sessions… » style position: where a value sits among the peers. */
export function standing(s: MarketStat, v: number, rank: number): string {
  if (inMiddle(s, v)) return 'middle half';
  const probability = s.key.startsWith('p') || s.key === 'hit';
  const above = rank >= 0.5;
  const share = Math.round((above ? rank : 1 - rank) * 100);
  if (probability) return above ? `more often than ${share} %` : `rarer than ${share} %`;
  return above ? `higher than ${share} %` : `lower than ${share} %`;
}

const FIX_WHAT: Record<FrustrationKey, string> = {
  drought: 'more bonuses',
  letdown: 'fewer weak bonuses',
  dry: 'more wins',
  broke: 'fewer broke',
  reach: 'big wins within reach',
  crumbs: 'fewer crumbs',
  shape: 'bonus shape',
};

export function cohortLabel(name: string, fix: FixRef | null | undefined): string {
  if (!fix) return name;
  if (fix.frustration === 'global') return `${fix.mode} · one fix for everything`;
  const who =
    fix.payer === 'blend'
      ? 'everyone pays a share'
      : fix.frustration === 'shape'
        ? { typical: 'typical bonus', base: 'base game', frequency: 'frequency' }[fix.payer as string]
        : null;
  return `${fix.mode} · ${FIX_WHAT[fix.frustration] ?? fix.frustration} · ${who ?? payerTitle(fix.frustration, fix.payer).toLowerCase()}`;
}
