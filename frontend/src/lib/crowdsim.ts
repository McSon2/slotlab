/**
 * CrowdSim — types of `/api/ui/crowdsim` (cf. `slotlab-core/src/crowdsim.rs`)
 * and the automatic reading of a run: what the players lived, and what in it
 * deserves a look. Same shape as Analyze's « Worth a look »: one short
 * sentence, one number, its reference; severity only where a fixed reference
 * exists, the rest is what happened (« lived »).
 */

import type { Population } from './populations';
import { fx, pct, threshold } from './populations';
import { fmtPct } from './api';
import type { FeelReport, FixRef } from './feel';
import { FRUSTRATION_TITLE, cohortLabel, formatFrustration, frustrationSentence, leverNow, observedLine } from './feel';

/** RTP figures: 2 decimals everywhere in the app ("96.50 %"). */
const rtpPct = (v: number) => fmtPct(v, 2);

const tokenCache = new Map<string, string>();

/**
 * A design token (`--series-2`, `--pass`…) as an `rgba()` string, for
 * Chart.js: the canvas can't read CSS variables and Chart.js' own color
 * parser doesn't know oklch(). Resolved once through a 1×1 canvas.
 */
export function tokenColor(name: string, alpha = 1): string {
  const key = `${name}|${alpha}`;
  const hit = tokenCache.get(key);
  if (hit) return hit;
  let out = `rgba(150, 160, 150, ${alpha})`;
  try {
    const raw = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    const ctx = document.createElement('canvas').getContext('2d', { willReadFrequently: true });
    if (raw && ctx) {
      const sentinel = '#010203';
      ctx.fillStyle = sentinel;
      ctx.fillStyle = raw;
      if (ctx.fillStyle !== sentinel) {
        ctx.fillRect(0, 0, 1, 1);
        const [r, g, b] = ctx.getImageData(0, 0, 1, 1).data;
        out = `rgba(${r}, ${g}, ${b}, ${alpha})`;
      }
    }
  } catch {
    // Keep the neutral fallback.
  }
  tokenCache.set(key, out);
  return out;
}

export interface SimMetric {
  key: string;
  label: string;
  unit: 'bets' | 'x' | 'ratio' | 'spins' | 'count';
  players: number;
  mean: number;
  min: number;
  p5: number;
  p25: number;
  p50: number;
  p75: number;
  p95: number;
  max: number;
}

export interface SimShare {
  key: string;
  label: string;
  value: number;
}

export interface RareCheck {
  atLeast: number;
  expected: number;
  observed: number;
}

export interface ModeObserved {
  mode: string;
  cost: number;
  imported: boolean;
  roundShare: number;
  rounds: number;
  wagered: number;
  won: number;
  rtp: number;
  theoreticalRtp: number;
  populations: Population[];
  rare: RareCheck[];
}

export interface Curves {
  checkpoints: number[];
  netP5: number[];
  netP25: number[];
  netP50: number[];
  netP75: number[];
  netP95: number[];
  netMean: number[];
  rtpP5: number[];
  rtpP50: number[];
  rtpP95: number[];
  pop: number[];
}

export interface Cohort {
  name: string;
  mix: [string, number][];
  players: number;
  spins: number;
  bankroll: number | null;
  rounds: number;
  wagered: number;
  won: number;
  rtp: number;
  theoreticalRtp: number;
  rtpStdError: number;
  hasBonus: boolean;
  bonuses: number;
  maxWins: number;
  metrics: SimMetric[];
  shares: SimShare[];
  bonusHist: number[];
  curves: Curves;
  modes: ModeObserved[];
  /** Player feel of a single base-like mode (absent for buys and mixes). */
  feel: FeelReport | null;
  /** The cohort plays a fixed version of a mode. */
  fix: FixRef | null;
  elapsedMs: number;
}

export interface SimResponse {
  game: string;
  players: number;
  spins: number;
  scenarios: Cohort[];
}

export const metricOf = (c: Cohort, key: string) => c.metrics.find((m) => m.key === key);
export const shareOf = (c: Cohort, key: string) => c.shares.find((s) => s.key === key)?.value;

/** Average cost of a round of the cohort. */
export const avgCost = (c: Cohort) => (c.rounds > 0 ? c.wagered / c.rounds : 1);

/** Every round opens a bonus: a buy. */
export const isBuy = (c: Cohort) => c.hasBonus && c.bonuses >= c.rounds * 0.999;

/** Observed population of the cohort's main mode (largest share of rounds). */
export function mainMode(c: Cohort): ModeObserved | undefined {
  return [...c.modes].sort((a, b) => b.rounds - a.rounds)[0];
}

export function formatValue(v: number, unit: SimMetric['unit']): string {
  if (!isFinite(v)) return '—';
  switch (unit) {
    case 'ratio':
      return pct(v);
    case 'x':
      return fx(v);
    case 'bets':
      return (v > 0 ? '+' : '') + Math.round(v).toLocaleString('en-US');
    case 'spins':
    case 'count':
      return Math.round(v).toLocaleString('en-US');
  }
}

// ── Reading a run ────────────────────────────────────────────────────────────

export type SimSeverity = 'serious' | 'minor' | 'lived';

export interface SimFinding {
  severity: SimSeverity;
  title: string;
  big: string;
  ref: string;
  text: string;
  why: string;
}

/**
 * SlotLab thresholds for the felt experience of a base-like mode. They are
 * judgement calls, not Stake rules — one place to tune them.
 */
export const SIM_REF = {
  /** Players who never see the feature over the session. */
  noBonusMinor: 0.25,
  noBonusSerious: 0.5,
  /** Bonuses paying under 10x the bet: felt as a disappointment. */
  weakBonusMinor: 0.3,
  weakBonusSerious: 0.5,
  /** Median player's longest run without any win, in spins. */
  drySpellMinor: 40,
  drySpellSerious: 80,
  /** Standard errors between empirical and theoretical RTP before suspecting the engine. */
  rtpZ: 4,
};

const n0 = (v: number) => Math.round(v).toLocaleString('en-US');

export function readCohort(c: Cohort): SimFinding[] {
  const out: SimFinding[] = [];
  const cost = avgCost(c);
  const buy = isBuy(c);
  const baseLike = !buy && cost < 10;
  const m = mainMode(c);
  const rtp = metricOf(c, 'rtp');
  const session = `${n0(c.spins)} spins`;

  // RTP vs the math: luck of the sample, or a real discrepancy.
  const gap = c.rtp - c.theoreticalRtp;
  const z = c.rtpStdError > 0 ? gap / c.rtpStdError : 0;
  const rare = m?.rare.find((r) => r.atLeast >= 1000) ?? m?.rare[m.rare.length - 1];
  if (Math.abs(z) > SIM_REF.rtpZ) {
    out.push({
      severity: 'serious',
      title: 'Empirical RTP outside the sampling noise',
      big: rtpPct(c.rtp),
      ref: `math ${rtpPct(c.theoreticalRtp)} ± ${rtpPct(2 * c.rtpStdError)}`,
      text: `${Math.abs(z).toFixed(1)} standard errors away — more than luck can explain at ${n0(c.rounds)} rounds.`,
      why: 'Check that the lookup table and the books served are the ones analysed.',
    });
  } else {
    out.push({
      severity: 'lived',
      title: `Overall RTP ${gap >= 0 ? 'above' : 'below'} the math by luck of the sample`,
      big: rtpPct(c.rtp),
      ref: `math ${rtpPct(c.theoreticalRtp)} ± ${rtpPct(2 * c.rtpStdError)}`,
      text:
        `Within the expected noise at ${n0(c.rounds)} rounds.` +
        (rare ? ` Wins ≥ ${fx(rare.atLeast)}: ${n0(rare.observed)} seen for ${rare.expected.toFixed(1)} expected.` : ''),
      why: 'The very big wins are rare: a sample reads above or below target depending on how many it catches.',
    });
  }

  // What the median player lives.
  if (rtp) {
    out.push({
      severity: 'lived',
      title: `The median player gets back ${rtpPct(rtp.p50)} of what they bet`,
      big: rtpPct(rtp.p50),
      ref: `P5 ${rtpPct(rtp.p5)} · P95 ${rtpPct(rtp.p95)}`,
      text: `${pct(shareOf(c, 'profit') ?? 0)} finish in profit after ${session}; ${pct(shareOf(c, 'lostHalf') ?? 0)} lose more than half of what they bet.`,
      why: 'Session RTP = what a player won ÷ what they bet over the session.',
    });
  }

  const mixed = c.mix.length > 1;
  // Player feel carries the severity of droughts, letdowns and dry spells
  // (exact figures, and a fix): the older readings stay as context.
  const feel = c.feel;
  const felt = (s: SimSeverity): SimSeverity => (feel ? 'lived' : s);
  if (feel) {
    for (const f of feel.frustrations.filter((x) => x.severity !== 'ok')) {
      out.push({
        severity: f.severity === 'serious' ? 'serious' : 'minor',
        title: `${FRUSTRATION_TITLE[f.key]}: ${frustrationSentence(feel, f)}`,
        big: formatFrustration(f.key, f.value),
        ref: f.band ? `market ${f.band[0].toFixed(2)}–${f.band[1].toFixed(2)}` : `≤ ${formatFrustration(f.key, f.minor)}`,
        text: `${observedLine(feel, f)}${observedLine(feel, f) ? '. ' : ''}Today: ${leverNow(feel, f.key)}.`,
        why: f.fix ? 'Player feel shows how to fix it, and what each fix costs.' : 'No fix within reach of these books.',
      });
    }
  }
  if (baseLike && c.hasBonus) {
    const none = shareOf(c, 'noBonus') ?? 0;
    const first = metricOf(c, 'firstBonus');
    const bonuses = metricOf(c, 'bonuses');
    out.push({
      severity: felt(mixed ? 'lived' : none >= SIM_REF.noBonusSerious ? 'serious' : none >= SIM_REF.noBonusMinor ? 'minor' : 'lived'),
      title: mixed
        ? `${bonuses ? bonuses.p50.toFixed(0) : 0} bonuses per player, bought ones included`
        : none > 0.005
          ? `${pct(none)} of the players never trigger a bonus`
          : 'Every player triggers the bonus',
      big: bonuses ? `${bonuses.p50.toFixed(0)} per player` : '',
      ref: mixed ? '' : `≤ ${pct(SIM_REF.noBonusMinor)} without`,
      text: `Over ${session}${first ? `; the first one comes after ${n0(first.p50)} spins for the median player (P95 ${n0(first.p95)})` : ''}.`,
      why: 'A player who never sees the feature never sees what the game is about. SlotLab threshold — depends on the session length you simulate.',
    });

    const bonusPop = m?.populations.find((p) => p.kind === 'bonus') ?? m?.populations.find((p) => p.kind === 'tier');
    const u10 = bonusPop ? threshold(bonusPop, 10) : undefined;
    if (bonusPop && u10) {
      out.push({
        severity: felt(u10.below >= SIM_REF.weakBonusSerious ? 'serious' : u10.below >= SIM_REF.weakBonusMinor ? 'minor' : 'lived'),
        title: `${pct(u10.below)} of the ${mixed ? `${m?.mode} ` : ''}bonuses they saw paid under 10x`,
        big: `median ${fx(bonusPop.median)}`,
        ref: `avg ${fx(bonusPop.mean)} · ≤ ${pct(SIM_REF.weakBonusMinor)} under 10x`,
        text: `${n0(bonusPop.probability * (m?.rounds ?? 0))} bonuses observed; best bonus of the median player ${fx(metricOf(c, 'bestBonus')?.p50 ?? 0)}.`,
        why: 'A bonus under 10x the bet is felt as a let-down, whatever its average. SlotLab threshold.',
      });
    }
  }

  if (baseLike) {
    const dry = metricOf(c, 'dead');
    if (dry) {
      out.push({
        severity: felt(dry.p50 >= SIM_REF.drySpellSerious ? 'serious' : dry.p50 >= SIM_REF.drySpellMinor ? 'minor' : 'lived'),
        title: `Longest run without any win: ${n0(dry.p50)} spins for the median player`,
        big: `${n0(dry.p50)} spins`,
        ref: `P95 ${n0(dry.p95)} · ≤ ${SIM_REF.drySpellMinor}`,
        text: `Longest run paying under the bet: ${n0(metricOf(c, 'losing')?.p50 ?? 0)} spins (median player).`,
        why: 'Dry spells are what a player feels between two wins. SlotLab threshold.',
      });
    }
  }

  // Big wins seen.
  const best = metricOf(c, 'best');
  if (best) {
    const never100 = shareOf(c, 'never100') ?? 0;
    out.push({
      severity: 'lived',
      title: `Best win of the median player: ${fx(best.p50)}`,
      big: fx(best.p50),
      ref: `P95 ${fx(best.p95)}`,
      text: `${pct(never100)} never win 100x · ${pct(shareOf(c, 'never1000') ?? 0)} never win 1000x · max win hit by ${pct(shareOf(c, 'maxWin') ?? 0)} (${n0(c.maxWins)} times).`,
      why: 'In multiples of the base bet.',
    });
  }

  if (buy && m) {
    const all = m.populations.find((p) => p.kind === 'all');
    if (all) {
      out.push({
        severity: 'lived',
        title: `A typical purchase returns ${(all.median / m.cost).toFixed(2)}× its cost`,
        big: fx(all.median),
        ref: `for ${fx(m.cost)}`,
        text: `${pct(all.profitableRate)} of the purchases return at least their cost · worst ${fx(all.min)} · best ${fx(all.max)}.`,
        why: 'Market reference (Waylanders Forge): a median of ≈ 0.25× the cost on a 100x buy.',
      });
    }
  } else if (!baseLike && m) {
    const all = m.populations.find((p) => p.kind === 'all');
    if (all) {
      out.push({
        severity: 'lived',
        title: `${pct(all.zeroRate)} of the ${fx(m.cost)} spins pay nothing`,
        big: fx(all.median),
        ref: 'median spin',
        text: `${pct(all.profitableRate)} return at least the cost · best ${fx(all.max)}.`,
        why: 'A costly one-shot mode: each spin is an event.',
      });
    }
  }

  const busted = shareOf(c, 'busted');
  if (busted !== undefined) {
    const rounds = metricOf(c, 'rounds');
    out.push({
      severity: busted >= 0.5 ? 'minor' : 'lived',
      title: `${pct(busted)} run out of balance`,
      big: rounds ? `${n0(rounds.p50)} spins` : '',
      ref: `median played, bankroll ${n0(c.bankroll ?? 0)}`,
      text: `Starting balance ${n0(c.bankroll ?? 0)} bets.`,
      why: 'A player who busts early leaves before the game has shown itself.',
    });
  }

  if (c.mix.length > 1) {
    const parts = c.modes.map((mo) => `${mo.mode} ${pct(mo.roundShare)} of the rounds, ${pct(mo.wagered / c.wagered)} of the bets`);
    out.push({ severity: 'lived', title: 'Mixed session', big: `${c.mix.length} modes`, ref: '', text: parts.join(' · ') + '.', why: '' });
  }

  const rank: Record<SimSeverity, number> = { serious: 0, minor: 1, lived: 2 };
  return out.sort((a, b) => rank[a.severity] - rank[b.severity]);
}

/** Cohorts side by side: a few sentences only when there are several. */
export function compareCohorts(cs: Cohort[]): string[] {
  if (cs.length < 2) return [];
  const out: string[] = [];
  const per1000 = (c: Cohort) => (c.wagered > 0 ? (c.bonuses / c.wagered) * 1000 : 0);
  const withBonus = cs.filter((c) => c.hasBonus && !isBuy(c));
  if (withBonus.length >= 2) {
    out.push('Bonuses per 1 000 bets wagered: ' + withBonus.map((c) => `${cohortLabel(c.name, c.fix)} ${per1000(c).toFixed(1)}`).join(' · ') + '.');
  }
  return out;
}

// ── Key figures ──────────────────────────────────────────────────────────────

export interface Kpi {
  label: string;
  value: string;
  sub: string;
}

/** The handful of numbers that say how a cohort went — adapted to the kind of mode. */
export function kpis(c: Cohort): Kpi[] {
  const rtp = metricOf(c, 'rtp');
  const best = metricOf(c, 'best');
  const net = metricOf(c, 'net');
  const out: Kpi[] = [
    { label: 'RTP', value: rtpPct(c.rtp), sub: `math ${rtpPct(c.theoreticalRtp)}` },
    { label: 'Players in profit', value: pct(shareOf(c, 'profit') ?? 0), sub: `after ${n0(c.spins)} spins` },
    { label: 'Median session RTP', value: rtpPct(rtp?.p50 ?? 0), sub: `P5 ${rtpPct(rtp?.p5 ?? 0)} · P95 ${rtpPct(rtp?.p95 ?? 0)}` },
    { label: 'Median net', value: `${Math.round(net?.p50 ?? 0).toLocaleString('en-US')}`, sub: 'bets' },
    { label: 'Best win', value: fx(best?.p50 ?? 0), sub: `median player · P95 ${fx(best?.p95 ?? 0)}` },
  ];
  if (isBuy(c)) {
    const all = mainMode(c)?.populations.find((p) => p.kind === 'all');
    if (all) out.push({ label: 'Typical purchase', value: fx(all.median), sub: `${(all.median / (mainMode(c)?.cost ?? 1)).toFixed(2)}× the cost` });
  } else if (c.hasBonus) {
    const b = metricOf(c, 'bonuses');
    out.push({ label: 'Bonuses per player', value: String(Math.round(b?.p50 ?? 0)), sub: `${pct(shareOf(c, 'noBonus') ?? 0)} never see one` });
  }
  return out;
}
