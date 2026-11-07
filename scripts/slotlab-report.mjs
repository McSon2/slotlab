function fmtPct(v, digits = 2) {
  if (!isFinite(v)) return "—";
  return (v * 100).toFixed(digits) + " %";
}
function fx(v) {
  if (!isFinite(v)) return "—";
  const a = Math.abs(v);
  const d = a === 0 || Number.isInteger(v) && a >= 1 ? 0 : a < 10 ? 2 : a < 100 ? 1 : 0;
  return v.toLocaleString("en-US", { minimumFractionDigits: d, maximumFractionDigits: d }) + "x";
}
function pct(v) {
  if (!isFinite(v)) return "—";
  const p = v * 100;
  if (p === 0) return "0 %";
  const a = Math.abs(p);
  const d = a >= 10 ? 1 : a >= 1 ? 1 : a >= 0.1 ? 2 : a >= 0.01 ? 3 : 4;
  return p.toFixed(d) + " %";
}
function oneIn(v) {
  if (!isFinite(v)) return "never";
  if (v < 10) return "1 / " + v.toFixed(2);
  return "1 / " + Math.round(v).toLocaleString("en-US");
}
function threshold(p, x) {
  return p.thresholds.find((t) => Math.abs(t.x - x) < 1e-9);
}
function quantile(p, q2) {
  return p.quantiles.find((v) => Math.abs(v.q - q2) < 1e-9);
}
function cdfBelow(p, x) {
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
const n0$1 = (v) => Math.round(v).toLocaleString("en-US");
function odds(p) {
  if (!(p > 0)) return "never";
  const v = 1 / p;
  if (v >= 1e6) return `1 in ${(v / 1e6).toPrecision(3)}M`;
  if (v >= 1e5) return `1 in ${Math.round(v / 1e3)}k`;
  return `1 in ${n0$1(v)}`;
}
const stakeWord = (m) => m.kind === "buy" || m.kind === "bust" ? "the price" : "the stake";
const times$1 = (v) => `${v < 10 ? v.toFixed(2) : n0$1(v)}×`;
const FIGURES = {
  rtp: { label: () => "RTP", format: (v) => fmtPct(v, 2), scale: "linear" },
  hit: { label: () => "Hit rate", format: pct, scale: "linear" },
  zero: { label: () => "Rounds paying nothing", format: pct, scale: "linear" },
  winsUnderStake: { label: () => "Wins under the stake (of the wins)", format: pct, scale: "linear" },
  p1: { label: (m) => m.kind === "buy" || m.kind === "bust" ? "Pays the price back" : "Wins ≥ the stake", format: pct, scale: "linear" },
  p2: { label: (m) => `≥ 2× ${stakeWord(m)}`, format: pct, scale: "linear" },
  p5: { label: (m) => `≥ 5× ${stakeWord(m)}`, format: odds, scale: "log" },
  p10: { label: (m) => `≥ 10× ${stakeWord(m)}`, format: odds, scale: "log" },
  p50: { label: (m) => `≥ 50× ${stakeWord(m)}`, format: odds, scale: "log" },
  p100: { label: (m) => `≥ 100× ${stakeWord(m)}`, format: odds, scale: "log" },
  p500: { label: (m) => `≥ 500× ${stakeWord(m)}`, format: odds, scale: "log" },
  p1000: { label: (m) => `≥ 1,000× ${stakeWord(m)}`, format: odds, scale: "log" },
  p5000: { label: (m) => `≥ 5,000× ${stakeWord(m)}`, format: odds, scale: "log" },
  p10000: { label: (m) => `≥ 10,000× ${stakeWord(m)}`, format: odds, scale: "log" },
  pmax: { label: (m) => `Max win (${fx(m.max)})`, format: odds, scale: "log" },
  never500: { label: (m) => `6,000 ${m.kind === "ante" ? "rounds" : "spins"} without 500× ${stakeWord(m)}`, format: pct, scale: "linear" },
  never1000: { label: (m) => `6,000 ${m.kind === "ante" ? "rounds" : "spins"} without 1,000× ${stakeWord(m)}`, format: pct, scale: "linear" },
  median: { label: (m) => `Median return (× ${stakeWord(m)})`, format: times$1, scale: "linear" },
  q25: { label: () => "Return, p25", format: times$1, scale: "linear" },
  q75: { label: () => "Return, p75", format: times$1, scale: "linear" },
  q90: { label: () => "Return, p90", format: times$1, scale: "linear" },
  crumbs: { label: (m) => `Under 25 % of ${stakeWord(m)}`, format: pct, scale: "linear" },
  nearZero: { label: (m) => `Under 10 % of ${stakeWord(m)}`, format: pct, scale: "linear" },
  neverRefunded: { label: () => "5 purchases in a row, none paying the price back", format: pct, scale: "linear" },
  vs10: { label: (m) => `RTP carried by wins ≥ 10× ${stakeWord(m)}`, format: pct, scale: "linear" },
  vs100: { label: (m) => `RTP carried by wins ≥ 100× ${stakeWord(m)}`, format: pct, scale: "linear" },
  vs1000: { label: (m) => `RTP carried by wins ≥ 1,000× ${stakeWord(m)}`, format: pct, scale: "linear" },
  vol: { label: (m) => `Volatility (std ÷ ${m.kind === "buy" || m.kind === "bust" ? "price" : "stake"})`, format: (v) => v.toFixed(1), scale: "linear" },
  top1: { label: () => "Value carried by the top 1 % of rounds", format: pct, scale: "linear" },
  s100Median: { label: (m) => `100 ${m.kind === "ante" ? "rounds" : "spins"}: median return (of 100 ${stakeWord(m)}s)`, format: (v) => `${v.toFixed(0)}`, scale: "linear" },
  s100Win: { label: (m) => `100 ${m.kind === "ante" ? "rounds" : "spins"}: sessions ending ahead`, format: pct, scale: "linear" },
  s100Bust: { label: (m) => `100 ${m.kind === "ante" ? "rounds" : "spins"}: sessions giving back under 20`, format: pct, scale: "linear" }
};
const inMiddleRow = (r, v = r.value) => v >= Math.min(r.p25, r.p75) && v <= Math.max(r.p25, r.p75);
function position(r, spec) {
  if (inMiddleRow(r)) return "middle half";
  const above = r.rank >= 0.5;
  const share = Math.round((above ? r.rank : 1 - r.rank) * 100);
  if (spec.scale === "log") return above ? `more often than ${share} %` : `rarer than ${share} %`;
  return above ? `higher than ${share} %` : `lower than ${share} %`;
}
const SEVERITIES = ["blocking", "serious", "minor", "context"];
const REF = {
  stepMid: 4,
  stepLast: 8,
  mostLikelyMin: 1e3,
  hitMin: 0.1,
  std3Star: 60,
  meanOverMedian: 3,
  top1: 0.4,
  essMin: 50,
  buyMedianOverCost: 0.25,
  buyProfitableMin: 0.1,
  sameTierMedianGap: 1.3,
  subBetWins: 0.6,
  plaqueEvery: 10,
  /** CVaR (mean of the worst 0.1 %, × base bet) ceiling of the 3★ grid — studio measurement. */
  cvar3Star: 5e4,
  /** A win ≥ 100x WITHOUT a bonus at most 1 in this many spins, per 1x of cost (studio target, Toxic Spins 2026-09-23). */
  outsideBonus100OneIn: 8e3,
  /** The target is a studio aim, not a Stake limit: only flag once this far past it (Toxic BASE at ×1.10 is fine). */
  outsideBonusTolerance: 1.25,
  /** At most this share of the ≥ 1,000x / ≥ 5,000x wins may come without a bonus. */
  outsideBonusTopShare: 0.3,
  /** At most this share of a mode's ≥ 100x (or ≥ 1,000x) wins may land on a round multiplier (chance: ~0.1 %). */
  roundShare: 0.02,
  /** Studio rule (Toxic Spins, 2026-10-04): drawable books per bonus tier, and per plain-spin feature carrying ≥ `poolFeatureRtpShare` of the RTP. */
  poolMin: 1e5,
  poolFeatureRtpShare: 0.05,
  /** Drawable books on the mode's max win (all identical in payout, not in board). */
  poolMaxWinMin: 1e4,
  /** A MEMORABLE band of a bonus tier (≥ `memorableFrom`, or ≥ `memorableFromCost` × the price of a buy) needs this many EFFECTIVE books (studio, 2026-10-04). */
  bandEffectiveMin: 1e4,
  /** Lifetime volume of a hit game, in spins of a 1x mode (÷ the mode's cost for the others): a band only matters if the same round would come back within it. */
  lifetimeSpins: 1e9,
  memorableFrom: 1e3,
  memorableFromCost: 10,
  /** Studio targets for a BASE game (LAST LAUGH 2026-10-01, TOXIC SPINS 2026-10-03, WILD CURRENT 2026-10-05): a bonus, any tier, at most 1 spin in `anyBonusMax` (aim 1/150–180); the first tier at most 1 in `firstTierMax` (aim 1/180–200, « même 1/240 »); the hidden (top tier of three or more) at most 1 in `hiddenMax` (« 1/20 000 max, c'est déjà très rare »). */
  anyBonusMax: 200,
  firstTierMax: 250,
  hiddenMax: 2e4,
  /** Aims, not limits — context up to the tolerance, a fault past it: any bonus 1/240 accepted (LAST LAUGH), 1/245 refused (WILD CURRENT v1); a first tier at 1/300 accepted on a low-volatility slot whose bonuses pay well (BUBBLE HEX), 1/350 refused (LAST LAUGH v2); the hidden at 1/22 400 on a 100 000x game is fine. */
  bonusRareTolerance: 1.2,
  firstTierTolerance: 1.5,
  hiddenTolerance: 1.25,
  /** A max win landing at least this often on a costly mode is a design (TOXIC SPINS 2026-10-03: DEGEN / HIDDEN / MYSTERY « en dessous de 1/3 000, je sais que ça ne correspond pas à la compliance, c'est pas grave »): the top of the curve and the CVaR are what the mode is. */
  accessibleMaxWin: 5e3,
  /** A step past ×4 by less than this is the grid, not a hole (TOXIC SPINS MYSTERY ×4.8 at 5k–10k, published). */
  stepTolerance: 1.25,
  /** Effective books of a memorable band: the 10 000 aim is a round figure — a band at two thirds of it is close (TOXIC SPINS published at 7.2k and 9.9k). */
  bandEffectiveTolerance: 1.5,
  /** Consecutive tiers: the higher at most this share as frequent as the lower (GREEDY GOBLIN 2026-10-02: a super at 1/24 next to a bonus at 1/20 — « presque autant » — refused; the base game keeps ×6). */
  tierCloseness: 1 / 3,
  /** Bonuses that look alike: P75 ÷ P25 of a bonus or a buy under this (BUBBLE HEX 2026-09-29: half the bonuses between 61x and 76x, ×1.25 — « toujours la même chose »; its published ×1.57 accepted). */
  flatSpread: 1.4,
  /** A triggered bonus ending under a tenth of its average is a dead one (TOXIC SPINS 2026-09-27: RAIDs at 2–6x made rare); market buys: 4–24 % under a tenth of the price. */
  deadBonusShare: 0.25,
  /** Bonuses carrying over this share of a base game's RTP leave a thin base game (WILD CURRENT 2026-10-05: 50/50 wanted at 30 000x; BUBBLE HEX: 22/78 questioned). */
  bonusRtpShareMax: 0.75,
  /** Hit rate of a base game by max win (studio, 2026-10-05): 5 000x → 32 %, 30 000x → 27 %, 100 000x → 19.5 % — the lower the ceiling, the more the RTP feeds small wins. */
  hitByMax: [
    [5e3, 0.32],
    [3e4, 0.27],
    [1e5, 0.195]
  ],
  hitTolerance: 0.8,
  /** An ante's max win: same share of the RTP as the base game ⇒ P(max) × its cost; ×/÷ 2 tolerated (TOXIC SPINS 2026-09-27: 1/13M vs 1/2M, « pourquoi un tel écart »). */
  anteMaxWinTolerance: 2,
  /** Market: a figure beyond this rank on either side (under p10, over p90) is a fault; inside the middle half nothing; between, context. */
  marketOutRank: 0.1,
  /** Paying symbols on screen (UNDEAD KING 2026-10-06): high ÷ (low + high) aims at 50 %, base game and free spins apart; nothing within ± `highShareTolerance`, minor past it, serious from twice as far. */
  highShare: 0.5,
  highShareTolerance: 0.05
};
const WHY = {
  steps: `Probability ratio between two consecutive 1-2-5 bands. Studio reference (skullperch BASE, judged good): ≤ ×${REF.stepMid} mid-curve, ≤ ×${REF.stepLast} into the max win. Above that, the curve reads as a hole.`,
  polarTop: "The max-win band should be rarer than the band just below it; otherwise the top of the mode pays the body or the max win, little in between.",
  gap: "No drawable result in a whole band between two populated ones. Stake checks « no significant gaps in payout ranges ».",
  mostLikely: `Stake requires the most likely simulation to land at most 1 in ${REF.mostLikelyMin}.`,
  cvar: `CVaR = average of the worst 0.1 % of outcomes for the operator, in × base bet. The 3★ grid caps it at ${REF.cvar3Star.toLocaleString("en-US")} (studio measurement on skullperch; SlotLab estimate per mode — Stake aggregates it at game level). It only binds when the max win is at least that high: a frequent max win is not a fault in itself (market degen reference: 1 / 496 at 30 000x, Jelly Siege).`,
  ladder: "Studio rule (blocking): more trigger symbols must be rarer AND pay more.",
  skew: `Average ≥ ${REF.meanOverMedian}× the median, or the top 1 % carrying ≥ ${REF.top1 * 100} % of the value: most players get far less than the average suggests.`,
  polar: "Half the results sit within 25 % of the minimum while the average is at least twice the median: the floor or a big win, little in between.",
  buy: "Market reference (Waylanders Forge): the median purchase returns ≈ 0.25× its cost on a 100x buy, 0.5× on a 1000x one.",
  celebrations: "From the Win tiers ladder: a plaque that lands every few spins is the rhythm of the mode, not an event.",
  outsideBonus: `Studio rule (2026-09-23, after Blood Water): the big wins belong to the bonuses. A ≥ 100x without a bonus at most 1 in ${REF.outsideBonus100OneIn.toLocaleString("en-US")} spins per 1x of cost (Blood Water, live: 1 / 2,997 — judged too many), and at most ${REF.outsideBonusTopShare * 100} % of the ≥ 1,000x / ≥ 5,000x wins without a bonus (Blood Water: 67-77 % above 5,000x). A huge win on a plain spin reads as artificial and steals the show from the bonus.`,
  round: `Studio rule: no spin paying x100 / x1,000 / x2,000 / x5,000 on the nose — a round multiplier reads as fake. An arbitrary win lands on a multiple of 100x ~0.1 % of the time, of 1,000x ~0.01 %. Above ${REF.roundShare * 100} % of the mode's big wins, a « one board × one multiplier » feature (wild multiplier, multi spin, Degen) or full screens make them. Mode max win excluded. Fix on the LUT: penalise round totals inside each band.`,
  pool: `Studio rule (2026-10-04, Toxic Spins: 34,000 books behind the 5-scatter bonus of the base game): at least ${1e5.toLocaleString("en-US")} drawable books per bonus tier and per plain-spin feature (multi spin…), at least ${1e4.toLocaleString("en-US")} on the max win, the rest for the base game. The lookup table sets the frequencies; the pool sets how many DIFFERENT rounds a player can see. Fix: re-allocate the criteria quotas (same total), rebuild the lookup table frozen — the distribution must not move.`,
  poolBands: "Studio rule (2026-10-04): every band of a bonus from 1,000x up (from 10× the price of a buy) stands on at least 10,000 EFFECTIVE books ((Σw)² / Σw²) — those are the wins people film and remember; across all players the same round comes back after about √(π/2 · effective books) hits, so a band only counts if that happens within a hit game’s life (1 billion spins ÷ the mode’s cost) (Toxic Spins HIDDEN 50k-100k: 271 effective for 1 hit in 4,181 spins). Below, no minimum: a 30x bonus is not remembered. An empty band between two populated ones (often just under the max win) is a hole in that bonus. Fix: a targeted criteria (same bonus, win kept inside the band) for the missing books, the lookup table frozen; few effective books among many books = the table piles the weight on a few of them.",
  ess: `Fewer than ${REF.essMin} effective outcomes ((Σw)² / Σw²): players will see the same results again and again.`,
  bonusRare: `Studio targets for a base game (LAST LAUGH, TOXIC SPINS, WILD CURRENT — 2026-10): a bonus, any tier, about 1 spin in 150–180 (« 1/350 c'est beaucoup trop rare », 1/240 accepted); the first tier about 1 in 180–200; the hidden between 1 in 15 000 and 1 in 20 000, never rarer (« 1/30 000 trop rare, 1/20 000 max »). The player must see bonuses often and the hidden must stay reachable.`,
  tierClose: `Studio rule (GREEDY GOBLIN 2026-10-02): tiers must be well separated — a super almost as frequent as the bonus (1/24 vs 1/20) was refused; the base game keeps about ×6 between tiers, and a boosted mode keeps the same factor on every tier.`,
  tierExtra: `GREEDY GOBLIN 2026-10-02: a 6th scatter when the hidden needs 5 « porterait à confusion » — a tier above the top bonus must not exist; cap the board at the top tier (and replay the round that would show one more).`,
  hiddenFloor: `Studio rule (Degen Lab, WILD CURRENT 2026-10-05): the hidden is an event, never a small win — a floor (800x on a 30 000–100 000x game, 500x on 5 000–25 000x) and a median around 1 200x; it must never pay less than a typical bonus of the tier below.`,
  sameTier: `Studio rule (LAST LAUGH 2026-10-01): a bonus tier pays the same — median, average, shape — in every mode it appears in (bought, triggered, mystery). « Si le hidden paie différemment dans chaque mode c'est visible et c'est pas bon. » Only round prices come first (then the gap is a decision, not a fault).`,
  flat: `BUBBLE HEX 2026-09-29 (half the bonuses between 61x and 76x): « on risque d'avoir l'impression de toujours voir la même chose ». A bonus needs spread: the market buys span ×3–5 between their first and third quartile.`,
  deadBonus: `TOXIC SPINS 2026-09-27, after 10 000 spins played: « énormément de bonus qui paient moins de x50 … 2,5x, 3,4x » — a bonus that ends under a tenth of its average is a dead one; they were made rare (under 5x: 17 % → 3 %) without a floor, paid by the small wins of the base game.`,
  rtpSplit: `WILD CURRENT 2026-10-05: « base game et bonus à 50/50 du RTP » on a 30 000x game; BUBBLE HEX: 22 % in the base game judged thin for a low-volatility slot. TOXIC SPINS (100 000x) lives at 36/64, LAST LAUGH at 30/70 — above 75 % the base game is only a wait for the bonus.`,
  baseDry: `WILD CURRENT 2026-10-05 (base at 20 % of paying spins, 80 % dead): « base sans bonus trop sèche … Toxic est une x100k, on est sur une x30k ». The lower the max win, the more the RTP can feed small wins: about 32 % of paying spins at 5 000x (BUBBLE HEX), 27 % at 30 000x, 19–20 % at 100 000x (TOXIC SPINS). Measured on the whole base game, bonuses included.`,
  anteMaxWin: `TOXIC SPINS 2026-09-27 (BASE 1/13M, HUNT 1/2M: « pourquoi un tel écart »): the ante spends the same share of its RTP on the max win as the base game, so its max win is exactly cost × more frequent (HUNT 3x = 3 × the base odds).`,
  symbolMix: `Studio rule (UNDEAD KING 2026-10-06, « la distribution low/high symbole : visé 50/50 »): the paying symbols the player sees — the revealed boards of the books, weighted by the lookup table, base game and free spins apart — split 50 % low / 50 % high. Too few highs and the game feels numb (moon_howl 2026-07-24: 18 % highs in the bonus, 86 % of the winning lines low — a symbol's share of the line wins goes with the CUBE of its density). Low / high: the math-sdk names (H* / L*), or the symbol table of the game's front (math.symbols of its game-config.json).`,
  market: `Studio rule (LAST LAUGH 2026-10-02): « regarde le marché pour tous les modes et essaie d'être dedans » — every figure of a mode inside the middle half (p25–p75) of its peers, the slots the market plays most. Max win on the RARE side: aligned on the peers' median, the max wins looked too frequent (GREEDY GOBLIN). Beyond p10/p90 the mode reads as unlike anything the market plays.`
};
function allFindings(input) {
  const out = [];
  compliance(input, out);
  distribution(input, out);
  for (const mp of input.pops) {
    ladder(mp, out);
    variants(mp, out);
  }
  baseGame(input, out);
  symbolMix(input, out);
  shapes(input, out);
  bigWins(input, out);
  anteTiers(input, out);
  anteMaxWin(input, out);
  acrossModes(input, out);
  celebrations(input, out);
  pool(input, out);
  marketFindings(input, out);
  data(input, out);
  return mergeAcrossModes(out).sort(bySeverity);
}
function expectedHit(maxWin) {
  const pts = REF.hitByMax;
  if (!(maxWin > 0) || maxWin <= pts[0][0]) return pts[0][1];
  if (maxWin >= pts[pts.length - 1][0]) return pts[pts.length - 1][1];
  for (let i = 1; i < pts.length; i++) {
    const [x0, y0] = pts[i - 1];
    const [x1, y1] = pts[i];
    if (maxWin <= x1) {
      const t = (Math.log(maxWin) - Math.log(x0)) / (Math.log(x1) - Math.log(x0));
      return y0 + (y1 - y0) * t;
    }
  }
  return pts[pts.length - 1][1];
}
function spinModes(pops) {
  return pops.filter((mp) => mp.cost < 10 && mp.populations.some((p) => p.kind === "tier") && mp.populations.some((p) => p.kind === "nobonus" && p.mean > 0)).sort((a, b) => a.cost - b.cost);
}
function bySeverity(a, b) {
  return SEVERITIES.indexOf(a.severity) - SEVERITIES.indexOf(b.severity) || b.excess - a.excess;
}
const graded = (excess) => excess >= 2 ? "serious" : "minor";
function issuesOf(issues, mode) {
  return issues.filter((i) => i.mode === mode || mode !== null && (i.also ?? []).includes(mode)).sort(bySeverity);
}
function mergeAcrossModes(issues) {
  const out = [];
  const close = (a = [], b = []) => a.length === b.length && a.every((v, i) => v === 0 && b[i] === 0 || Math.abs(v - b[i]) <= 0.15 * Math.max(Math.abs(v), Math.abs(b[i])));
  for (const it of issues) {
    const host = it.merge ? out.find((o) => o.merge === it.merge && o.severity === it.severity && o.mode !== it.mode && !(o.also ?? []).includes(it.mode ?? "") && close(o.sig, it.sig)) : void 0;
    if (host) host.also = [...host.also ?? [], it.mode ?? ""];
    else out.push({ ...it });
  }
  return out;
}
function fk(v) {
  if (v >= 1e3) {
    const k = v / 1e3;
    return `${Number.isInteger(k) ? k : k.toFixed(1)}k`;
  }
  return Number.isInteger(v) ? `${v}` : v.toFixed(v < 1 ? 2 : 1);
}
const band = (b) => `${fk(b.start)}–${fk(b.end)}`;
const times = (r) => "×" + (r >= 100 ? Math.round(r).toLocaleString("en-US") : r.toFixed(1));
const shortOneIn = (v) => isFinite(v) ? `1 / ${Math.round(v).toLocaleString("en-US")}` : "never";
function compliance({ checks, stats, lenient }, out) {
  if (!checks) return;
  const agreed = lenient ? " (Stake exceptions agreed for this studio)" : "";
  const perModeMostLikely = stats.some((m) => m.stats.mostLikelyOneIn < REF.mostLikelyMin);
  const sev = (s) => s === "fail" ? "blocking" : "minor";
  let crossReported = false;
  for (const m of checks.modes) {
    for (const c of m.checks) {
      if (c.status !== "fail" && c.status !== "warn") continue;
      const game = c.id === "cross_mode_rtp";
      if (game && crossReported) continue;
      crossReported ||= game;
      out.push({
        rule: `check:${c.id}`,
        severity: sev(c.status),
        category: "Compliance",
        mode: game ? null : m.mode,
        title: c.label,
        big: c.value,
        ref: c.threshold,
        excess: 10,
        text: `${c.label}: ${c.value} — needs ${c.threshold}.`,
        why: "Compliance per mode, official Stake report.",
        target: { view: "compliance", mode: game ? void 0 : m.mode }
      });
    }
  }
  for (const c of checks.publishChecks) {
    if (c.status !== "fail" && c.status !== "warn") continue;
    if (c.id === "most_likely_hr" && perModeMostLikely) continue;
    out.push({ rule: `publish:${c.id}`, severity: sev(c.status), category: "Compliance", mode: null, title: c.label, big: c.value, ref: c.threshold, excess: 10, text: `${c.label}: ${c.value}.`, why: "RGS publish check.", target: { view: "compliance" } });
  }
  for (const c of checks.betLevelConstraints) {
    const base2 = { rule: `star:${c.label}`, category: "Compliance", mode: null, big: c.value, why: "Overall Bet Level Compliance: the 2★ and 3★ limits of the Stake report.", target: { view: "compliance" } };
    if (!c.pass2star && !c.pass3star) out.push({ ...base2, severity: lenient ? "minor" : "blocking", title: `${c.label} over the 3★ limit${agreed}`, ref: `2★ ${c.limit2star} · 3★ ${c.limit3star}`, excess: 10, text: `${c.value}: over both limits${lenient ? " — allowed for this studio, to be confirmed with Stake for this game." : "."}` });
    else if (!c.pass3star) out.push({ ...base2, severity: lenient ? "context" : "serious", title: `${c.label} keeps the game out of 3★${agreed}`, ref: `3★ ${c.limit3star}`, excess: 5, text: `${c.value}: over the 3★ limit.` });
    else if (!c.pass2star) out.push({ ...base2, severity: "context", title: `${c.label} over the 2★ limit (fine at 3★)`, ref: `2★ ${c.limit2star}`, excess: 1, text: `${c.value} passes 3★ (${c.limit3star}): the game is not eligible for 2★, which does not count as a fail at 3★.` });
  }
  const rtps = stats.map((m) => m.stats.rtp).sort((a, b) => a - b);
  const mid = rtps[Math.floor(rtps.length / 2)] ?? 0;
  for (const m of stats) {
    const d = m.stats.rtp - mid;
    if (Math.abs(d) > 1e-3) {
      out.push({ rule: "rtp-drift", severity: "context", category: "Compliance", mode: m.mode, title: "RTP off the other modes", big: `${d > 0 ? "+" : ""}${(d * 100).toFixed(2)} pt`, ref: fmtPct(mid, 2), excess: Math.abs(d) * 100, text: `RTP ${fmtPct(m.stats.rtp, 2)} vs ${fmtPct(mid, 2)}.`, why: "The report fails past 1 point of spread; a smaller drift is usually a lookup table not rebuilt.", target: { view: "compliance", mode: m.mode } });
    }
  }
  const base = stats.find((m) => m.stats.cost <= 2) ?? stats[0];
  if (base) {
    const std = base.stats.stdDev / Math.max(1, base.stats.cost);
    if (std <= REF.std3Star && std >= REF.std3Star * 0.9) {
      out.push({
        rule: "std-margin",
        severity: std >= REF.std3Star * 0.975 ? "serious" : "minor",
        category: "Compliance",
        mode: base.mode,
        title: "Almost at the 3★ volatility cap",
        big: std.toFixed(2),
        ref: `cap ${REF.std3Star}`,
        excess: std / (REF.std3Star * 0.9),
        text: `Std ÷ cost ${std.toFixed(2)}: ${(REF.std3Star - std).toFixed(2)} left before the cap.`,
        why: "Any RTP moved to a rare tier raises the base volatility and crosses the 3★ cap.",
        target: { view: "compliance", mode: base.mode }
      });
    }
  }
}
function distribution({ stats, lenient }, out) {
  const gameMax = Math.max(0, ...stats.map((m) => m.stats.maxPayout));
  for (const { mode, stats: s } of stats) {
    const d = { category: "Distribution", mode, target: { view: "explore", mode, key: "all" } };
    const accessible = s.cost >= 10 && s.maxPayoutOneIn <= REF.accessibleMaxWin;
    const bands = s.buckets.filter((b) => b.weight > 0 && !(b.start === 0 && b.end === 0));
    const gaps = [];
    const steps = [];
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
      if (last && b.probability > a.probability && b.start >= 1e3 && !accessible) {
        const q2 = b.probability / a.probability;
        out.push({ ...d, rule: "polar-top", severity: q2 >= 4 ? "serious" : q2 >= 2 ? "minor" : "context", title: `Max win more likely than the whole ${band(a)} band`, big: times(q2), ref: "should be rarer", excess: q2, text: `The max-win band (${band(b)}) is ${times(q2)} more likely than ${band(a)}.`, why: WHY.polarTop });
      }
    }
    if (gaps.length) {
      out.push({ ...d, rule: "gap", severity: "blocking", title: `No win possible in ${gaps[0]}${gaps.length > 1 ? ` (+${gaps.length - 1})` : ""}`, big: "gap", ref: "Stake: no gaps", excess: 10, text: `No drawable result in ${gaps.join(", ")}.`, why: WHY.gap });
    }
    if (steps.length) {
      const w = steps.reduce((x, y) => y.r / y.limit > x.r / x.limit ? y : x);
      const excess = w.r / w.limit;
      out.push({
        ...d,
        rule: "steps",
        severity: excess < REF.stepTolerance ? "context" : graded(excess),
        title: `${excess >= 2 ? "Hole" : "Steep drop"} in the curve at ${band(w.b)}${steps.length > 1 ? ` (+${steps.length - 1} more)` : ""}`,
        big: times(w.r),
        ref: `vs ${band(w.a)} · ≤ ×${w.limit}`,
        excess,
        text: steps.map((x) => `${band(x.a)} → ${band(x.b)} ${times(x.r)}`).join(" · "),
        why: WHY.steps
      });
    }
    if (s.mostLikelyOneIn < REF.mostLikelyMin) {
      out.push({ ...d, rule: "most-likely", severity: "blocking", title: "Most likely book too frequent", big: shortOneIn(s.mostLikelyOneIn), ref: `≥ 1 / ${REF.mostLikelyMin}`, excess: REF.mostLikelyMin / s.mostLikelyOneIn, text: `The most likely book lands ${oneIn(s.mostLikelyOneIn)}.`, why: WHY.mostLikely });
    }
    if (s.cvar001 >= REF.cvar3Star * 0.95) {
      const excess = s.cvar001 / (REF.cvar3Star * 0.95);
      out.push({
        ...d,
        rule: "cvar",
        // A per-mode estimate of a game-level Stake aggregate: a fault only where the grid is a rule.
        severity: s.cvar001 >= REF.cvar3Star && !lenient ? "minor" : "context",
        title: s.cvar001 >= REF.cvar3Star ? `Worst 0.1 % over the 3★ CVaR ceiling${lenient ? " (Stake exceptions agreed)" : ""}` : "Worst 0.1 % close to the 3★ CVaR ceiling",
        big: `${fk(Math.round(s.cvar001))}x`,
        ref: `≤ ${fk(REF.cvar3Star)}x (3★)`,
        excess,
        text: `CVaR 0.1 % ${fx(s.cvar001)} · max win ${fx(s.maxPayout)} lands ${oneIn(s.maxPayoutOneIn)}: the rarer the max win, the lower the CVaR.`,
        why: WHY.cvar
      });
    }
    if (s.maxPayout < gameMax) {
      out.push({ ...d, rule: "never-max", severity: "context", title: "Never reaches the game's max win", big: `${fk(s.maxPayout)}x`, ref: `game ${fk(gameMax)}x`, excess: gameMax / Math.max(s.maxPayout, 1), text: `Tops out at ${fx(s.maxPayout)}.`, why: "The mode's highest drawable result is below the game's max win." });
    }
    if (s.hitRate >= 0.05 && s.hitRate < REF.hitMin) {
      const excess = REF.hitMin / s.hitRate;
      out.push({ ...d, rule: "hit-low", severity: graded(excess), title: "Over 90 % of spins pay nothing", big: fmtPct(s.hitRate, 2), ref: "≥ 10.00 %", excess, text: `Hit rate ${fmtPct(s.hitRate, 2)}.`, why: "Flagged by the Stake evaluator on tachyon (non-paying ≤ 90 %)." });
    }
    if (s.cost <= 2 && s.hitRate > 0) {
      const sub = (s.hitRate - s.breakevenRate) / s.hitRate;
      if (sub > REF.subBetWins) {
        out.push({ ...d, rule: "sub-bet", severity: "context", title: "Most wins pay less than the bet", big: pct(sub), ref: `≤ ${REF.subBetWins * 100} %`, excess: sub / REF.subBetWins, text: `${pct(sub)} of the winning spins return less than the bet.`, why: "« Winning » mostly means losing a little less." });
      }
    }
  }
}
function tiersOf(mp) {
  return mp.populations.filter((p) => p.kind === "tier" && p.tier !== null).sort((a, b) => a.tier - b.tier);
}
function ladder(mp, out) {
  const tiers = tiersOf(mp);
  for (let i = 1; i < tiers.length; i++) {
    const lo = tiers[i - 1];
    const hi = tiers[i];
    const d = { category: "Bonus ladder", mode: mp.mode, why: WHY.ladder, target: { view: "explore", mode: mp.mode, key: hi.key } };
    if (hi.probability >= lo.probability) {
      out.push({ ...d, rule: "ladder-freq", severity: "blocking", title: `${hi.label} not rarer than ${lo.label}`, big: oneIn(hi.oneIn), ref: `vs ${oneIn(lo.oneIn)}`, excess: 10, text: `${hi.label} ${oneIn(hi.oneIn)} · ${lo.label} ${oneIn(lo.oneIn)}.` });
    }
    if (hi.mean <= lo.mean) {
      const top = i === tiers.length - 1;
      out.push({
        ...d,
        rule: "ladder-mean",
        severity: "blocking",
        title: top ? `${hi.label}: an extra tier that pays no more than ${lo.label}` : `${hi.label} pay less on average than ${lo.label}`,
        big: fx(hi.mean),
        ref: `vs ${fx(lo.mean)}`,
        excess: 10,
        text: `Average ${hi.label} ${fx(hi.mean)} · ${lo.label} ${fx(lo.mean)}${top ? ` — ${hi.label} land ${oneIn(hi.oneIn)}: one scatter more than the top bonus needs.` : "."}`,
        why: top ? WHY.tierExtra : WHY.ladder
      });
    } else if (mp.cost < 10 && hi.probability < lo.probability && hi.probability / lo.probability > REF.tierCloseness) {
      const r = hi.probability / lo.probability;
      const excess = r / REF.tierCloseness;
      out.push({ ...d, rule: "ladder-close", severity: r >= 2 / 3 ? "serious" : "minor", title: `${hi.label} almost as frequent as ${lo.label}`, big: oneIn(hi.oneIn), ref: `vs ${oneIn(lo.oneIn)} · at most ⅓ as often`, excess, text: `${hi.label} come ${pct(r)} as often as ${lo.label} (${oneIn(hi.oneIn)} vs ${oneIn(lo.oneIn)}); the base game keeps about ×6 between tiers.`, why: WHY.tierClose });
    }
    if (tiers.length >= 3 && i === tiers.length - 1 && hi.min < lo.median && hi.min < hi.max) {
      out.push({ ...d, rule: "hidden-floor", severity: "minor", title: `${hi.label} can pay less than a typical ${lo.label} bonus`, big: fx(hi.min), ref: `≥ ${fx(lo.median)} (${lo.label} median)`, excess: lo.median / Math.max(hi.min, 1e-9), text: `The hidden pays from ${fx(hi.min)} (P5 ${fx(quantile(hi, 0.05)?.value ?? hi.min)}) while a typical ${lo.label} bonus pays ${fx(lo.median)}.`, why: WHY.hiddenFloor });
    }
    if (hi.median <= lo.median) {
      out.push({ ...d, rule: "ladder-median", severity: "serious", title: `Typical ${hi.label} bonus no better than ${lo.label}`, big: fx(hi.median), ref: `vs ${fx(lo.median)}`, excess: 2, text: `Median ${hi.label} ${fx(hi.median)} · ${lo.label} ${fx(lo.median)}.`, why: "The higher tier only pays better on average: its typical bonus is not better." });
    }
    if (hi.spins && lo.spins && hi.spins.meanInitial < lo.spins.meanInitial) {
      out.push({ ...d, rule: "ladder-spins", severity: "minor", title: `${hi.label} award fewer spins than ${lo.label}`, big: hi.spins.meanInitial.toFixed(1), ref: `vs ${lo.spins.meanInitial.toFixed(1)}`, excess: 1.5, text: `${hi.label} ${hi.spins.meanInitial.toFixed(1)} spins · ${lo.label} ${lo.spins.meanInitial.toFixed(1)}.`, why: "A higher tier awards fewer free spins at entry." });
    }
  }
}
function variants(mp, out) {
  const tiers = tiersOf(mp);
  for (const [i, t] of tiers.entries()) {
    const vs = mp.populations.filter((p) => p.kind === "variant" && p.parent === t.key && p.min < p.max);
    if (!vs.length) continue;
    const below = tiers[i - 1];
    const above = tiers[i + 1];
    for (const v of vs) {
      const d = { category: "Variants", mode: mp.mode, target: { view: "explore", mode: mp.mode, key: v.key } };
      if (below && v.median < below.median) {
        out.push({ ...d, rule: "variant-below", severity: "serious", title: `${v.label} pays less than a typical ${below.label} bonus`, big: fx(v.median), ref: `vs ${fx(below.median)}`, excess: below.median / Math.max(v.median, 1e-9), text: `Median ${fx(v.median)} vs ${fx(below.median)}.`, why: "More trigger symbols, less money." });
      }
      if (above && v.median > above.median) {
        out.push({ ...d, rule: "variant-above", severity: "context", title: `${v.label} pays more than a typical ${above.label} bonus`, big: fx(v.median), ref: `vs ${fx(above.median)}`, excess: v.median / above.median, text: `Median ${fx(v.median)} vs ${fx(above.median)}.`, why: "A way into a tier that pays more than the tier above." });
      }
    }
    if (vs.length >= 2) {
      const byMed = [...vs].sort((a, b) => a.median - b.median);
      const lo = byMed[0];
      const hi = byMed[byMed.length - 1];
      if (lo.median > 0 && hi.median / lo.median >= 3) {
        const byCv = [...vs].sort((a, b) => a.cv - b.cv);
        out.push({
          rule: "variant-spread",
          severity: "context",
          category: "Variants",
          mode: mp.mode,
          title: `${t.label}: very different bonuses behind one trigger`,
          big: times(hi.median / lo.median),
          ref: "median spread",
          excess: hi.median / lo.median,
          text: `Medians ${fx(lo.median)} (${lo.criteria}) → ${fx(hi.median)} (${hi.criteria}) · most regular ${byCv[0].criteria}, most volatile ${byCv[byCv.length - 1].criteria}.`,
          why: "The ways into a same tier differ by 3× or more in median.",
          target: { view: "explore", mode: mp.mode, key: t.key }
        });
      }
    }
  }
}
const PRIORITY = { tier: 0, variant: 1, bonus: 2, all: 3, criteria: 4, nobonus: 5 };
const near = (a, b, tol = 0.03) => a === 0 && b === 0 || Math.abs(a - b) <= tol * Math.max(Math.abs(a), Math.abs(b));
function distinctPopulations(mp) {
  const kept = [];
  for (const p of [...mp.populations].sort((a, b) => PRIORITY[a.kind] - PRIORITY[b.kind])) {
    if (kept.some((k) => near(k.probability, p.probability) && near(k.mean, p.mean) && near(k.median, p.median))) continue;
    kept.push(p);
  }
  return kept;
}
function shapes({ pops, stats, market }, out) {
  const gameMax = Math.max(0, ...stats.map((m) => m.stats.maxPayout));
  for (const mp of pops) {
    const cost = mp.cost > 0 ? mp.cost : 1;
    const hasTiers = mp.populations.some((p) => p.kind === "tier");
    const hasNobonus = mp.populations.some((p) => p.kind === "nobonus" && p.mean > 0);
    const allBonus = !mp.populations.some((p) => p.kind === "nobonus") && hasTiers;
    const raised = /* @__PURE__ */ new Set();
    const tierRetrig = mp.populations.filter((p) => p.kind === "tier" && p.retriggers);
    if (tierRetrig.length && tierRetrig.every((p) => p.retriggers.rate >= 0.999)) {
      const b = mp.populations.find((p) => p.kind === "bonus") ?? mp.populations.find((p) => p.kind === "all");
      const mean = b?.retriggers?.mean ?? tierRetrig[0].retriggers.mean;
      out.push({ rule: "retrig-all", severity: "context", category: "Retriggers", mode: mp.mode, title: "Every bonus retriggers", big: `${mean.toFixed(1)} avg`, ref: "intended?", excess: 1, text: `Every bonus retriggers, ${mean.toFixed(1)} times on average.`, why: "Check it is the intended rule, not a counting artefact (e.g. one « retrigger » per fish eaten).", target: { view: "explore", mode: mp.mode, key: b?.key ?? "all" }, merge: "retrig-all", sig: [] });
    }
    for (const p of distinctPopulations(mp)) {
      const single = p.min === p.max;
      const isBonus = p.kind === "tier" || p.kind === "variant" || p.kind === "bonus" && !hasTiers || p.kind === "all" && allBonus;
      const isFeature = p.kind === "criteria" && !p.retriggers && p.mean > 0 && p.probability < 0.1 && p.criteria !== "base game";
      const name = p.kind === "all" ? "Purchases" : p.label;
      const mergeKey = (rule) => `${rule}:${p.kind === "all" ? "all" : p.key}`;
      const speak = (rule) => {
        if (p.kind === "variant" && p.parent && raised.has(`${rule}:${p.parent}`)) return false;
        raised.add(`${rule}:${p.key}`);
        return true;
      };
      const d = { mode: mp.mode, target: { view: "explore", mode: mp.mode, key: p.key } };
      if ((isBonus || isFeature) && !single) {
        const ratio = p.meanOverMedian ?? 0;
        const u10 = threshold(p, 10);
        const excess = Math.max(ratio / REF.meanOverMedian, p.top1Share / REF.top1);
        const skewed = isBonus ? excess >= 1 : ratio >= REF.meanOverMedian && p.top1Share >= 0.25;
        const rule = isBonus ? "skew" : "feature-skew";
        if (skewed && speak(rule)) {
          out.push({
            ...d,
            rule,
            severity: isBonus ? graded(excess) : "context",
            category: isBonus ? "Bonus shape" : "Features",
            title: `${name}: half pay under ${fx(p.median)}, average ${fx(p.mean)}`,
            big: `${ratio.toFixed(1)}×`,
            ref: `avg ÷ median ≤ ${REF.meanOverMedian}`,
            excess,
            text: (u10 && u10.below > 0.05 && u10.below < 1 ? `${pct(u10.below)} end under 10x · ` : "") + `the top 1 % carries ${pct(p.top1Share)} of the value · average without it ${fx(p.meanWithoutTop1)}.`,
            why: WHY.skew,
            merge: mergeKey(rule),
            sig: [p.median, p.mean]
          });
        }
        if (p.min > 0 && p.median <= 1.25 * p.min && p.mean >= 2 * p.median && speak("polar")) {
          const r2 = p.mean / p.median;
          out.push({
            ...d,
            rule: "polar",
            severity: graded(r2 / 2),
            category: isBonus ? "Bonus shape" : "Features",
            title: `${name}: the floor or a big win, little between`,
            big: fx(p.median),
            ref: `median, floor ${fx(p.min)}`,
            excess: r2 / 2,
            text: `Minimum ${fx(p.min)} · median ${fx(p.median)} · average ${fx(p.mean)}.`,
            why: WHY.polar,
            merge: mergeKey("polar"),
            sig: [p.min, p.mean]
          });
        }
      }
      if (isBonus && !single && p.effectiveOutcomes >= REF.essMin) {
        const q25 = quantile(p, 0.25)?.value ?? 0;
        const q75 = quantile(p, 0.75)?.value ?? 0;
        if (q25 > 0 && q75 / q25 < REF.flatSpread && speak("flat")) {
          const spread = q75 / q25;
          const excess = REF.flatSpread / spread;
          out.push({ ...d, rule: "flat", severity: spread < 1.2 ? "serious" : "minor", category: "Bonus shape", title: `${name}: most look alike — half land between ${fx(q25)} and ${fx(q75)}`, big: `×${spread.toFixed(2)}`, ref: `P75 ÷ P25 ≥ ${REF.flatSpread}`, excess, text: `P25 ${fx(q25)} · median ${fx(p.median)} · P75 ${fx(q75)} — the middle half of these bonuses spans ×${spread.toFixed(2)}.`, why: WHY.flat, merge: mergeKey("flat"), sig: [spread] });
        }
      }
      if (p.kind === "tier" && !single && cost < 10 && hasNobonus && p.mean > 0) {
        const tenth = 0.1 * p.mean;
        const under = cdfBelow(p, tenth);
        if (under > REF.deadBonusShare && speak("dead")) {
          const excess = under / REF.deadBonusShare;
          out.push({ ...d, rule: "dead-bonus", severity: graded(excess), category: "Bonus shape", title: `${name}: ${pct(under)} end under ${fx(tenth)} — a tenth of their average`, big: pct(under), ref: `≤ ${REF.deadBonusShare * 100} %`, excess, text: `${pct(under)} of the ${name} bonuses pay under ${fx(tenth)} (average ${fx(p.mean)}, median ${fx(p.median)}): dead bonuses the player remembers.`, why: WHY.deadBonus, merge: mergeKey("dead"), sig: [under] });
        }
      }
      if (isBonus && p.zeroRate > 0 && !(p.kind === "all" && p.zeroRate >= 0.5) && speak("zero")) {
        out.push({ ...d, rule: "zero", severity: "serious", category: "Bonus shape", title: `${name}: some pay nothing`, big: pct(p.zeroRate), ref: "0 %", excess: 2 + p.zeroRate, text: `${pct(p.zeroRate)} end at 0x.`, why: "A bonus that ends at 0x.", merge: mergeKey("zero"), sig: [p.zeroRate] });
      }
      if (isBonus && !single && p.maxShare >= 0.02 && p.max >= gameMax && speak("max")) {
        out.push({ ...d, rule: "max-share", severity: "context", category: "Bonus shape", title: `${name}: often end on the max win`, big: pct(p.maxShare), ref: "< 2 %", excess: p.maxShare / 0.02, text: `${pct(p.maxShare)} end on ${fx(p.max)}.`, why: "At least 2 % of these bonuses land exactly on the max win.", merge: mergeKey("max"), sig: [p.maxShare] });
      }
      if (isBonus && p.bonusOnly && p.bonusOnly.shareOfPayout < 0.5 && speak("fg")) {
        out.push({ ...d, rule: "fg-share", severity: "context", category: "Bonus shape", title: `${name}: free spins pay less than the trigger`, big: pct(p.bonusOnly.shareOfPayout), ref: "≥ 50 %", excess: 0.5 / Math.max(p.bonusOnly.shareOfPayout, 0.01), text: "Share of the win paid by the free spins themselves (`freeGameWins`).", why: "The spin that opened the bonus pays most of it." });
      }
      if (isFeature && p.zeroRate > 0.01 && p.zeroRate < 0.99) {
        out.push({ ...d, rule: "feature-mixed", severity: "context", category: "Features", title: `${name}: mixes wins and dead spins`, big: pct(p.zeroRate), ref: "at 0x", excess: 1, text: `${pct(p.zeroRate)} of its spins pay 0x.`, why: "Part of the spins of this criteria pay nothing." });
      }
      if (p.kind === "all" && cost >= 10 && p.zeroRate < 0.5) {
        const medCost = p.median / cost;
        if (medCost < REF.buyMedianOverCost || p.profitableRate < REF.buyProfitableMin) {
          const excess = Math.max(REF.buyMedianOverCost / Math.max(medCost, 1e-6), REF.buyProfitableMin / Math.max(p.profitableRate, 1e-6));
          const row = market?.modes.find((m) => m.mode === mp.mode)?.rows.find((r2) => r2.key === "median");
          const peers = row ? `peers ${row.p25.toFixed(2)}× · ${row.median.toFixed(2)}× · ${row.p75.toFixed(2)}×` : "market ≈ 0.25–0.5×";
          out.push({ ...d, rule: "buy-median", severity: graded(excess), category: "Buys", title: `A typical purchase returns ${medCost.toFixed(2)}× its cost`, big: fx(p.median), ref: `for ${fx(cost)} · ${peers}`, excess, text: `${pct(p.profitableRate)} of purchases return at least the cost; the studio aims at half the price (LAST LAUGH 2026-10-01).`, why: WHY.buy });
        }
      }
      const r = p.retriggers;
      if (r && p.kind === "tier" && !single && r.rate < 0.999 && r.avgWith !== null && r.avgWithout && r.avgWith / r.avgWithout >= 3) {
        const lift = r.avgWith / r.avgWithout;
        out.push({ ...d, rule: "retrig-lift", severity: "context", category: "Retriggers", title: `${name}: retriggers carry the bonus`, big: times(lift), ref: "with vs without", excess: lift / 3, text: `${fx(r.avgWith)} with a retrigger vs ${fx(r.avgWithout)} without · ${pct(r.rate)} of bonuses carry ${pct(r.valueShareWith)} of the value.`, why: "Retriggered bonuses pay 3× or more those that do not.", merge: mergeKey("retrig-lift"), sig: [lift] });
      }
      if (!single && (isBonus || isFeature || p.rtpShare >= 5e-3) && p.effectiveOutcomes < REF.essMin) {
        const excess = REF.essMin / Math.max(p.effectiveOutcomes, 1);
        out.push({ ...d, rule: "ess", severity: graded(excess), category: "Sample", title: `${name}: few distinct results`, big: p.effectiveOutcomes.toFixed(0), ref: `≥ ${REF.essMin}`, excess, text: `${p.effectiveOutcomes.toFixed(0)} effective outcomes out of ${p.drawableBooks} drawable books.`, why: WHY.ess });
      }
    }
  }
}
function perSpinAtLeast(p, x) {
  return p.probability * (threshold(p, x)?.atLeast ?? 0);
}
function bigWins({ pops }, out) {
  for (const mp of pops) {
    const cost = mp.cost > 0 ? mp.cost : 1;
    const all = mp.populations.find((p) => p.kind === "all");
    const nb = mp.populations.find((p) => p.kind === "nobonus");
    const withBonus = mp.populations.some((p) => p.kind === "bonus");
    if (all && nb && withBonus && cost < 10) {
      const d = { mode: mp.mode, target: { view: "explore", mode: mp.mode, key: "all" } };
      const r100 = perSpinAtLeast(nb, 100);
      const refOneIn = REF.outsideBonus100OneIn / cost;
      const all100 = perSpinAtLeast(all, 100);
      if (r100 > 0 && r100 * refOneIn > REF.outsideBonusTolerance) {
        const excess2 = r100 * refOneIn;
        out.push({
          ...d,
          rule: "nobonus-100",
          severity: graded(excess2),
          category: "Big wins",
          title: "Too many ≥ 100x wins without a bonus",
          big: shortOneIn(1 / r100),
          ref: `≤ ${shortOneIn(refOneIn)}`,
          excess: excess2,
          text: `A ≥ 100x without a bonus lands ${oneIn(1 / r100)} — ${pct(r100 / all100)} of all the ≥ 100x wins of the mode.`,
          why: WHY.outsideBonus
        });
      }
      const tops = [1e3, 5e3].map((x) => ({ x, whole: perSpinAtLeast(all, x), part: perSpinAtLeast(nb, x) })).filter((t) => t.whole > 0 && t.part > 0).map((t) => ({ ...t, share: t.part / t.whole }));
      const worst = tops.sort((a, b) => b.share - a.share)[0];
      if (worst && worst.share > REF.outsideBonusTopShare) {
        const excess2 = worst.share / REF.outsideBonusTopShare;
        out.push({
          ...d,
          rule: "nobonus-top",
          severity: graded(excess2),
          category: "Big wins",
          title: `${pct(worst.share)} of the ≥ ${fk(worst.x)}x wins come without a bonus`,
          big: pct(worst.share),
          ref: `≤ ${REF.outsideBonusTopShare * 100} %`,
          excess: excess2,
          text: tops.sort((a, b) => a.x - b.x).map((t) => `≥ ${fk(t.x)}x: ${pct(t.share)} without a bonus (${oneIn(1 / t.part)}) vs ${oneIn(1 / (t.whole - t.part))} from bonuses`).join(" · ") + ".",
          why: WHY.outsideBonus
        });
      }
    }
    const ra = all?.roundAmounts;
    if (!ra || ra.atLeast100 <= 0) continue;
    const share = Math.max(ra.share100, ra.share1000);
    if (share <= REF.roundShare) continue;
    const perSpin = ra.atLeast100 * ra.share100;
    const sources = mp.populations.filter((p) => p.kind === "criteria" && p.roundAmounts && p.roundAmounts.share100 > 0).map((p) => ({ label: p.label, part: p.roundAmounts.atLeast100 * p.roundAmounts.share100 / Math.max(perSpin, 1e-30) })).sort((a, b) => b.part - a.part).slice(0, 3);
    const excess = share / REF.roundShare;
    out.push({
      mode: mp.mode,
      target: { view: "explore", mode: mp.mode, key: "all" },
      rule: "round",
      severity: graded(excess),
      category: "Big wins",
      title: `${pct(share)} of the big wins land on a round multiplier`,
      big: pct(share),
      ref: `≤ ${REF.roundShare * 100} % · chance ≈ 0.1 %`,
      excess,
      text: `A spin pays a round multiplier (x100, x200… max win excluded) ${oneIn(1 / Math.max(perSpin, 1e-30))} — ${pct(ra.share100)} of the ≥ 100x wins` + (ra.atLeast1000 > 0 ? ` · ${pct(ra.share1000)} of the ≥ 1,000x wins are a multiple of 1,000x` : "") + (ra.topValue ? ` · most frequent x${ra.topValue.toLocaleString("en-US")} (${pct(ra.topShare)} of the ≥ 100x)` : "") + (sources.length ? ` · from ${sources.map((x) => `${x.label} ${pct(x.part)}`).join(", ")}` : "") + ".",
      why: WHY.round,
      merge: "round",
      sig: [ra.share100, ra.share1000]
    });
  }
}
function anteTiers({ pops }, out) {
  const spinModes2 = pops.filter(
    (mp) => mp.populations.some((p) => p.kind === "tier") && mp.populations.some((p) => p.kind === "nobonus" && p.mean > 0)
  );
  if (spinModes2.length < 2) return;
  const base = spinModes2.reduce((a, b) => b.cost < a.cost ? b : a);
  for (const mp of spinModes2) {
    if (mp === base || mp.cost <= base.cost) continue;
    for (const t of tiersOf(base)) {
      const p = mp.populations.find((c) => c.kind === "tier" && c.key === t.key);
      const pr = p?.probability ?? 0;
      if (pr >= t.probability) continue;
      out.push({
        rule: "ante-tier",
        severity: "blocking",
        category: "Across modes",
        mode: mp.mode,
        title: `${t.label} rarer than in ${base.mode}`,
        big: pr > 0 ? shortOneIn(1 / pr) : "never",
        ref: `${base.mode} ${shortOneIn(1 / t.probability)}`,
        excess: pr > 0 ? t.probability / pr : 10,
        text: `${t.label}: ${pr > 0 ? oneIn(1 / pr) : "never"} in ${mp.mode} (cost ${fx(mp.cost)}) vs ${oneIn(1 / t.probability)} in ${base.mode} (cost ${fx(base.cost)}).`,
        why: "Studio rule: a paid spin mode must not lower the chance of any bonus the base game gives — the player pays more to get more bonuses.",
        target: { view: "explore", mode: mp.mode, key: t.key }
      });
    }
  }
}
function acrossModes({ pops }, out) {
  const keys = new Set(pops.flatMap((mp) => mp.populations.filter((p) => p.kind === "tier").map((p) => p.key)));
  for (const key of keys) {
    const mysteryLike = (mp) => mp.cost >= 10 && mp.populations.filter((p) => p.kind === "tier").length >= 2;
    const rows = pops.filter((mp) => !mysteryLike(mp)).map((mp) => ({ mode: mp.mode, pop: mp.populations.find((p) => p.key === key) })).filter((r) => !!r.pop && r.pop.min < r.pop.max);
    if (rows.length < 2) continue;
    const lo = rows.reduce((a, b) => b.pop.median < a.pop.median ? b : a);
    const hi = rows.reduce((a, b) => b.pop.median > a.pop.median ? b : a);
    if (lo.pop.median > 0 && hi.pop.median / lo.pop.median >= REF.sameTierMedianGap) {
      const r = hi.pop.median / lo.pop.median;
      out.push({
        rule: "same-tier",
        severity: r >= 3 ? "serious" : r >= 2 ? "minor" : "context",
        category: "Across modes",
        mode: hi.mode,
        also: [lo.mode],
        title: `${hi.pop.label} pay ${times(r)} more in ${hi.mode} than in ${lo.mode}`,
        big: `${fx(hi.pop.median)} vs ${fx(lo.pop.median)}`,
        ref: `median · ≤ ×${REF.sameTierMedianGap}`,
        excess: r / REF.sameTierMedianGap,
        text: `Medians ${fx(hi.pop.median)} vs ${fx(lo.pop.median)} · averages ${fx(hi.pop.mean)} vs ${fx(lo.pop.mean)}${rows.length > 2 ? ` · in ${rows.length} modes: ${rows.map((x) => `${x.mode} ${fx(x.pop.median)}`).join(", ")}` : ""}.`,
        why: WHY.sameTier,
        target: { view: "explore", mode: hi.mode, key }
      });
    }
  }
}
function celebrations({ tiers, pops, stats }, out) {
  const guessed = [];
  for (const t of tiers) {
    const mode = t.report.mode;
    const cost = t.report.cost > 0 ? t.report.cost : 1;
    const target = { view: "tiers", mode };
    if (/default/i.test(t.report.source) && !/winLevelThresholds/.test(t.report.source)) {
      guessed.push(mode);
      continue;
    }
    const mp = pops.find((m) => m.mode === mode);
    const s = stats.find((m) => m.mode === mode)?.stats;
    const buy = !!mp && mp.populations.some((p) => p.kind === "tier") && !mp.populations.some((p) => p.kind === "nobonus") || cost >= 10 && (s?.hitRate ?? 0) >= 0.95;
    if (buy) continue;
    if (cost < 10) {
      const w = t.report.warnings;
      if (!w.length) continue;
      const often = w.map((x) => /^(.+?) lands 1 spin in ([\d.]+)/.exec(x)).filter((m) => !!m).map((m) => ({ name: m[1], every: Number(m[2]) }));
      const rest = w.filter((x) => !/ lands 1 spin in /.test(x));
      const top = often.length ? often.reduce((a, b) => b.every < a.every ? b : a) : null;
      const excess = top ? REF.plaqueEvery / top.every : 1.5;
      out.push({
        rule: "celebrations",
        severity: graded(excess),
        category: "Celebrations",
        mode,
        title: top ? `${top.name} plaque ${top.every <= 1.05 ? "on almost every spin" : `every ${top.every} spins`}` : "Celebrations too long",
        big: top ? `1 / ${top.every}` : "",
        ref: `rarer than 1 / ${REF.plaqueEvery}`,
        excess,
        text: [...often.map((o) => `${o.name} every ${o.every} spins`), ...rest].join(" · ") + ".",
        why: WHY.celebrations,
        target
      });
      continue;
    }
    const first = t.report.rows.find((r) => r.plaque && r.atLeast > 0);
    if (!first) continue;
    const breakeven = s?.breakevenRate ?? 0;
    if (first.threshold < cost) {
      const losing = first.atLeast > 0 ? Math.max(0, first.atLeast - breakeven) / first.atLeast : 0;
      out.push({
        rule: "plaque-loss",
        severity: "serious",
        category: "Celebrations",
        mode,
        title: `${first.name} plaque celebrates results below the ${fx(cost)} the spin cost`,
        big: fx(first.threshold),
        ref: `cost ${fx(cost)}`,
        excess: cost / first.threshold,
        text: `${pct(losing)} of the ${first.name} plaques are for a result under the cost.`,
        why: "A celebration only makes sense against what the player staked on THIS spin (client feedback, skullperch DEGEN SPIN).",
        target
      });
    } else {
      out.push({
        rule: "plaque-rhythm",
        severity: "context",
        category: "Celebrations",
        mode,
        title: `${first.name} plaque on 1 spin in ${first.atLeastOneIn.toFixed(1)}`,
        big: `${(first.threshold / cost).toFixed(1)}× cost`,
        ref: "first plaque",
        excess: 1,
        text: `From ${fx(first.threshold)}, i.e. ${(first.threshold / cost).toFixed(1)}× the ${fx(cost)} the spin cost: frequent, but never for a loss.`,
        why: "On a costly one-shot mode every spin is an event: the plaque rhythm is the mode, not a fault.",
        target
      });
    }
  }
  if (guessed.length) {
    out.push({
      rule: "plaque-unknown",
      severity: "context",
      category: "Celebrations",
      mode: null,
      title: "Celebrations not checked: the game has no winLevelThresholds.ts",
      big: `${guessed.length} modes`,
      ref: "",
      excess: 0,
      text: `Win tiers shows SlotLab's default ladder for ${guessed.join(", ")} — not what the game displays.`,
      why: "Celebration findings are only meaningful against the ladder the game really uses.",
      target: { view: "tiers" }
    });
  }
}
function pool({ pops, stats }, out) {
  const gameMax = Math.max(0, ...stats.map((m) => m.stats.maxPayout));
  for (const mp of pops) {
    if (!mp.imported) continue;
    const d = (key) => ({ mode: mp.mode, category: "Sample", target: { view: "explore", mode: mp.mode, key } });
    const parts = [];
    for (const p of mp.populations) if (p.kind === "tier") parts.push({ name: p.label, key: p.key, books: p.drawableBooks });
    for (const p of mp.populations) {
      if (p.kind !== "criteria" || !p.criteria || p.retriggers || p.criteria === "base game") continue;
      if (p.min === p.max && p.max >= gameMax) continue;
      if (p.probability < 0.05 && p.rtpShare >= REF.poolFeatureRtpShare) parts.push({ name: p.label, key: p.key, books: p.drawableBooks });
    }
    for (const part of parts) {
      if (part.books >= REF.poolMin) continue;
      const excess = REF.poolMin / Math.max(part.books, 1);
      out.push({ ...d(part.key), rule: "pool", severity: graded(excess), title: `${part.name}: too few books`, big: fk(part.books), ref: `≥ ${fk(REF.poolMin)}`, excess, text: `${part.books.toLocaleString("en-US")} drawable books behind ${part.name}.`, why: WHY.pool, merge: `pool:${part.name}`, sig: [part.books] });
    }
    const all = mp.populations.find((p) => p.kind === "all");
    const top = all?.topPayouts[0];
    if (top && gameMax > 0 && top.payout >= gameMax && top.books < REF.poolMaxWinMin) {
      const excess = REF.poolMaxWinMin / Math.max(top.books, 1);
      out.push({ ...d("all"), rule: "pool-max", severity: graded(excess), title: "Few books on the max win", big: fk(top.books), ref: `≥ ${fk(REF.poolMaxWinMin)}`, excess, text: `${top.books.toLocaleString("en-US")} drawable books pay ${fx(top.payout)}.`, why: WHY.pool, merge: "pool-max", sig: [top.books] });
    }
    for (const p of mp.populations) {
      if (p.kind !== "tier") continue;
      const bs = p.bands.filter((b) => b.end !== 0);
      const maxBand = bs.findIndex((b) => p.max >= b.start && (b.end === null || p.max < b.end));
      const used = bs.map((b, i) => b.books > 0 && i !== maxBand);
      const first = used.indexOf(true);
      const last = Math.max(used.lastIndexOf(true), p.max >= gameMax && maxBand >= 0 ? maxBand : -1);
      const label = (b) => b.end === null ? `${fk(b.start)}+` : `${fk(b.start)}–${fk(b.end)}`;
      const holes = first < 0 ? [] : bs.filter((b, i) => i > first && i < last && i !== maxBand && b.books === 0).map(label);
      if (holes.length) {
        out.push({ ...d(p.key), rule: "pool-hole", severity: "serious", title: `${p.label}: no book in ${holes[0]}${holes.length > 1 ? ` (+${holes.length - 1})` : ""}`, big: "0", ref: "books", excess: 2 + holes.length, text: `Empty: ${holes.join(", ")} — between populated bands of the same bonus.`, why: WHY.poolBands, merge: `pool-hole:${p.label}`, sig: [holes.length] });
      }
      const memorable = Math.max(REF.memorableFrom, REF.memorableFromCost * mp.cost);
      const lifetime = REF.lifetimeSpins / Math.max(mp.cost, 1);
      const repeatsAfter = (b) => Math.sqrt(Math.PI / 2 * b.effective) / Math.max(b.share * p.probability, 1e-300);
      const needOf = (b) => Math.min(REF.bandEffectiveMin, lifetime * b.share * p.probability);
      const thin = bs.filter((b, i) => i !== maxBand && b.books > 0 && b.start >= memorable && b.effective < needOf(b));
      if (thin.length) {
        const worst = thin.reduce((a, b) => b.effective < a.effective ? b : a);
        const excess = needOf(worst) / Math.max(worst.effective, 1);
        out.push({ ...d(p.key), rule: "pool-thin", severity: excess < REF.bandEffectiveTolerance ? "context" : graded(excess), title: `${p.label}: ${fk(Math.round(worst.effective))} effective books in ${label(worst)}`, big: fk(Math.round(worst.effective)), ref: `≥ ${fk(Math.round(needOf(worst)))}`, excess, text: thin.map((b) => `${label(b)}: ${Math.round(b.effective).toLocaleString("en-US")} effective of ${b.books.toLocaleString("en-US")} books for ~${fk(Math.round(lifetime * b.share * p.probability))} hits in the game's life — the same round comes back after ~${fk(Math.round(repeatsAfter(b)))} spins`).join(" · "), why: WHY.poolBands, merge: `pool-thin:${p.label}`, sig: [worst.effective] });
      }
    }
  }
}
function baseGame({ pops, stats }, out) {
  for (const mp of spinModes(pops)) {
    if (mp.cost > 2) continue;
    const d = (key) => ({ mode: mp.mode, category: "Base game", target: { view: "explore", mode: mp.mode, key } });
    const tiers = tiersOf(mp);
    const bonus = mp.populations.find((p) => p.kind === "bonus");
    const nobonus = mp.populations.find((p) => p.kind === "nobonus");
    if (bonus && bonus.oneIn > REF.anyBonusMax) {
      const excess = bonus.oneIn / REF.anyBonusMax;
      out.push({ ...d(bonus.key), rule: "bonus-rare", severity: excess < REF.bonusRareTolerance ? "context" : graded(excess), title: "Bonuses too rare in the base game", big: oneIn(bonus.oneIn), ref: `≤ 1 / ${REF.anyBonusMax} · aim 1 / 150–180`, excess, text: `A bonus, any tier, lands ${oneIn(bonus.oneIn)}${tiers.length ? ` (${tiers.map((t) => `${t.label} ${oneIn(t.oneIn)}`).join(" · ")})` : ""}.`, why: WHY.bonusRare });
    }
    if (tiers.length >= 2 && tiers[0].oneIn > REF.firstTierMax) {
      const t = tiers[0];
      const excess = t.oneIn / REF.firstTierMax;
      out.push({ ...d(t.key), rule: "first-tier-rare", severity: excess < REF.firstTierTolerance ? "context" : graded(excess), title: `${t.label} too rare`, big: oneIn(t.oneIn), ref: `≤ 1 / ${REF.firstTierMax} · aim 1 / 180–200`, excess, text: `The first bonus tier lands ${oneIn(t.oneIn)}; the studio aims at 1 in 180–200 spins.`, why: WHY.bonusRare });
    }
    if (tiers.length >= 3) {
      const h = tiers[tiers.length - 1];
      if (h.oneIn > REF.hiddenMax) {
        const excess = h.oneIn / REF.hiddenMax;
        out.push({ ...d(h.key), rule: "hidden-rare", severity: excess < REF.hiddenTolerance ? "context" : graded(excess), title: `${h.label} (hidden) rarer than 1 / ${REF.hiddenMax.toLocaleString("en-US")}`, big: oneIn(h.oneIn), ref: `1 / 15 000 – 1 / ${REF.hiddenMax.toLocaleString("en-US")}`, excess, text: `The top tier lands ${oneIn(h.oneIn)}; « 1/30 000 c'est trop rare, 1/20 000 max ».`, why: WHY.bonusRare });
      }
    }
    if (bonus && nobonus && bonus.rtpShare > REF.bonusRtpShareMax) {
      const excess = bonus.rtpShare / REF.bonusRtpShareMax;
      out.push({ ...d("nobonus"), rule: "rtp-split", severity: graded(excess), title: `Base game carries only ${pct(1 - bonus.rtpShare)} of the RTP`, big: pct(1 - bonus.rtpShare), ref: `≥ ${Math.round((1 - REF.bonusRtpShareMax) * 100)} % · 50 % on a 30k game`, excess, text: `Bonuses ${pct(bonus.rtpShare)} of the RTP · plain spins ${pct(nobonus.rtpShare)} (${tiers.map((t) => `${t.label} ${pct(t.rtpShare)}`).join(" · ")}).`, why: WHY.rtpSplit });
    }
    const s = stats.find((m) => m.mode === mp.mode)?.stats;
    if (s && s.maxPayout > 0) {
      const want = expectedHit(s.maxPayout);
      if (s.hitRate < REF.hitTolerance * want) {
        const excess = REF.hitTolerance * want / Math.max(s.hitRate, 1e-9);
        const sub = Math.max(0, s.hitRate - s.breakevenRate);
        out.push({ ...d("all"), rule: "base-dry", severity: graded(excess), title: `Base game dry: ${fmtPct(s.hitRate, 1)} of spins pay`, big: fmtPct(s.hitRate, 1), ref: `≈ ${fmtPct(want, 0)} for a ${fk(s.maxPayout)}x game`, excess, text: `Dead ${fmtPct(s.zeroRate, 1)} · under the stake ${fmtPct(sub, 1)} · at least the stake ${fmtPct(s.breakevenRate, 1)} — the studio aims at ${fmtPct(want, 0)} of paying spins for this max win.`, why: WHY.baseDry });
      }
    }
  }
}
function symbolMix({ pops }, out) {
  for (const mp of pops) {
    const s = mp.symbols;
    if (!s) continue;
    const parts = [
      { key: "base", label: "base game", mix: s.base, applies: mp.cost < 10 },
      { key: "fs", label: "free spins", mix: s.freeSpins, applies: true }
    ];
    for (const p of parts) {
      const n = p.mix.low + p.mix.high;
      if (!p.applies || !(n > 0)) continue;
      const high = p.mix.high / n;
      const off = Math.abs(high - REF.highShare);
      if (off <= REF.highShareTolerance) continue;
      const excess = off / REF.highShareTolerance;
      out.push({
        rule: `symbols-${p.key}`,
        severity: graded(excess),
        category: p.key === "base" ? "Base game" : "Bonus shape",
        mode: mp.mode,
        title: `${high < REF.highShare ? "Few" : "Too many"} high symbols on screen in the ${p.label}`,
        big: `${Math.round(high * 100)} % high`,
        ref: `${Math.round(REF.highShare * 100)} % ± ${Math.round(REF.highShareTolerance * 100)}`,
        excess,
        text: `${pct(high)} of the paying symbols revealed in the ${p.label} are high, ${pct(1 - high)} low (weighted by the lookup table). Fix on the reel strips: swap ${high < REF.highShare ? "lows for highs" : "highs for lows"}, keeping the relative weights inside each group; the lookup table keeps the RTP.`,
        why: WHY.symbolMix,
        target: { view: "explore", mode: mp.mode, key: "all" },
        merge: `symbols-${p.key}`,
        sig: [high]
      });
    }
  }
}
function anteMaxWin({ pops, stats }, out) {
  const modes = spinModes(pops);
  if (modes.length < 2) return;
  const base = modes[0];
  const bs = stats.find((m) => m.mode === base.mode)?.stats;
  if (!bs || !isFinite(bs.maxPayoutOneIn)) return;
  for (const mp of modes.slice(1)) {
    const s = stats.find((m) => m.mode === mp.mode)?.stats;
    if (!s || s.maxPayout !== bs.maxPayout || !isFinite(s.maxPayoutOneIn) || mp.cost <= base.cost) continue;
    const expected = bs.maxPayoutOneIn / (mp.cost / Math.max(base.cost, 1e-9));
    const ratio = expected / s.maxPayoutOneIn;
    if (ratio <= REF.anteMaxWinTolerance && ratio >= 1 / REF.anteMaxWinTolerance) continue;
    const excess = Math.max(ratio, 1 / ratio) / REF.anteMaxWinTolerance;
    out.push({
      rule: "ante-maxwin",
      severity: graded(excess),
      category: "Across modes",
      mode: mp.mode,
      title: `Max win ${times(Math.max(ratio, 1 / ratio))} ${ratio > 1 ? "more" : "less"} frequent than ${base.mode} would give at ${fx(mp.cost)}`,
      big: shortOneIn(s.maxPayoutOneIn),
      ref: `expected ≈ ${shortOneIn(expected)}`,
      excess,
      text: `${base.mode} ${shortOneIn(bs.maxPayoutOneIn)} × ${fx(mp.cost)} the cost ⇒ ${shortOneIn(expected)} at the same share of the RTP; ${mp.mode} lands it ${shortOneIn(s.maxPayoutOneIn)}.`,
      why: WHY.anteMaxWin,
      target: { view: "explore", mode: mp.mode, key: "all" }
    });
  }
}
function marketFindings({ market }, out) {
  if (!market) return;
  for (const m of market.modes) {
    const strong = [];
    const mild = [];
    for (const row of m.rows) {
      if (row.key === "rtp" || inMiddleRow(row)) continue;
      const spec = FIGURES[row.key];
      if (!spec) continue;
      const label = spec.label(m);
      const more = row.value > row.median;
      const beyond = row.key === "pmax" ? more ? row.value > row.p75 : false : row.rank >= 1 - REF.marketOutRank || row.rank <= REF.marketOutRank;
      const text = `${label} ${spec.format(row.value)} — peers ${spec.format(row.p25)} · ${spec.format(row.median)} · ${spec.format(row.p75)} (${position(row, spec)}).`;
      if (beyond) strong.push({ row, label, text });
      else mild.push(label);
    }
    const target = { view: "market", mode: m.mode };
    const n = strong.length + mild.length;
    if (!n) continue;
    out.push({
      rule: "market",
      severity: "context",
      category: "Market",
      mode: m.mode,
      title: strong.length ? `${strong.length} figure${strong.length > 1 ? "s" : ""} beyond p10–p90 of its peers${mild.length ? `, ${mild.length} more outside the middle half` : ""}` : `${n} figure${n > 1 ? "s" : ""} outside the peers' middle half`,
      big: `${n}`,
      ref: `of ${m.rows.length} · ${m.count} peers`,
      excess: strong.length + mild.length / 10,
      text: [...strong.map((s) => s.text), ...mild.length ? [`Outside the middle half, within p10–p90: ${mild.join(" · ")}.`] : []].join(" "),
      why: WHY.market,
      target
    });
  }
}
function data({ pops, popsError }, out) {
  if (popsError) out.push({ rule: "pops-error", severity: "minor", category: "Data", mode: null, title: "Bonus analysis unavailable", big: "error", ref: "", excess: 1, text: popsError, why: "The populations endpoint failed." });
  for (const mp of pops) {
    const u = mp.populations.find((p) => p.kind === "tier" && p.tier === null);
    if (u) out.push({ rule: "untiered", severity: "context", category: "Data", mode: mp.mode, title: "Bonuses without a known tier", big: pct(u.shareOfBonuses ?? 0), ref: "of the bonuses", excess: 1, text: "Their freeSpinTrigger does not list positions.", why: "SlotLab cannot tell how many symbols opened them.", target: { view: "explore", mode: mp.mode, key: u.key } });
    if (!mp.imported) out.push({ rule: "not-imported", severity: "context", category: "Data", mode: mp.mode, title: "Books still importing", big: "", ref: "", excess: 0, text: "Bonus findings appear once the import ends.", why: "" });
  }
}
const UNMUTABLE_PREFIXES = ["check:", "publish:", "star:"];
const UNMUTABLE_RULES = ["gap", "most-likely", "ladder-freq", "ladder-mean", "ante-tier"];
function canMute(rule, blocking) {
  if (blocking || !rule) return false;
  return !UNMUTABLE_PREFIXES.some((p) => rule.startsWith(p)) && !UNMUTABLE_RULES.includes(rule);
}
function muteOf(mutes, rule, mode) {
  return (mode !== null ? mutes.find((m) => m.rule === rule && m.mode === mode) : void 0) ?? mutes.find((m) => m.rule === rule && m.mode === null);
}
function splitIssues(issues, mutes) {
  const out = { shown: [], muted: [] };
  const withModes = (it, modes) => ({ ...it, mode: modes[0], also: modes.length > 1 ? modes.slice(1) : void 0 });
  for (const it of issues) {
    if (!mutes.length || !canMute(it.rule, it.severity === "blocking")) {
      out.shown.push(it);
      continue;
    }
    if (it.mode === null) {
      (muteOf(mutes, it.rule, null) ? out.muted : out.shown).push(it);
      continue;
    }
    const modes = [it.mode, ...it.also ?? []];
    const off = modes.filter((m) => muteOf(mutes, it.rule, m));
    if (!off.length) out.shown.push(it);
    else if (off.length === modes.length) out.muted.push(it);
    else {
      out.shown.push(withModes(it, modes.filter((m) => !off.includes(m))));
      out.muted.push(withModes(it, off));
    }
  }
  return out;
}
const DEFAULT_BASE = process.env.SLOTLAB_URL ?? "http://localhost:3600";
async function get(base, path, init) {
  const res = await fetch(base + path, { headers: { "Content-Type": "application/json" }, ...init });
  const body = await res.json().catch(() => null);
  if (!res.ok) throw new Error(body?.error ?? `${res.status} ${res.statusText} on ${path}`);
  return body;
}
const enc = encodeURIComponent;
const games = (base = DEFAULT_BASE) => get(base, "/api/ui/games");
const version = (game, base = DEFAULT_BASE) => get(base, `/api/ui/games/${enc(game)}/version`);
const reload = (game, base = DEFAULT_BASE) => get(base, `/api/ui/games/${enc(game)}/reload`, { method: "POST" });
const reach = (game, mode, spins, x, base = DEFAULT_BASE) => get(base, `/api/ui/games/${enc(game)}/reach?mode=${enc(mode)}&spins=${spins}&x=${x}`);
async function waitImported(game, base = DEFAULT_BASE, timeoutMs = 20 * 6e4) {
  const t0 = Date.now();
  for (; ; ) {
    const v = await version(game, base);
    if (v.imported && !v.sourceChanged) return v;
    if (Date.now() - t0 > timeoutMs) throw new Error(`${game}: books still not imported after ${Math.round(timeoutMs / 6e4)} min`);
    await new Promise((r) => setTimeout(r, 5e3));
  }
}
async function fetchGame(game, base = DEFAULT_BASE) {
  const [list, v, s, checks, p, tiers, market, muted] = await Promise.all([
    games(base).catch(() => []),
    version(game, base),
    get(base, `/api/ui/games/${enc(game)}/stats`),
    get(base, `/api/ui/games/${enc(game)}/checks`),
    get(base, `/api/ui/games/${enc(game)}/populations`),
    get(base, `/api/ui/games/${enc(game)}/tiers`).catch(() => null),
    get(base, `/api/ui/games/${enc(game)}/market`).catch(() => null),
    get(base, `/api/ui/games/${enc(game)}/muted`).then((r) => r.muted.map((m) => ({ ...m, mode: m.mode ?? null }))).catch(() => [])
  ]);
  return { game, summary: list.find((g) => g.name === game), version: v, stats: s.modes, checks, pops: p.modes, tiers, market, muted };
}
function isLenient(d, choice = "auto") {
  if (choice !== "auto") return choice;
  return /degen[-_ ]?lab/i.test(d.summary?.publishDir ?? "");
}
const q = (p, v) => p.quantiles.find((x) => Math.abs(x.q - v) < 1e-9)?.value ?? 0;
function buildReport(d, lenient) {
  const tiers = d.tiers?.modes ?? [];
  const split = splitIssues(allFindings({ stats: d.stats, checks: d.checks, pops: d.pops, tiers, market: d.market, lenient }), d.muted ?? []);
  const issues = split.shown;
  const counts = Object.fromEntries(SEVERITIES.map((s) => [s, issues.filter((i) => i.severity === s).length]));
  const modes = d.stats.map(({ mode, stats: s }) => {
    const mp = d.pops.find((m) => m.mode === mode);
    const pops = mp?.populations ?? [];
    const nb = pops.find((p) => p.kind === "nobonus");
    const b = pops.find((p) => p.kind === "bonus");
    const mm = d.market?.modes.find((m) => m.mode === mode);
    return {
      mode,
      cost: s.cost,
      rtp: s.rtp,
      hitRate: s.hitRate,
      stdOverCost: s.stdDev / Math.max(1, s.cost),
      maxWin: s.maxPayout,
      maxWinOneIn: s.maxPayoutOneIn,
      mostLikelyOneIn: s.mostLikelyOneIn,
      shortSession: s.shortSession,
      nobonus: nb ? { rtpShare: nb.rtpShare, mean: nb.mean, hitRate: 1 - nb.zeroRate } : null,
      bonus: b ? { oneIn: b.oneIn, rtpShare: b.rtpShare, median: b.median, mean: b.mean } : null,
      tiers: pops.filter((p) => p.kind === "tier").map((p) => ({ label: p.label, key: p.key, oneIn: p.oneIn, shareOfBonuses: p.shareOfBonuses, min: p.min, median: p.median, mean: p.mean, p90: q(p, 0.9), rtpShare: p.rtpShare, books: p.drawableBooks, effective: p.effectiveOutcomes })),
      features: pops.filter((p) => p.kind === "criteria" && p.criteria !== "base game").map((p) => ({ label: p.label, oneIn: p.oneIn, zeroRate: p.zeroRate, median: p.median, mean: p.mean, rtpShare: p.rtpShare })),
      anticipation: mp?.anticipation ?? null,
      market: (mm?.rows ?? []).filter((r) => FIGURES[r.key]).map((r) => {
        const spec = FIGURES[r.key];
        return { key: r.key, label: spec.label(mm), value: spec.format(r.value), peers: `${spec.format(r.p25)} · ${spec.format(r.median)} · ${spec.format(r.p75)}`, position: position(r, spec), out: !inMiddleRow(r) };
      }),
      marketPeers: mm ? `${mm.count} ${mm.kind} peers, max win ${fx(mm.window[0])}–${fx(mm.window[1])}, cost ${mm.costs[0]}×–${mm.costs[1]}×` : null
    };
  });
  return { game: d.game, readAt: (/* @__PURE__ */ new Date()).toISOString(), version: d.version, lenient, compliant: d.checks.passed, starTier: d.checks.starTier, counts, issues, muted: split.muted, modes };
}
const n0 = (v) => isFinite(v) ? Math.round(v).toLocaleString("en-US") : "—";
function toMarkdown(r, opts = {}) {
  const L = [];
  const v = r.version;
  L.push(`# SlotLab — ${r.game}`);
  L.push(`Build read at ${new Date(v.loadedAt).toLocaleTimeString("en-GB")} · build ${v.fingerprint}${v.sourceChanged ? " · ⚠️ FILES CHANGED ON DISK SINCE (reloading)" : ""}${v.imported ? "" : " · ⚠️ BOOKS NOT IMPORTED (bonus findings missing)"}`);
  L.push(`Compliance: ${r.compliant ? "COMPLIANT" : "FAIL"} · estimated ${r.starTier}★ · 3★ grid ${r.lenient ? "exceptions agreed for this studio (lenient)" : "required (strict)"}`);
  L.push("");
  const faults = r.counts.blocking + r.counts.serious + r.counts.minor;
  L.push(`## Worth a look — ${faults === 0 ? "nothing: no blocking, serious or minor finding" : `${r.counts.blocking} blocking · ${r.counts.serious} serious · ${r.counts.minor} minor`}${opts.context ? ` · ${r.counts.context} context` : ` (${r.counts.context} context hidden)`}${r.muted.length ? ` · ${r.muted.length} muted in the UI (${[...new Set(r.muted.map((i) => i.rule))].join(", ")})` : ""}`);
  const modesShown = [...r.modes.map((m) => m.mode), null].filter((m) => !opts.mode || m === opts.mode || m === null);
  for (const mode of modesShown) {
    const list = issuesOf(r.issues, mode).filter((i) => opts.context || i.severity !== "context");
    if (!list.length) continue;
    L.push("");
    L.push(`### ${mode ?? "Whole game"}`);
    for (const it of list) {
      L.push(`- **${it.severity}** — ${it.title} — \`${it.big}\` (${it.ref})`);
      L.push(`  ${it.text}${it.also?.length ? ` Same in ${it.also.join(", ")}.` : ""}`);
    }
  }
  L.push("");
  L.push("## Modes");
  for (const m of r.modes.filter((m2) => !opts.mode || m2.mode === opts.mode)) {
    L.push("");
    L.push(`### ${m.mode} — ${fx(m.cost)} · RTP ${fmtPct(m.rtp, 2)} · hit ${fmtPct(m.hitRate, 2)} · std÷cost ${m.stdOverCost.toFixed(2)} · max win ${fx(m.maxWin)} ${oneIn(m.maxWinOneIn)} · most likely ${oneIn(m.mostLikelyOneIn)}`);
    if (m.nobonus && m.bonus) L.push(`- RTP split: plain spins ${pct(m.nobonus.rtpShare)} / bonuses ${pct(m.bonus.rtpShare)} · a bonus ${oneIn(m.bonus.oneIn)} (median ${fx(m.bonus.median)}, average ${fx(m.bonus.mean)})`);
    if (m.shortSession) L.push(`- 100 rounds: median return ${n0(m.shortSession.median)} of 100 · ${pct(m.shortSession.win)} of sessions ahead · ${pct(m.shortSession.bust)} give back under 20 · middle half ${n0(m.shortSession.p25)}–${n0(m.shortSession.p75)}`);
    if (m.tiers.length) {
      L.push("- Tiers: | tier | frequency | share of bonuses | min | median | average | P90 | RTP share | books (effective) |");
      L.push("  |---|---|---|---|---|---|---|---|---|");
      for (const t of m.tiers) L.push(`  | ${t.label} | ${oneIn(t.oneIn)} | ${t.shareOfBonuses === null ? "—" : pct(t.shareOfBonuses)} | ${fx(t.min)} | ${fx(t.median)} | ${fx(t.mean)} | ${fx(t.p90)} | ${pct(t.rtpShare)} | ${n0(t.books)} (${n0(t.effective)}) |`);
    }
    for (const f of m.features) L.push(`- Feature ${f.label}: ${oneIn(f.oneIn)} · ${pct(f.zeroRate)} pay nothing · median ${fx(f.median)} · average ${fx(f.mean)} · ${pct(f.rtpShare)} of the RTP`);
    if (m.anticipation) {
      const a = m.anticipation;
      L.push(`- Anticipation (reels slowing for the missing scatter, read on the books): ${oneIn(1 / a.all)} spins · without a bonus (near misses felt, forced or natural) ${a.withoutBonus > 0 ? oneIn(1 / a.withoutBonus) : "never"}${a.bonusAnnounced !== null ? ` · ${pct(a.bonusAnnounced)} of the bonuses announced` : ""}`);
    }
    if (opts.market !== false && m.market.length) {
      const out = m.market.filter((x) => x.out);
      L.push(`- Market (${m.marketPeers}): ${out.length ? `${out.length} of ${m.market.length} figures outside the middle half` : "every figure inside the middle half of the peers"}`);
      for (const x of out) L.push(`  - ${x.label}: **${x.value}** — ${x.position} (peers p25 · median · p75: ${x.peers})`);
    }
  }
  return L.join("\n");
}
function populationMarkdown(mp, p) {
  const L = [];
  L.push(`### ${mp.mode} › ${p.label} (${p.kind}, key \`${p.key}\`)`);
  L.push(`${p.kind === "all" ? "every spin" : oneIn(p.oneIn)} · ${pct(p.probability)} per spin · RTP share ${pct(p.rtpShare)} (${pct(p.rtpContribution)} points) · median ${fx(p.median)} · average ${fx(p.mean)} · min ${fx(p.min)} · max ${fx(p.max)} · 0x ${pct(p.zeroRate)} · ≥ cost ${pct(p.profitableRate)} · top 1 % carries ${pct(p.top1Share)} · ${n0(p.drawableBooks)} books, ${n0(p.effectiveOutcomes)} effective`);
  L.push("Percentiles: " + p.quantiles.map((x) => `P${Math.round(x.q * 100)} ${fx(x.value)} (#${x.simId})`).join(" · "));
  L.push("Under: " + p.thresholds.filter((t) => t.below > 0 && t.below < 1).map((t) => `${fx(t.x)} ${pct(t.below)}`).join(" · "));
  L.push("Bands (share · value share · books/effective): " + p.bands.filter((b) => b.share > 0).map((b) => `${b.start === 0 && b.end === 0 ? "0" : `${b.start}–${b.end ?? "∞"}`}: ${pct(b.share)} · ${pct(b.valueShare)} · ${b.books}/${Math.round(b.effective)}`).join(" · "));
  if (p.retriggers) L.push(`Retriggers: ${pct(p.retriggers.rate)} retrigger, ${p.retriggers.mean.toFixed(2)} on average, max ${p.retriggers.max}`);
  if (p.spins) L.push(`Free spins: ${p.spins.meanInitial.toFixed(2)} awarded, ${p.spins.meanFinal.toFixed(2)} played`);
  return L.join("\n");
}
function marketMarkdown(r, mode) {
  const L = [`# Market — ${r.game}`];
  for (const m of r.modes.filter((m2) => !mode || m2.mode === mode)) {
    L.push("");
    L.push(`## ${m.mode} (${fx(m.cost)}) — ${m.marketPeers ?? "no peers"}`);
    L.push("| figure | mode | peers p25 · median · p75 | position |");
    L.push("|---|---|---|---|");
    for (const x of m.market) L.push(`| ${x.label} | ${x.out ? `**${x.value}**` : x.value} | ${x.peers} | ${x.position} |`);
  }
  return L.join("\n");
}
async function report(game, opts = {}) {
  const d = await fetchGame(game, opts.base ?? DEFAULT_BASE);
  return buildReport(d, isLenient(d, opts.lenient ?? "auto"));
}
function usage() {
  console.error(`usage: slotlab-report <game> [--mode M] [--json] [--context] [--no-market] [--strict|--lenient] [--wait] [--reload] [--population KEY] [--market] [--base URL]
       slotlab-report --list`);
  process.exit(2);
}
async function main(argv) {
  const args = [...argv];
  const flag = (name) => {
    const i = args.indexOf(name);
    if (i < 0) return void 0;
    const v = args[i + 1];
    args.splice(i, 2);
    return v;
  };
  const has = (name) => {
    const i = args.indexOf(name);
    if (i < 0) return false;
    args.splice(i, 1);
    return true;
  };
  const base = flag("--base") ?? DEFAULT_BASE;
  if (has("--list")) {
    const g = await games(base);
    for (const x of g) console.log(`${x.name}	${x.loaded ? "loaded" : "NOT LOADED"}	${x.imported ? "imported" : "not imported"}	${x.modes.map((m) => `${m.name} ${m.cost}x`).join(", ")}	${x.publishDir}`);
    return;
  }
  const json = has("--json");
  const context = has("--context");
  const noMarket = has("--no-market");
  const strict = has("--strict");
  const lenient = has("--lenient");
  const wait = has("--wait");
  const doReload = has("--reload");
  const marketOnly = has("--market");
  const mode = flag("--mode");
  const population = flag("--population");
  const game = args.find((a) => !a.startsWith("--"));
  if (!game) usage();
  if (doReload) await reload(game, base);
  if (wait || doReload) await waitImported(game, base);
  const d = await fetchGame(game, base);
  const r = buildReport(d, isLenient(d, strict ? false : lenient ? true : "auto"));
  if (population) {
    const mp = d.pops.find((m) => m.mode === mode) ?? d.pops[0];
    const p = mp?.populations.find((x) => x.key === population);
    if (!mp || !p) throw new Error(`no population ${population} in ${mode ?? mp?.mode}`);
    console.log(json ? JSON.stringify(p, null, 1) : populationMarkdown(mp, p));
    return;
  }
  if (marketOnly) {
    console.log(json ? JSON.stringify(r.modes.map((m) => ({ mode: m.mode, peers: m.marketPeers, rows: m.market })), null, 1) : marketMarkdown(r, mode));
    return;
  }
  console.log(json ? JSON.stringify(r, null, 1) : toMarkdown(r, { mode, context, market: !noMarket }));
}
if (process.argv[1] && /slotlab-report(\.mjs)?$/.test(process.argv[1])) {
  main(process.argv.slice(2)).catch((e) => {
    console.error(String(e?.message ?? e));
    process.exit(1);
  });
}
export {
  DEFAULT_BASE,
  buildReport,
  fetchGame,
  games,
  isLenient,
  main,
  marketMarkdown,
  populationMarkdown,
  reach,
  reload,
  report,
  toMarkdown,
  version,
  waitImported
};
