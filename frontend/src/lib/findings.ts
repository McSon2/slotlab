/**
 * « Worth a look » — everything that may be wrong with a game.
 *
 * Every rule reads numbers SlotLab already computes (compliance report,
 * 1-2-5 bands of each mode, populations, celebration ladders); none invents a
 * statistic. Each issue is ONE short sentence (`title`), ONE number (`big`)
 * next to its reference (`ref`), and how far past that reference it is
 * (`excess`, 1 = at the limit) — which is what ranks it:
 *
 *   blocking  a Stake check fails, or a studio rule marked blocking breaks;
 *   serious   2× the reference or more;
 *   minor     past the reference, less than 2×;
 *   context   worth knowing, not a fault.
 *
 * The specifics (`text`) and the reason the rule exists (`why`) only show
 * when a line is opened.
 */

import { fmtPct, type GameCheckReport, type ModeStats, type ModeTiers, type TierSpec } from './api';
import { fx, pct, oneIn, threshold, type Band, type ModePopulations, type Population } from './populations';

export type Severity = 'blocking' | 'serious' | 'minor' | 'context';
export const SEVERITIES: Severity[] = ['blocking', 'serious', 'minor', 'context'];

export type Category =
  | 'Compliance'
  | 'Distribution'
  | 'Bonus ladder'
  | 'Bonus shape'
  | 'Variants'
  | 'Buys'
  | 'Features'
  | 'Big wins'
  | 'Retriggers'
  | 'Across modes'
  | 'Sample'
  | 'Celebrations'
  | 'Data';

export interface Target {
  view: 'explore' | 'compliance' | 'tiers';
  mode?: string;
  key?: string;
}

export interface Issue {
  rule: string;
  severity: Severity;
  category: Category;
  /** `null`: the whole game. */
  mode: string | null;
  /** Other modes where the same issue holds (merged). */
  also?: string[];
  /** One short sentence. */
  title: string;
  /** The number that says how bad. */
  big: string;
  /** Its reference (« ≤ ×4 »). */
  ref: string;
  /** Times past the reference; ranks issues inside a severity. */
  excess: number;
  /** Specifics, shown when the line is opened. */
  text: string;
  /** Why the rule exists, and where its reference comes from. */
  why: string;
  target?: Target;
  merge?: string;
  sig?: number[];
}

/** Studio / Stake references used below — one place to read and tune them. */
export const REF = {
  stepMid: 4,
  stepLast: 8,
  mostLikelyMin: 1000,
  hitMin: 0.1,
  std3Star: 60,
  meanOverMedian: 3,
  top1: 0.4,
  essMin: 50,
  buyMedianOverCost: 0.1,
  buyProfitableMin: 0.1,
  sameTierMedianGap: 1.3,
  subBetWins: 0.6,
  plaqueEvery: 10,
  /** CVaR (mean of the worst 0.1 %, × base bet) ceiling of the 3★ grid — studio measurement. */
  cvar3Star: 50_000,
  /** A win ≥ 100x WITHOUT a bonus at most 1 in this many spins, per 1x of cost (studio target, Toxic Spins 2026-09-23). */
  outsideBonus100OneIn: 8_000,
  /** The target is a studio aim, not a Stake limit: only flag once this far past it (Toxic BASE at ×1.10 is fine). */
  outsideBonusTolerance: 1.25,
  /** At most this share of the ≥ 1,000x / ≥ 5,000x wins may come without a bonus. */
  outsideBonusTopShare: 0.3,
  /** At most this share of a mode's ≥ 100x (or ≥ 1,000x) wins may land on a round multiplier (chance: ~0.1 %). */
  roundShare: 0.02,
  /** Studio rule (Toxic Spins, 2026-10-04): drawable books per bonus tier, and per plain-spin feature carrying ≥ `poolFeatureRtpShare` of the RTP. */
  poolMin: 100_000,
  poolFeatureRtpShare: 0.05,
  /** Drawable books on the mode's max win (all identical in payout, not in board). */
  poolMaxWinMin: 10_000,
  /** A MEMORABLE band of a bonus tier (≥ `memorableFrom`, or ≥ `memorableFromCost` × the price of a buy) needs this many EFFECTIVE books (studio, 2026-10-04). */
  bandEffectiveMin: 10_000,
  memorableFrom: 1_000,
  memorableFromCost: 10,
};

const WHY = {
  steps: `Probability ratio between two consecutive 1-2-5 bands. Studio reference (skullperch BASE, judged good): ≤ ×${REF.stepMid} mid-curve, ≤ ×${REF.stepLast} into the max win. Above that, the curve reads as a hole.`,
  polarTop: 'The max-win band should be rarer than the band just below it; otherwise the top of the mode pays the body or the max win, little in between.',
  gap: 'No drawable result in a whole band between two populated ones. Stake checks « no significant gaps in payout ranges ».',
  mostLikely: `Stake requires the most likely simulation to land at most 1 in ${REF.mostLikelyMin}.`,
  cvar: `CVaR = average of the worst 0.1 % of outcomes for the operator, in × base bet. The 3★ grid caps it at ${REF.cvar3Star.toLocaleString('en-US')} (studio measurement on skullperch; SlotLab estimate per mode — Stake aggregates it at game level). It only binds when the max win is at least that high: a frequent max win is not a fault in itself (market degen reference: 1 / 496 at 30 000x, Jelly Siege).`,
  ladder: 'Studio rule (blocking): more trigger symbols must be rarer AND pay more.',
  skew: `Average ≥ ${REF.meanOverMedian}× the median, or the top 1 % carrying ≥ ${REF.top1 * 100} % of the value: most players get far less than the average suggests.`,
  polar: 'Half the results sit within 25 % of the minimum while the average is at least twice the median: the floor or a big win, little in between.',
  buy: 'Market reference (Waylanders Forge): the median purchase returns ≈ 0.25× its cost on a 100x buy, 0.5× on a 1000x one.',
  celebrations: 'From the Win tiers ladder: a plaque that lands every few spins is the rhythm of the mode, not an event.',
  outsideBonus: `Studio rule (2026-09-23, after Blood Water): the big wins belong to the bonuses. A ≥ 100x without a bonus at most 1 in ${REF.outsideBonus100OneIn.toLocaleString('en-US')} spins per 1x of cost (Blood Water, live: 1 / 2,997 — judged too many), and at most ${REF.outsideBonusTopShare * 100} % of the ≥ 1,000x / ≥ 5,000x wins without a bonus (Blood Water: 67-77 % above 5,000x). A huge win on a plain spin reads as artificial and steals the show from the bonus.`,
  round: `Studio rule: no spin paying x100 / x1,000 / x2,000 / x5,000 on the nose — a round multiplier reads as fake. An arbitrary win lands on a multiple of 100x ~0.1 % of the time, of 1,000x ~0.01 %. Above ${REF.roundShare * 100} % of the mode's big wins, a « one board × one multiplier » feature (wild multiplier, multi spin, Degen) or full screens make them. Mode max win excluded. Fix on the LUT: penalise round totals inside each band.`,
  pool: `Studio rule (2026-10-04, Toxic Spins: 34,000 books behind the 5-scatter bonus of the base game): at least ${(100_000).toLocaleString('en-US')} drawable books per bonus tier and per plain-spin feature (multi spin…), at least ${(10_000).toLocaleString('en-US')} on the max win, the rest for the base game. The lookup table sets the frequencies; the pool sets how many DIFFERENT rounds a player can see. Fix: re-allocate the criteria quotas (same total), rebuild the lookup table frozen — the distribution must not move.`,
  poolBands: 'Studio rule (2026-10-04): every band of a bonus from 1,000x up (from 10× the price of a buy) stands on at least 10,000 EFFECTIVE books ((Σw)² / Σw²) — those are the wins people film and remember; across all players the same round comes back after about √(effective books) hits (Toxic Spins HIDDEN 50k-100k: 271 effective for 1 hit in 4,181 spins). Below, no minimum: a 30x bonus is not remembered. An empty band between two populated ones (often just under the max win) is a hole in that bonus. Fix: a targeted criteria (same bonus, win kept inside the band) for the missing books, the lookup table frozen; few effective books among many books = the table piles the weight on a few of them.',
  ess: `Fewer than ${REF.essMin} effective outcomes ((Σw)² / Σw²): players will see the same results again and again.`,
};

export interface FindingsInput {
  stats: { mode: string; stats: ModeStats }[];
  checks: GameCheckReport | null;
  pops: ModePopulations[];
  tiers: { report: ModeTiers; spec: TierSpec }[];
  popsError?: string | null;
}

export function allFindings(input: FindingsInput): Issue[] {
  const out: Issue[] = [];
  compliance(input, out);
  distribution(input, out);
  for (const mp of input.pops) {
    ladder(mp, out);
    variants(mp, out);
  }
  shapes(input, out);
  bigWins(input, out);
  anteTiers(input, out);
  acrossModes(input, out);
  celebrations(input, out);
  pool(input, out);
  data(input, out);
  return mergeAcrossModes(out).sort(bySeverity);
}

export function bySeverity(a: Issue, b: Issue): number {
  return SEVERITIES.indexOf(a.severity) - SEVERITIES.indexOf(b.severity) || b.excess - a.excess;
}

/** Warn-level issue: serious from 2× its reference, minor below. */
const graded = (excess: number): Severity => (excess >= 2 ? 'serious' : 'minor');

/** Issues of one mode (merged ones included), most severe first. */
export function issuesOf(issues: Issue[], mode: string | null): Issue[] {
  return issues.filter((i) => i.mode === mode || (mode !== null && (i.also ?? []).includes(mode))).sort(bySeverity);
}

function mergeAcrossModes(issues: Issue[]): Issue[] {
  const out: Issue[] = [];
  const close = (a: number[] = [], b: number[] = []) =>
    a.length === b.length && a.every((v, i) => (v === 0 && b[i] === 0) || Math.abs(v - b[i]) <= 0.15 * Math.max(Math.abs(v), Math.abs(b[i])));
  for (const it of issues) {
    const host = it.merge
      ? out.find((o) => o.merge === it.merge && o.severity === it.severity && o.mode !== it.mode && !(o.also ?? []).includes(it.mode ?? '') && close(o.sig, it.sig))
      : undefined;
    if (host) host.also = [...(host.also ?? []), it.mode ?? ''];
    else out.push({ ...it });
  }
  return out;
}

// ── Formatting ───────────────────────────────────────────────────────────────

/** 1 234 → « 1.2k », 50 000 → « 50k ». */
export function fk(v: number): string {
  if (v >= 1000) {
    const k = v / 1000;
    return `${Number.isInteger(k) ? k : k.toFixed(1)}k`;
  }
  return Number.isInteger(v) ? `${v}` : v.toFixed(v < 1 ? 2 : 1);
}

const band = (b: { start: number; end: number }) => `${fk(b.start)}–${fk(b.end)}`;
const times = (r: number) => '×' + (r >= 100 ? Math.round(r).toLocaleString('en-US') : r.toFixed(1));
const shortOneIn = (v: number) => (isFinite(v) ? `1 / ${Math.round(v).toLocaleString('en-US')}` : 'never');

// ── Compliance ───────────────────────────────────────────────────────────────

function compliance({ checks, stats }: FindingsInput, out: Issue[]) {
  if (!checks) return;
  const perModeMostLikely = stats.some((m) => m.stats.mostLikelyOneIn < REF.mostLikelyMin);
  const sev = (s: string): Severity => (s === 'fail' ? 'blocking' : 'minor');
  let crossReported = false;
  for (const m of checks.modes) {
    for (const c of m.checks) {
      if (c.status !== 'fail' && c.status !== 'warn') continue;
      const game = c.id === 'cross_mode_rtp';
      if (game && crossReported) continue;
      crossReported ||= game;
      out.push({
        rule: `check:${c.id}`,
        severity: sev(c.status),
        category: 'Compliance',
        mode: game ? null : m.mode,
        title: c.label,
        big: c.value,
        ref: c.threshold,
        excess: 10,
        text: `${c.label}: ${c.value} — needs ${c.threshold}.`,
        why: 'Compliance per mode, official Stake report.',
        target: { view: 'compliance', mode: game ? undefined : m.mode },
      });
    }
  }
  for (const c of checks.publishChecks) {
    if (c.status !== 'fail' && c.status !== 'warn') continue;
    if (c.id === 'most_likely_hr' && perModeMostLikely) continue;
    out.push({ rule: `publish:${c.id}`, severity: sev(c.status), category: 'Compliance', mode: null, title: c.label, big: c.value, ref: c.threshold, excess: 10, text: `${c.label}: ${c.value}.`, why: 'RGS publish check.', target: { view: 'compliance' } });
  }
  for (const c of checks.betLevelConstraints) {
    const base = { rule: `star:${c.label}`, category: 'Compliance' as const, mode: null, big: c.value, why: 'Overall Bet Level Compliance: the 2★ and 3★ limits of the Stake report.', target: { view: 'compliance' as const } };
    if (!c.pass2star && !c.pass3star) out.push({ ...base, severity: 'blocking', title: `${c.label} fails both star tiers`, ref: `2★ ${c.limit2star} · 3★ ${c.limit3star}`, excess: 10, text: `${c.value}: over both limits.` });
    else if (!c.pass3star) out.push({ ...base, severity: 'serious', title: `${c.label} keeps the game out of 3★`, ref: `3★ ${c.limit3star}`, excess: 5, text: `${c.value}: over the 3★ limit.` });
    else if (!c.pass2star) out.push({ ...base, severity: 'context', title: `${c.label} over the 2★ limit (fine at 3★)`, ref: `2★ ${c.limit2star}`, excess: 1, text: `${c.value} passes 3★ (${c.limit3star}): the game is not eligible for 2★, which does not count as a fail at 3★.` });
  }

  const rtps = stats.map((m) => m.stats.rtp).sort((a, b) => a - b);
  const mid = rtps[Math.floor(rtps.length / 2)] ?? 0;
  for (const m of stats) {
    const d = m.stats.rtp - mid;
    if (Math.abs(d) > 0.001) {
      out.push({ rule: 'rtp-drift', severity: 'context', category: 'Compliance', mode: m.mode, title: 'RTP off the other modes', big: `${d > 0 ? '+' : ''}${(d * 100).toFixed(2)} pt`, ref: fmtPct(mid, 2), excess: Math.abs(d) * 100, text: `RTP ${fmtPct(m.stats.rtp, 2)} vs ${fmtPct(mid, 2)}.`, why: 'The report fails past 1 point of spread; a smaller drift is usually a lookup table not rebuilt.', target: { view: 'compliance', mode: m.mode } });
    }
  }

  const base = stats.find((m) => m.stats.cost <= 2) ?? stats[0];
  if (base) {
    const std = base.stats.stdDev / Math.max(1, base.stats.cost);
    if (std <= REF.std3Star && std >= REF.std3Star * 0.9) {
      out.push({
        rule: 'std-margin',
        severity: std >= REF.std3Star * 0.975 ? 'serious' : 'minor',
        category: 'Compliance',
        mode: base.mode,
        title: 'Almost at the 3★ volatility cap',
        big: std.toFixed(2),
        ref: `cap ${REF.std3Star}`,
        excess: std / (REF.std3Star * 0.9),
        text: `Std ÷ cost ${std.toFixed(2)}: ${(REF.std3Star - std).toFixed(2)} left before the cap.`,
        why: 'Any RTP moved to a rare tier raises the base volatility and crosses the 3★ cap.',
        target: { view: 'compliance', mode: base.mode },
      });
    }
  }
}

// ── Distribution (1-2-5 bands of each mode, from the LUT) ───────────────────

function distribution({ stats }: FindingsInput, out: Issue[]) {
  const gameMax = Math.max(0, ...stats.map((m) => m.stats.maxPayout));
  for (const { mode, stats: s } of stats) {
    const d = { category: 'Distribution' as const, mode, target: { view: 'explore' as const, mode, key: 'all' } };
    const bands = s.buckets.filter((b) => b.weight > 0 && !(b.start === 0 && b.end === 0));
    const gaps: string[] = [];
    const steps: { a: (typeof bands)[number]; b: (typeof bands)[number]; r: number; limit: number }[] = [];
    for (let i = 1; i < bands.length; i++) {
      const a = bands[i - 1];
      const b = bands[i];
      if (b.start - a.end > 1e-9) {
        gaps.push(`${fk(a.end)}–${fk(b.start)}`);
        continue;
      }
      const last = i === bands.length - 1;
      const limit = last ? REF.stepLast : REF.stepMid;
      const r = a.probability / b.probability;
      if (r > limit) steps.push({ a, b, r, limit });
      if (last && b.probability > a.probability && b.start >= 1000) {
        const q = b.probability / a.probability;
        out.push({ ...d, rule: 'polar-top', severity: graded(q), title: `Max win more likely than the whole ${band(a)} band`, big: times(q), ref: 'should be rarer', excess: q, text: `The max-win band (${band(b)}) is ${times(q)} more likely than ${band(a)}.`, why: WHY.polarTop });
      }
    }
    if (gaps.length) {
      out.push({ ...d, rule: 'gap', severity: 'blocking', title: `No win possible in ${gaps[0]}${gaps.length > 1 ? ` (+${gaps.length - 1})` : ''}`, big: 'gap', ref: 'Stake: no gaps', excess: 10, text: `No drawable result in ${gaps.join(', ')}.`, why: WHY.gap });
    }
    if (steps.length) {
      const w = steps.reduce((x, y) => (y.r / y.limit > x.r / x.limit ? y : x));
      const excess = w.r / w.limit;
      out.push({
        ...d,
        rule: 'steps',
        severity: graded(excess),
        title: `${excess >= 2 ? 'Hole' : 'Steep drop'} in the curve at ${band(w.b)}${steps.length > 1 ? ` (+${steps.length - 1} more)` : ''}`,
        big: times(w.r),
        ref: `vs ${band(w.a)} · ≤ ×${w.limit}`,
        excess,
        text: steps.map((x) => `${band(x.a)} → ${band(x.b)} ${times(x.r)}`).join(' · '),
        why: WHY.steps,
      });
    }
    if (s.mostLikelyOneIn < REF.mostLikelyMin) {
      out.push({ ...d, rule: 'most-likely', severity: 'blocking', title: 'Most likely book too frequent', big: shortOneIn(s.mostLikelyOneIn), ref: `≥ 1 / ${REF.mostLikelyMin}`, excess: REF.mostLikelyMin / s.mostLikelyOneIn, text: `The most likely book lands ${oneIn(s.mostLikelyOneIn)}.`, why: WHY.mostLikely });
    }
    if (s.cvar001 >= REF.cvar3Star * 0.95) {
      const excess = s.cvar001 / (REF.cvar3Star * 0.95);
      out.push({
        ...d,
        rule: 'cvar',
        severity: s.cvar001 >= REF.cvar3Star ? 'serious' : 'minor',
        title: s.cvar001 >= REF.cvar3Star ? 'Worst 0.1 % over the 3★ CVaR ceiling' : 'Worst 0.1 % close to the 3★ CVaR ceiling',
        big: `${fk(Math.round(s.cvar001))}x`,
        ref: `≤ ${fk(REF.cvar3Star)}x (3★)`,
        excess,
        text: `CVaR 0.1 % ${fx(s.cvar001)} · max win ${fx(s.maxPayout)} lands ${oneIn(s.maxPayoutOneIn)}: the rarer the max win, the lower the CVaR.`,
        why: WHY.cvar,
      });
    }
    if (s.maxPayout < gameMax) {
      out.push({ ...d, rule: 'never-max', severity: 'context', title: "Never reaches the game's max win", big: `${fk(s.maxPayout)}x`, ref: `game ${fk(gameMax)}x`, excess: gameMax / Math.max(s.maxPayout, 1), text: `Tops out at ${fx(s.maxPayout)}.`, why: "The mode's highest drawable result is below the game's max win." });
    }
    if (s.hitRate >= 0.05 && s.hitRate < REF.hitMin) {
      const excess = REF.hitMin / s.hitRate;
      out.push({ ...d, rule: 'hit-low', severity: graded(excess), title: 'Over 90 % of spins pay nothing', big: fmtPct(s.hitRate, 2), ref: '≥ 10.00 %', excess, text: `Hit rate ${fmtPct(s.hitRate, 2)}.`, why: 'Flagged by the Stake evaluator on tachyon (non-paying ≤ 90 %).' });
    }
    if (s.cost <= 2 && s.hitRate > 0) {
      const sub = (s.hitRate - s.breakevenRate) / s.hitRate;
      if (sub > REF.subBetWins) {
        out.push({ ...d, rule: 'sub-bet', severity: 'context', title: 'Most wins pay less than the bet', big: pct(sub), ref: `≤ ${REF.subBetWins * 100} %`, excess: sub / REF.subBetWins, text: `${pct(sub)} of the winning spins return less than the bet.`, why: '« Winning » mostly means losing a little less.' });
      }
    }
  }
}

// ── Bonus ladder & variants ─────────────────────────────────────────────────

function tiersOf(mp: ModePopulations): Population[] {
  return mp.populations.filter((p) => p.kind === 'tier' && p.tier !== null).sort((a, b) => a.tier! - b.tier!);
}

function ladder(mp: ModePopulations, out: Issue[]) {
  const tiers = tiersOf(mp);
  for (let i = 1; i < tiers.length; i++) {
    const lo = tiers[i - 1];
    const hi = tiers[i];
    const d = { category: 'Bonus ladder' as const, mode: mp.mode, why: WHY.ladder, target: { view: 'explore' as const, mode: mp.mode, key: hi.key } };
    if (hi.probability >= lo.probability) {
      out.push({ ...d, rule: 'ladder-freq', severity: 'blocking', title: `${hi.label} not rarer than ${lo.label}`, big: oneIn(hi.oneIn), ref: `vs ${oneIn(lo.oneIn)}`, excess: 10, text: `${hi.label} ${oneIn(hi.oneIn)} · ${lo.label} ${oneIn(lo.oneIn)}.` });
    }
    if (hi.mean <= lo.mean) {
      out.push({ ...d, rule: 'ladder-mean', severity: 'blocking', title: `${hi.label} pay less on average than ${lo.label}`, big: fx(hi.mean), ref: `vs ${fx(lo.mean)}`, excess: 10, text: `Average ${hi.label} ${fx(hi.mean)} · ${lo.label} ${fx(lo.mean)}.` });
    }
    if (hi.median <= lo.median) {
      out.push({ ...d, rule: 'ladder-median', severity: 'serious', title: `Typical ${hi.label} bonus no better than ${lo.label}`, big: fx(hi.median), ref: `vs ${fx(lo.median)}`, excess: 2, text: `Median ${hi.label} ${fx(hi.median)} · ${lo.label} ${fx(lo.median)}.`, why: 'The higher tier only pays better on average: its typical bonus is not better.' });
    }
    if (hi.spins && lo.spins && hi.spins.meanInitial < lo.spins.meanInitial) {
      out.push({ ...d, rule: 'ladder-spins', severity: 'minor', title: `${hi.label} award fewer spins than ${lo.label}`, big: hi.spins.meanInitial.toFixed(1), ref: `vs ${lo.spins.meanInitial.toFixed(1)}`, excess: 1.5, text: `${hi.label} ${hi.spins.meanInitial.toFixed(1)} spins · ${lo.label} ${lo.spins.meanInitial.toFixed(1)}.`, why: 'A higher tier awards fewer free spins at entry.' });
    }
  }
}

function variants(mp: ModePopulations, out: Issue[]) {
  const tiers = tiersOf(mp);
  for (const [i, t] of tiers.entries()) {
    const vs = mp.populations.filter((p) => p.kind === 'variant' && p.parent === t.key && p.min < p.max);
    if (!vs.length) continue;
    const below = tiers[i - 1];
    const above = tiers[i + 1];
    for (const v of vs) {
      const d = { category: 'Variants' as const, mode: mp.mode, target: { view: 'explore' as const, mode: mp.mode, key: v.key } };
      if (below && v.median < below.median) {
        out.push({ ...d, rule: 'variant-below', severity: 'serious', title: `${v.label} pays less than a typical ${below.label} bonus`, big: fx(v.median), ref: `vs ${fx(below.median)}`, excess: below.median / Math.max(v.median, 1e-9), text: `Median ${fx(v.median)} vs ${fx(below.median)}.`, why: 'More trigger symbols, less money.' });
      }
      if (above && v.median > above.median) {
        out.push({ ...d, rule: 'variant-above', severity: 'context', title: `${v.label} pays more than a typical ${above.label} bonus`, big: fx(v.median), ref: `vs ${fx(above.median)}`, excess: v.median / above.median, text: `Median ${fx(v.median)} vs ${fx(above.median)}.`, why: 'A way into a tier that pays more than the tier above.' });
      }
    }
    if (vs.length >= 2) {
      const byMed = [...vs].sort((a, b) => a.median - b.median);
      const lo = byMed[0];
      const hi = byMed[byMed.length - 1];
      if (lo.median > 0 && hi.median / lo.median >= 3) {
        const byCv = [...vs].sort((a, b) => a.cv - b.cv);
        out.push({
          rule: 'variant-spread',
          severity: 'context',
          category: 'Variants',
          mode: mp.mode,
          title: `${t.label}: very different bonuses behind one trigger`,
          big: times(hi.median / lo.median),
          ref: 'median spread',
          excess: hi.median / lo.median,
          text: `Medians ${fx(lo.median)} (${lo.criteria}) → ${fx(hi.median)} (${hi.criteria}) · most regular ${byCv[0].criteria}, most volatile ${byCv[byCv.length - 1].criteria}.`,
          why: 'The ways into a same tier differ by 3× or more in median.',
          target: { view: 'explore', mode: mp.mode, key: t.key },
        });
      }
    }
  }
}

// ── Shapes: bonus, buys, features, retriggers, sample ───────────────────────

const PRIORITY: Record<Population['kind'], number> = { tier: 0, variant: 1, bonus: 2, all: 3, criteria: 4, nobonus: 5 };
const near = (a: number, b: number, tol = 0.03) => (a === 0 && b === 0) || Math.abs(a - b) <= tol * Math.max(Math.abs(a), Math.abs(b));

/** Populations of a mode minus near-copies of a more meaningful one (`crit:freegame_4` = `tier:4`). */
export function distinctPopulations(mp: ModePopulations): Population[] {
  const kept: Population[] = [];
  for (const p of [...mp.populations].sort((a, b) => PRIORITY[a.kind] - PRIORITY[b.kind])) {
    if (kept.some((k) => near(k.probability, p.probability) && near(k.mean, p.mean) && near(k.median, p.median))) continue;
    kept.push(p);
  }
  return kept;
}

function shapes({ pops, stats }: FindingsInput, out: Issue[]) {
  const gameMax = Math.max(0, ...stats.map((m) => m.stats.maxPayout));
  for (const mp of pops) {
    const cost = mp.cost > 0 ? mp.cost : 1;
    const hasTiers = mp.populations.some((p) => p.kind === 'tier');
    const allBonus = !mp.populations.some((p) => p.kind === 'nobonus') && hasTiers;
    const raised = new Set<string>();

    const tierRetrig = mp.populations.filter((p) => p.kind === 'tier' && p.retriggers);
    if (tierRetrig.length && tierRetrig.every((p) => p.retriggers!.rate >= 0.999)) {
      const b = mp.populations.find((p) => p.kind === 'bonus') ?? mp.populations.find((p) => p.kind === 'all');
      const mean = b?.retriggers?.mean ?? tierRetrig[0].retriggers!.mean;
      out.push({ rule: 'retrig-all', severity: 'context', category: 'Retriggers', mode: mp.mode, title: 'Every bonus retriggers', big: `${mean.toFixed(1)} avg`, ref: 'intended?', excess: 1, text: `Every bonus retriggers, ${mean.toFixed(1)} times on average.`, why: 'Check it is the intended rule, not a counting artefact (e.g. one « retrigger » per fish eaten).', target: { view: 'explore', mode: mp.mode, key: b?.key ?? 'all' }, merge: 'retrig-all', sig: [] });
    }

    for (const p of distinctPopulations(mp)) {
      const single = p.min === p.max;
      const isBonus = p.kind === 'tier' || p.kind === 'variant' || (p.kind === 'bonus' && !hasTiers) || (p.kind === 'all' && allBonus);
      const isFeature = p.kind === 'criteria' && !p.retriggers && p.mean > 0 && p.probability < 0.05;
      const name = p.kind === 'all' ? 'Purchases' : p.label;
      const mergeKey = (rule: string) => `${rule}:${p.kind === 'all' ? 'all' : p.key}`;
      const speak = (rule: string) => {
        if (p.kind === 'variant' && p.parent && raised.has(`${rule}:${p.parent}`)) return false;
        raised.add(`${rule}:${p.key}`);
        return true;
      };
      const d = { mode: mp.mode, target: { view: 'explore' as const, mode: mp.mode, key: p.key } };

      if ((isBonus || isFeature) && !single) {
        const ratio = p.meanOverMedian ?? 0;
        const u10 = threshold(p, 10);
        const excess = Math.max(ratio / REF.meanOverMedian, p.top1Share / REF.top1);
        const skewed = isBonus ? excess >= 1 : ratio >= REF.meanOverMedian && p.top1Share >= 0.25;
        const rule = isBonus ? 'skew' : 'feature-skew';
        if (skewed && speak(rule)) {
          out.push({
            ...d,
            rule,
            severity: graded(excess),
            category: isBonus ? 'Bonus shape' : 'Features',
            title: `${name}: half pay under ${fx(p.median)}, average ${fx(p.mean)}`,
            big: `${ratio.toFixed(1)}×`,
            ref: `avg ÷ median ≤ ${REF.meanOverMedian}`,
            excess,
            text:
              (u10 && u10.below > 0.05 && u10.below < 1 ? `${pct(u10.below)} end under 10x · ` : '') +
              `the top 1 % carries ${pct(p.top1Share)} of the value · average without it ${fx(p.meanWithoutTop1)}.`,
            why: WHY.skew,
            merge: mergeKey(rule),
            sig: [p.median, p.mean],
          });
        }
        if (p.min > 0 && p.median <= 1.25 * p.min && p.mean >= 2 * p.median && speak('polar')) {
          const r = p.mean / p.median;
          out.push({
            ...d,
            rule: 'polar',
            severity: graded(r / 2),
            category: isBonus ? 'Bonus shape' : 'Features',
            title: `${name}: the floor or a big win, little between`,
            big: fx(p.median),
            ref: `median, floor ${fx(p.min)}`,
            excess: r / 2,
            text: `Minimum ${fx(p.min)} · median ${fx(p.median)} · average ${fx(p.mean)}.`,
            why: WHY.polar,
            merge: mergeKey('polar'),
            sig: [p.min, p.mean],
          });
        }
      }
      if (isBonus && p.zeroRate > 0 && !(p.kind === 'all' && p.zeroRate >= 0.5) && speak('zero')) {
        out.push({ ...d, rule: 'zero', severity: 'serious', category: 'Bonus shape', title: `${name}: some pay nothing`, big: pct(p.zeroRate), ref: '0 %', excess: 2 + p.zeroRate, text: `${pct(p.zeroRate)} end at 0x.`, why: 'A bonus that ends at 0x.', merge: mergeKey('zero'), sig: [p.zeroRate] });
      }
      if (isBonus && !single && p.maxShare >= 0.02 && p.max >= gameMax && speak('max')) {
        out.push({ ...d, rule: 'max-share', severity: 'context', category: 'Bonus shape', title: `${name}: often end on the max win`, big: pct(p.maxShare), ref: '< 2 %', excess: p.maxShare / 0.02, text: `${pct(p.maxShare)} end on ${fx(p.max)}.`, why: 'At least 2 % of these bonuses land exactly on the max win.', merge: mergeKey('max'), sig: [p.maxShare] });
      }
      if (isBonus && p.bonusOnly && p.bonusOnly.shareOfPayout < 0.5 && speak('fg')) {
        out.push({ ...d, rule: 'fg-share', severity: 'context', category: 'Bonus shape', title: `${name}: free spins pay less than the trigger`, big: pct(p.bonusOnly.shareOfPayout), ref: '≥ 50 %', excess: 0.5 / Math.max(p.bonusOnly.shareOfPayout, 0.01), text: 'Share of the win paid by the free spins themselves (`freeGameWins`).', why: 'The spin that opened the bonus pays most of it.' });
      }
      if (isFeature && p.zeroRate > 0.01 && p.zeroRate < 0.99) {
        out.push({ ...d, rule: 'feature-mixed', severity: 'context', category: 'Features', title: `${name}: mixes wins and dead spins`, big: pct(p.zeroRate), ref: 'at 0x', excess: 1, text: `${pct(p.zeroRate)} of its spins pay 0x.`, why: 'Part of the spins of this criteria pay nothing.' });
      }

      if (p.kind === 'all' && cost >= 10 && p.zeroRate < 0.5) {
        const medCost = p.median / cost;
        if (medCost < REF.buyMedianOverCost || p.profitableRate < REF.buyProfitableMin) {
          const excess = Math.max(REF.buyMedianOverCost / Math.max(medCost, 1e-6), REF.buyProfitableMin / Math.max(p.profitableRate, 1e-6));
          out.push({ ...d, rule: 'buy-median', severity: graded(excess), category: 'Buys', title: `A typical purchase returns ${medCost.toFixed(2)}× its cost`, big: fx(p.median), ref: `for ${fx(cost)} · market ≈ 0.25×`, excess, text: `${pct(p.profitableRate)} of purchases return at least the cost.`, why: WHY.buy });
        }
      }

      const r = p.retriggers;
      if (r && p.kind === 'tier' && !single && r.rate < 0.999 && r.avgWith !== null && r.avgWithout && r.avgWith / r.avgWithout >= 3) {
        const lift = r.avgWith / r.avgWithout;
        out.push({ ...d, rule: 'retrig-lift', severity: 'context', category: 'Retriggers', title: `${name}: retriggers carry the bonus`, big: times(lift), ref: 'with vs without', excess: lift / 3, text: `${fx(r.avgWith)} with a retrigger vs ${fx(r.avgWithout)} without · ${pct(r.rate)} of bonuses carry ${pct(r.valueShareWith)} of the value.`, why: 'Retriggered bonuses pay 3× or more those that do not.', merge: mergeKey('retrig-lift'), sig: [lift] });
      }

      if (!single && (isBonus || isFeature || p.rtpShare >= 0.005) && p.effectiveOutcomes < REF.essMin) {
        const excess = REF.essMin / Math.max(p.effectiveOutcomes, 1);
        out.push({ ...d, rule: 'ess', severity: graded(excess), category: 'Sample', title: `${name}: few distinct results`, big: p.effectiveOutcomes.toFixed(0), ref: `≥ ${REF.essMin}`, excess, text: `${p.effectiveOutcomes.toFixed(0)} effective outcomes out of ${p.drawableBooks} drawable books.`, why: WHY.ess });
      }
    }
  }
}

// ── Big wins: without a bonus, round amounts ────────────────────────────────

/** P(win ≥ x) per spin of the mode, carried by population `p`. */
function perSpinAtLeast(p: Population, x: number): number {
  return p.probability * (threshold(p, x)?.atLeast ?? 0);
}

function bigWins({ pops }: FindingsInput, out: Issue[]) {
  for (const mp of pops) {
    const cost = mp.cost > 0 ? mp.cost : 1;
    const all = mp.populations.find((p) => p.kind === 'all');
    const nb = mp.populations.find((p) => p.kind === 'nobonus');
    const withBonus = mp.populations.some((p) => p.kind === 'bonus');

    // Spin modes that mix plain spins and bonuses (buys: the LUT is the bonus).
    if (all && nb && withBonus && cost < 10) {
      const d = { mode: mp.mode, target: { view: 'explore' as const, mode: mp.mode, key: 'all' } };
      const r100 = perSpinAtLeast(nb, 100);
      const refOneIn = REF.outsideBonus100OneIn / cost;
      const all100 = perSpinAtLeast(all, 100);
      if (r100 > 0 && r100 * refOneIn > REF.outsideBonusTolerance) {
        const excess = r100 * refOneIn;
        out.push({
          ...d,
          rule: 'nobonus-100',
          severity: graded(excess),
          category: 'Big wins',
          title: 'Too many ≥ 100x wins without a bonus',
          big: shortOneIn(1 / r100),
          ref: `≤ ${shortOneIn(refOneIn)}`,
          excess,
          text: `A ≥ 100x without a bonus lands ${oneIn(1 / r100)} — ${pct(r100 / all100)} of all the ≥ 100x wins of the mode.`,
          why: WHY.outsideBonus,
        });
      }
      const tops = [1000, 5000]
        .map((x) => ({ x, whole: perSpinAtLeast(all, x), part: perSpinAtLeast(nb, x) }))
        .filter((t) => t.whole > 0 && t.part > 0)
        .map((t) => ({ ...t, share: t.part / t.whole }));
      const worst = tops.sort((a, b) => b.share - a.share)[0];
      if (worst && worst.share > REF.outsideBonusTopShare) {
        const excess = worst.share / REF.outsideBonusTopShare;
        out.push({
          ...d,
          rule: 'nobonus-top',
          severity: graded(excess),
          category: 'Big wins',
          title: `${pct(worst.share)} of the ≥ ${fk(worst.x)}x wins come without a bonus`,
          big: pct(worst.share),
          ref: `≤ ${REF.outsideBonusTopShare * 100} %`,
          excess,
          text: tops
            .sort((a, b) => a.x - b.x)
            .map((t) => `≥ ${fk(t.x)}x: ${pct(t.share)} without a bonus (${oneIn(1 / t.part)}) vs ${oneIn(1 / (t.whole - t.part))} from bonuses`)
            .join(' · ') + '.',
          why: WHY.outsideBonus,
        });
      }
    }

    // Round multipliers (x100, x200… x1,000, x5,000), judged on the MODE: what a
    // player sees is one spin paying x1,000.00 on the nose, whatever produced it.
    const ra = all?.roundAmounts;
    if (!ra || ra.atLeast100 <= 0) continue;
    const share = Math.max(ra.share100, ra.share1000);
    if (share <= REF.roundShare) continue;
    const perSpin = ra.atLeast100 * ra.share100;
    const sources = mp.populations
      .filter((p) => p.kind === 'criteria' && p.roundAmounts && p.roundAmounts.share100 > 0)
      .map((p) => ({ label: p.label, part: (p.roundAmounts!.atLeast100 * p.roundAmounts!.share100) / Math.max(perSpin, 1e-30) }))
      .sort((a, b) => b.part - a.part)
      .slice(0, 3);
    const excess = share / REF.roundShare;
    out.push({
      mode: mp.mode,
      target: { view: 'explore', mode: mp.mode, key: 'all' },
      rule: 'round',
      severity: graded(excess),
      category: 'Big wins',
      title: `${pct(share)} of the big wins land on a round multiplier`,
      big: pct(share),
      ref: `≤ ${REF.roundShare * 100} % · chance ≈ 0.1 %`,
      excess,
      text:
        `A spin pays a round multiplier (x100, x200… max win excluded) ${oneIn(1 / Math.max(perSpin, 1e-30))} — ${pct(ra.share100)} of the ≥ 100x wins` +
        (ra.atLeast1000 > 0 ? ` · ${pct(ra.share1000)} of the ≥ 1,000x wins are a multiple of 1,000x` : '') +
        (ra.topValue ? ` · most frequent x${ra.topValue.toLocaleString('en-US')} (${pct(ra.topShare)} of the ≥ 100x)` : '') +
        (sources.length ? ` · from ${sources.map((x) => `${x.label} ${pct(x.part)}`).join(', ')}` : '') +
        '.',
      why: WHY.round,
      merge: 'round',
      sig: [ra.share100, ra.share1000],
    });
  }
}

// ── Across modes ─────────────────────────────────────────────────────────────

/**
 * Studio rule (blocking, GREEDY GOBLIN 2026-10-02): a paid spin mode (ante, bonus boost, super
 * chance) must not make any bonus tier rarer than the base game does. Spin modes = modes whose
 * plain spins pay something (a `nobonus` population with a mean > 0), which leaves out buys and
 * mysteries whose « no bonus » draws pay nothing.
 */
function anteTiers({ pops }: FindingsInput, out: Issue[]) {
  const spinModes = pops.filter(
    (mp) => mp.populations.some((p) => p.kind === 'tier') && mp.populations.some((p) => p.kind === 'nobonus' && p.mean > 0),
  );
  if (spinModes.length < 2) return;
  const base = spinModes.reduce((a, b) => (b.cost < a.cost ? b : a));
  for (const mp of spinModes) {
    if (mp === base || mp.cost <= base.cost) continue;
    for (const t of tiersOf(base)) {
      const p = mp.populations.find((c) => c.kind === 'tier' && c.key === t.key);
      const pr = p?.probability ?? 0;
      if (pr >= t.probability) continue;
      out.push({
        rule: 'ante-tier',
        severity: 'blocking',
        category: 'Across modes',
        mode: mp.mode,
        title: `${t.label} rarer than in ${base.mode}`,
        big: pr > 0 ? shortOneIn(1 / pr) : 'never',
        ref: `${base.mode} ${shortOneIn(1 / t.probability)}`,
        excess: pr > 0 ? t.probability / pr : 10,
        text: `${t.label}: ${pr > 0 ? oneIn(1 / pr) : 'never'} in ${mp.mode} (cost ${fx(mp.cost)}) vs ${oneIn(1 / t.probability)} in ${base.mode} (cost ${fx(base.cost)}).`,
        why: 'Studio rule: a paid spin mode must not lower the chance of any bonus the base game gives — the player pays more to get more bonuses.',
        target: { view: 'explore', mode: mp.mode, key: t.key },
      });
    }
  }
}

function acrossModes({ pops }: FindingsInput, out: Issue[]) {
  const keys = new Set(pops.flatMap((mp) => mp.populations.filter((p) => p.kind === 'tier').map((p) => p.key)));
  for (const key of keys) {
    const rows = pops
      .map((mp) => ({ mode: mp.mode, pop: mp.populations.find((p) => p.key === key) }))
      .filter((r): r is { mode: string; pop: Population } => !!r.pop && r.pop.min < r.pop.max);
    if (rows.length < 2) continue;
    const lo = rows.reduce((a, b) => (b.pop.median < a.pop.median ? b : a));
    const hi = rows.reduce((a, b) => (b.pop.median > a.pop.median ? b : a));
    if (lo.pop.median > 0 && hi.pop.median / lo.pop.median >= REF.sameTierMedianGap) {
      const r = hi.pop.median / lo.pop.median;
      out.push({
        rule: 'same-tier',
        severity: 'context',
        category: 'Across modes',
        mode: hi.mode,
        also: [lo.mode],
        title: `${hi.pop.label} pay ${times(r)} more in ${hi.mode} than in ${lo.mode}`,
        big: `${fx(hi.pop.median)} vs ${fx(lo.pop.median)}`,
        ref: 'median',
        excess: r,
        text: `Averages ${fx(hi.pop.mean)} vs ${fx(lo.pop.mean)}.`,
        why: 'The same trigger does not feel the same from one mode to another.',
        target: { view: 'explore', mode: hi.mode, key },
      });
    }
  }
}

// ── Celebrations & data ──────────────────────────────────────────────────────

/**
 * Celebrations depend on what the spin cost:
 *   · base-like modes (≤ 2x): a plaque every few spins is the mode's rhythm, not
 *     an event — the Win tiers warnings apply as they are;
 *   · costly one-shot modes (DEGEN, MYSTERY, HIDDEN…): each spin IS an event;
 *     frequency is the mode's nature. The fault is a plaque that celebrates a
 *     result below what the spin cost;
 *   · pure buys: the lookup table only knows the bonus total, while plaques
 *     fire on each free spin (free, so ladder in × base bet by design) —
 *     nothing here can be measured from it.
 */
function celebrations({ tiers, pops, stats }: FindingsInput, out: Issue[]) {
  const guessed: string[] = [];
  for (const t of tiers) {
    const mode = t.report.mode;
    const cost = t.report.cost > 0 ? t.report.cost : 1;
    const target: Target = { view: 'tiers', mode };
    // Not the game's ladder: nothing to say about the game's celebrations.
    if (/default/i.test(t.report.source) && !/winLevelThresholds/.test(t.report.source)) {
      guessed.push(mode);
      continue;
    }
    const mp = pops.find((m) => m.mode === mode);
    const s = stats.find((m) => m.mode === mode)?.stats;
    // A buy: every round opens the bonus (or, in books without
    // freeSpinTrigger, a costly mode whose every round pays).
    const buy =
      (!!mp && mp.populations.some((p) => p.kind === 'tier') && !mp.populations.some((p) => p.kind === 'nobonus')) ||
      (cost >= 10 && (s?.hitRate ?? 0) >= 0.95);
    if (buy) continue;

    // Repeated spins (base, ante / hunt): the base-game rhythm rule.
    if (cost < 10) {
      const w = t.report.warnings;
      if (!w.length) continue;
      const often = w
        .map((x) => /^(.+?) lands 1 spin in ([\d.]+)/.exec(x))
        .filter((m): m is RegExpExecArray => !!m)
        .map((m) => ({ name: m[1], every: Number(m[2]) }));
      const rest = w.filter((x) => !/ lands 1 spin in /.test(x));
      const top = often.length ? often.reduce((a, b) => (b.every < a.every ? b : a)) : null;
      const excess = top ? REF.plaqueEvery / top.every : 1.5;
      out.push({
        rule: 'celebrations',
        severity: graded(excess),
        category: 'Celebrations',
        mode,
        title: top ? `${top.name} plaque ${top.every <= 1.05 ? 'on almost every spin' : `every ${top.every} spins`}` : 'Celebrations too long',
        big: top ? `1 / ${top.every}` : '',
        ref: `rarer than 1 / ${REF.plaqueEvery}`,
        excess,
        text: [...often.map((o) => `${o.name} every ${o.every} spins`), ...rest].join(' · ') + '.',
        why: WHY.celebrations,
        target,
      });
      continue;
    }

    // Costly one-shot mode: does the first plaque celebrate a loss?
    const first = t.report.rows.find((r) => r.plaque && r.atLeast > 0);
    if (!first) continue;
    const breakeven = s?.breakevenRate ?? 0;
    if (first.threshold < cost) {
      const losing = first.atLeast > 0 ? Math.max(0, first.atLeast - breakeven) / first.atLeast : 0;
      out.push({
        rule: 'plaque-loss',
        severity: 'serious',
        category: 'Celebrations',
        mode,
        title: `${first.name} plaque celebrates results below the ${fx(cost)} the spin cost`,
        big: fx(first.threshold),
        ref: `cost ${fx(cost)}`,
        excess: cost / first.threshold,
        text: `${pct(losing)} of the ${first.name} plaques are for a result under the cost.`,
        why: 'A celebration only makes sense against what the player staked on THIS spin (client feedback, skullperch DEGEN SPIN).',
        target,
      });
    } else {
      out.push({
        rule: 'plaque-rhythm',
        severity: 'context',
        category: 'Celebrations',
        mode,
        title: `${first.name} plaque on 1 spin in ${first.atLeastOneIn.toFixed(1)}`,
        big: `${(first.threshold / cost).toFixed(1)}× cost`,
        ref: 'first plaque',
        excess: 1,
        text: `From ${fx(first.threshold)}, i.e. ${(first.threshold / cost).toFixed(1)}× the ${fx(cost)} the spin cost: frequent, but never for a loss.`,
        why: 'On a costly one-shot mode every spin is an event: the plaque rhythm is the mode, not a fault.',
        target,
      });
    }
  }
  if (guessed.length) {
    out.push({
      rule: 'plaque-unknown',
      severity: 'context',
      category: 'Celebrations',
      mode: null,
      title: 'Celebrations not checked: the game has no winLevelThresholds.ts',
      big: `${guessed.length} modes`,
      ref: '',
      excess: 0,
      text: `Win tiers shows SlotLab's default ladder for ${guessed.join(', ')} — not what the game displays.`,
      why: 'Celebration findings are only meaningful against the ladder the game really uses.',
      target: { view: 'tiers' },
    });
  }
}

// ── Pool: how many different books stand behind each part of a mode ─────────

function pool({ pops, stats }: FindingsInput, out: Issue[]) {
  const gameMax = Math.max(0, ...stats.map((m) => m.stats.maxPayout));
  for (const mp of pops) {
    if (!mp.imported) continue;
    const d = (key: string) => ({ mode: mp.mode, category: 'Sample' as const, target: { view: 'explore' as const, mode: mp.mode, key } });

    // 1. Books per bonus tier, per plain-spin feature.
    const parts: { name: string; key: string; books: number }[] = [];
    for (const p of mp.populations) if (p.kind === 'tier') parts.push({ name: p.label, key: p.key, books: p.drawableBooks });
    // Plain-spin features: criteria populations are already FAMILIES (`slotlab-core/src/family.rs`).
    for (const p of mp.populations) {
      if (p.kind !== 'criteria' || !p.criteria || p.retriggers || p.criteria === 'base game') continue;
      if (p.probability < 0.05 && p.rtpShare >= REF.poolFeatureRtpShare) parts.push({ name: p.label, key: p.key, books: p.drawableBooks });
    }
    for (const part of parts) {
      if (part.books >= REF.poolMin) continue;
      const excess = REF.poolMin / Math.max(part.books, 1);
      out.push({ ...d(part.key), rule: 'pool', severity: graded(excess), title: `${part.name}: too few books`, big: fk(part.books), ref: `≥ ${fk(REF.poolMin)}`, excess, text: `${part.books.toLocaleString('en-US')} drawable books behind ${part.name}.`, why: WHY.pool, merge: `pool:${part.name}`, sig: [part.books] });
    }

    // 2. Books on the max win.
    const all = mp.populations.find((p) => p.kind === 'all');
    const top = all?.topPayouts[0];
    if (top && gameMax > 0 && top.payout >= gameMax && top.books < REF.poolMaxWinMin) {
      const excess = REF.poolMaxWinMin / Math.max(top.books, 1);
      out.push({ ...d('all'), rule: 'pool-max', severity: graded(excess), title: 'Few books on the max win', big: fk(top.books), ref: `≥ ${fk(REF.poolMaxWinMin)}`, excess, text: `${top.books.toLocaleString('en-US')} drawable books pay ${fx(top.payout)}.`, why: WHY.pool, merge: 'pool-max', sig: [top.books] });
    }

    // 3. Inside each bonus tier: holes and thin bands.
    for (const p of mp.populations) {
      if (p.kind !== 'tier') continue;
      const bs = p.bands.filter((b) => b.end !== 0); // the exact-zero band apart
      const maxBand = bs.findIndex((b) => p.max >= b.start && (b.end === null || p.max < b.end));
      const used = bs.map((b, i) => b.books > 0 && i !== maxBand);
      const first = used.indexOf(true);
      const last = Math.max(used.lastIndexOf(true), p.max >= gameMax && maxBand >= 0 ? maxBand : -1);
      const label = (b: Band) => (b.end === null ? `${fk(b.start)}+` : `${fk(b.start)}–${fk(b.end)}`);
      const holes = first < 0 ? [] : bs.filter((b, i) => i > first && i < last && i !== maxBand && b.books === 0).map(label);
      if (holes.length) {
        out.push({ ...d(p.key), rule: 'pool-hole', severity: 'serious', title: `${p.label}: no book in ${holes[0]}${holes.length > 1 ? ` (+${holes.length - 1})` : ''}`, big: '0', ref: 'books', excess: 2 + holes.length, text: `Empty: ${holes.join(', ')} — between populated bands of the same bonus.`, why: WHY.poolBands, merge: `pool-hole:${p.label}`, sig: [holes.length] });
      }
      const memorable = Math.max(REF.memorableFrom, REF.memorableFromCost * mp.cost);
      const thin = bs.filter((b, i) => i !== maxBand && b.books > 0 && b.start >= memorable && b.effective < REF.bandEffectiveMin);
      if (thin.length) {
        const worst = thin.reduce((a, b) => (b.effective < a.effective ? b : a));
        const excess = REF.bandEffectiveMin / Math.max(worst.effective, 1);
        out.push({ ...d(p.key), rule: 'pool-thin', severity: graded(excess), title: `${p.label}: ${fk(Math.round(worst.effective))} effective books in ${label(worst)}`, big: fk(Math.round(worst.effective)), ref: `≥ ${fk(REF.bandEffectiveMin)}`, excess, text: thin.map((b) => `${label(b)}: ${Math.round(b.effective).toLocaleString('en-US')} effective of ${b.books.toLocaleString('en-US')} books`).join(' · '), why: WHY.poolBands, merge: `pool-thin:${p.label}`, sig: [worst.effective] });
      }
    }
  }
}

function data({ pops, popsError }: FindingsInput, out: Issue[]) {
  if (popsError) out.push({ rule: 'pops-error', severity: 'minor', category: 'Data', mode: null, title: 'Bonus analysis unavailable', big: 'error', ref: '', excess: 1, text: popsError, why: 'The populations endpoint failed.' });
  for (const mp of pops) {
    const u = mp.populations.find((p) => p.kind === 'tier' && p.tier === null);
    if (u) out.push({ rule: 'untiered', severity: 'context', category: 'Data', mode: mp.mode, title: 'Bonuses without a known tier', big: pct(u.shareOfBonuses ?? 0), ref: 'of the bonuses', excess: 1, text: 'Their freeSpinTrigger does not list positions.', why: 'SlotLab cannot tell how many symbols opened them.', target: { view: 'explore', mode: mp.mode, key: u.key } });
    if (!mp.imported) out.push({ rule: 'not-imported', severity: 'context', category: 'Data', mode: mp.mode, title: 'Books still importing', big: '', ref: '', excess: 0, text: 'Bonus findings appear once the import ends.', why: '' });
  }
}
