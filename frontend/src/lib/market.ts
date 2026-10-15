/**
 * Market — types of `/api/ui/games/{name}/market` (cf. `slotlab-core/src/market.rs`)
 * and the words that read it: every mode of a game against the modes of the
 * same kind of the slots the market plays.
 */

import { fx, pct } from './populations';
import { fmtPct } from './api';

export type MarketKind = 'base' | 'ante' | 'buy' | 'bust';

export interface MarketRow {
  key: string;
  value: number;
  p10: number;
  p25: number;
  median: number;
  p75: number;
  p90: number;
  /** Share of the peers under the value. */
  rank: number;
}

export interface MarketPeerRow {
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
  median: number;
  p1: number;
  share: number;
}

export interface ModeMarket {
  mode: string;
  cost: number;
  kind: MarketKind;
  max: number;
  by: 'max' | 'cost' | 'cost+max';
  window: [number, number];
  costs: [number, number];
  count: number;
  rows: MarketRow[];
  peers: MarketPeerRow[];
}

export interface GameMarket {
  source: string;
  rule: string;
  fetched: string;
  coverage: number;
  size: number;
  slots: number;
  modes: ModeMarket[];
  alone: string[];
}

const n0 = (v: number) => Math.round(v).toLocaleString('en-US');

/** « 1 in 4,984 », « 1 in 3.82M ». */
export function odds(p: number): string {
  if (!(p > 0)) return 'never';
  const v = 1 / p;
  if (v >= 1e6) return `1 in ${(v / 1e6).toPrecision(3)}M`;
  if (v >= 1e5) return `1 in ${Math.round(v / 1e3)}k`;
  return `1 in ${n0(v)}`;
}

export const KIND_LABEL: Record<MarketKind, string> = {
  base: 'base game',
  ante: 'double chance (ante)',
  buy: 'bonus buy',
  bust: 'all-or-nothing buy',
};

/** How a figure reads, and how its strip is scaled. */
export interface FigureSpec {
  label: (m: ModeMarket) => string;
  format: (v: number) => string;
  /** `log`: odds, drawn on a log scale; `linear` otherwise. */
  scale: 'log' | 'linear';
}

const stakeWord = (m: ModeMarket) => (m.kind === 'buy' || m.kind === 'bust' ? 'the price' : 'the stake');
const times = (v: number) => `${v < 10 ? v.toFixed(2) : n0(v)}×`;

export const FIGURES: Record<string, FigureSpec> = {
  rtp: { label: () => 'RTP', format: (v) => fmtPct(v, 2), scale: 'linear' },
  hit: { label: () => 'Hit rate', format: pct, scale: 'linear' },
  zero: { label: () => 'Rounds paying nothing', format: pct, scale: 'linear' },
  winsUnderStake: { label: () => 'Wins under the stake (of the wins)', format: pct, scale: 'linear' },
  p1: { label: (m) => (m.kind === 'buy' || m.kind === 'bust' ? 'Pays the price back' : 'Wins ≥ the stake'), format: pct, scale: 'linear' },
  p2: { label: (m) => `≥ 2× ${stakeWord(m)}`, format: pct, scale: 'linear' },
  p5: { label: (m) => `≥ 5× ${stakeWord(m)}`, format: odds, scale: 'log' },
  p10: { label: (m) => `≥ 10× ${stakeWord(m)}`, format: odds, scale: 'log' },
  p50: { label: (m) => `≥ 50× ${stakeWord(m)}`, format: odds, scale: 'log' },
  p100: { label: (m) => `≥ 100× ${stakeWord(m)}`, format: odds, scale: 'log' },
  p500: { label: (m) => `≥ 500× ${stakeWord(m)}`, format: odds, scale: 'log' },
  p1000: { label: (m) => `≥ 1,000× ${stakeWord(m)}`, format: odds, scale: 'log' },
  p5000: { label: (m) => `≥ 5,000× ${stakeWord(m)}`, format: odds, scale: 'log' },
  p10000: { label: (m) => `≥ 10,000× ${stakeWord(m)}`, format: odds, scale: 'log' },
  pmax: { label: (m) => `Max win (${fx(m.max)})`, format: odds, scale: 'log' },
  never500: { label: (m) => `6,000 ${m.kind === 'ante' ? 'rounds' : 'spins'} without 500× ${stakeWord(m)}`, format: pct, scale: 'linear' },
  never1000: { label: (m) => `6,000 ${m.kind === 'ante' ? 'rounds' : 'spins'} without 1,000× ${stakeWord(m)}`, format: pct, scale: 'linear' },
  median: { label: (m) => `Median return (× ${stakeWord(m)})`, format: times, scale: 'linear' },
  q25: { label: () => 'Return, p25', format: times, scale: 'linear' },
  q75: { label: () => 'Return, p75', format: times, scale: 'linear' },
  q90: { label: () => 'Return, p90', format: times, scale: 'linear' },
  crumbs: { label: (m) => `Under 25 % of ${stakeWord(m)}`, format: pct, scale: 'linear' },
  nearZero: { label: (m) => `Under 10 % of ${stakeWord(m)}`, format: pct, scale: 'linear' },
  neverRefunded: { label: () => '5 purchases in a row, none paying the price back', format: pct, scale: 'linear' },
  vs10: { label: (m) => `RTP carried by wins ≥ 10× ${stakeWord(m)}`, format: pct, scale: 'linear' },
  vs100: { label: (m) => `RTP carried by wins ≥ 100× ${stakeWord(m)}`, format: pct, scale: 'linear' },
  vs1000: { label: (m) => `RTP carried by wins ≥ 1,000× ${stakeWord(m)}`, format: pct, scale: 'linear' },
  vol: { label: (m) => `Volatility (std ÷ ${m.kind === 'buy' || m.kind === 'bust' ? 'price' : 'stake'})`, format: (v) => v.toFixed(1), scale: 'linear' },
  top1: { label: () => 'Value carried by the top 1 % of rounds', format: pct, scale: 'linear' },
  s100Median: { label: (m) => `100 ${m.kind === 'ante' ? 'rounds' : 'spins'}: median return (of 100 ${stakeWord(m)}s)`, format: (v) => `${v.toFixed(0)}`, scale: 'linear' },
  s100Win: { label: (m) => `100 ${m.kind === 'ante' ? 'rounds' : 'spins'}: sessions ending ahead`, format: pct, scale: 'linear' },
  s100Bust: { label: (m) => `100 ${m.kind === 'ante' ? 'rounds' : 'spins'}: sessions giving back under 20`, format: pct, scale: 'linear' },
};

/** The rows of a mode, by key. */
export function rowOf(m: ModeMarket | undefined, key: string): MarketRow | undefined {
  return m?.rows.find((r) => r.key === key);
}

/** « 20 · 36 · 74 » — p25, median, p75 of the peers, in the figure's format. */
export function quartilesText(r: MarketRow, format: (v: number) => string): string {
  return `${format(r.p25)} · ${format(r.median)} · ${format(r.p75)}`;
}

/** Who the peers are, in a sentence. */
export function peersSentence(m: ModeMarket): string {
  const maxes = `max win ${fx(m.window[0])}–${fx(m.window[1])}`;
  const costs = m.costs[0] === m.costs[1] ? `${m.costs[0]}×` : `${m.costs[0]}×–${m.costs[1]}×`;
  // Chosen by cost, then by max win when plenty share it.
  const near = m.by === 'cost+max' ? `, the closest max wins (${fx(m.window[0])}–${fx(m.window[1])})` : '';
  switch (m.kind) {
    case 'base':
      return `${m.count} base games with a ${maxes}`;
    case 'ante':
      return `${m.count} double-chance modes costing ${costs}${near}`;
    case 'buy':
      return `${m.count} bonus buys costing ${costs}${near}`;
    case 'bust':
      return `${m.count} all-or-nothing buys costing ${costs}${near}`;
  }
}

export const inMiddleRow = (r: MarketRow, v = r.value) => v >= Math.min(r.p25, r.p75) && v <= Math.max(r.p25, r.p75);

/** « more often than 83 % of them », « middle half ». */
export function position(r: MarketRow, spec: FigureSpec): string {
  if (inMiddleRow(r)) return 'middle half';
  const above = r.rank >= 0.5;
  const share = Math.round((above ? r.rank : 1 - r.rank) * 100);
  if (spec.scale === 'log') return above ? `more often than ${share} %` : `rarer than ${share} %`;
  return above ? `higher than ${share} %` : `lower than ${share} %`;
}
