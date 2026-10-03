// What an entry board SHOWS, judged like the game would judge it — run live
// while the Board tab paints. An entry board that displays a paying connection
// nobody is paid for, or a symbol on a reel it can never land on, is exactly
// what a review flags; the page must say so before the file is written.
//
// Coordinates are board coordinates: `[reel, row]`, row 0 and the last row of
// each reel being the padding rows outside the frame.

import type { BoardCell, BoardInfo, BoardSymbol } from './api';

export type Pos = [number, number];
export type WinType = 'lines' | 'ways' | 'cluster';
export type SymMode = 'none' | 'rot' | 'mirror' | 'flip' | 'diag' | 'anti';

export const SYM_LABELS: Record<SymMode, string> = {
  none: 'Free',
  rot: 'Rotation 180°',
  mirror: 'Mirror ↔',
  flip: 'Mirror ↕',
  diag: 'Diagonal ↘',
  anti: 'Diagonal ↙',
};

/** The same modes spelled out — the arrows above read badly in a screen reader. */
export const SYM_ARIA: Record<SymMode, string> = {
  none: 'Free (no symmetry)',
  rot: 'Rotation 180 degrees',
  mirror: 'Mirror left–right',
  flip: 'Mirror top–bottom',
  diag: 'Diagonal, top-left to bottom-right',
  anti: 'Diagonal, top-right to bottom-left',
};

export interface Rules {
  info: BoardInfo;
  type: WinType;
  bySym: Map<string, BoardSymbol>;
  wild: Set<string>;
  scatter: Set<string>;
  /** Symbols with pays of their own, in palette order. */
  paying: string[];
  /** Anything that is not a plain paying symbol (wilds, scatters, bombs, fish…). */
  special: Set<string>;
  /** Every symbol at least one reel strip carries. */
  onStrips: Set<string>;
}

export interface Win {
  label: string;
  pay: number;
  cells: Pos[];
}

export interface Issue {
  level: 'fail' | 'warn' | 'info' | 'pass';
  title: string;
  detail?: string;
  cells?: Pos[];
}

const natural = (a: string, b: string) =>
  a.localeCompare(b, undefined, { numeric: true, sensitivity: 'base' });

/** Lines when the config has paylines; cluster when a pay needs more symbols than there are reels. */
export function winTypeOf(info: BoardInfo): WinType {
  if (info.paylines && info.paylines.length > 0) return 'lines';
  const maxKind = Math.max(0, ...info.symbols.flatMap((s) => s.pays.map((p) => p.max)));
  return maxKind > info.numReels ? 'cluster' : 'ways';
}

export function makeRules(info: BoardInfo): Rules {
  const bySym = new Map(info.symbols.map((s) => [s.name, s]));
  const wild = new Set(info.symbols.filter((s) => s.special.includes('wild')).map((s) => s.name));
  const scatter = new Set(
    info.symbols.filter((s) => s.special.includes('scatter')).map((s) => s.name),
  );
  const paying = info.symbols
    .filter((s) => s.pays.length > 0 && !wild.has(s.name) && !scatter.has(s.name))
    .map((s) => s.name)
    .sort(natural);
  const special = new Set(
    info.symbols.filter((s) => !paying.includes(s.name)).map((s) => s.name),
  );
  const onStrips = new Set((info.stripSymbols ?? []).flat());
  return { info, type: winTypeOf(info), bySym, wild, scatter, paying, special, onStrips };
}

/** Pay for `n` of a symbol; past the top of its table, the top pay. */
export function payFor(sym: BoardSymbol | undefined, n: number): number {
  if (!sym || sym.pays.length === 0) return 0;
  for (const p of sym.pays) if (n >= p.min && n <= p.max) return p.pay;
  const top = sym.pays[sym.pays.length - 1];
  return n > top.max ? top.pay : 0;
}

const round2 = (x: number) => Math.round(x * 100) / 100;
const visibleRows = (reel: BoardCell[]) => Array.from({ length: reel.length - 2 }, (_, i) => i + 1);

/**
 * Ways, left to right: each reel must hold the symbol or a wild. The symbol has
 * to appear for real at least once — three reels of wilds alone pay nothing
 * (otherwise every symbol of the paytable would be paid at once).
 */
function waysWins(b: BoardCell[][], R: Rules): Win[] {
  const out: Win[] = [];
  for (const s of R.paying) {
    const counts: number[] = [];
    const cells: Pos[] = [];
    let real = false;
    for (let c = 0; c < b.length; c++) {
      const col: Pos[] = [];
      let own = false;
      for (const r of visibleRows(b[c])) {
        const n = b[c][r].name;
        if (n === s) {
          own = true;
          col.push([c, r]);
        } else if (R.wild.has(n)) col.push([c, r]);
      }
      if (col.length === 0) break;
      if (own) real = true;
      counts.push(col.length);
      cells.push(...col);
    }
    const kind = counts.length;
    const pay = payFor(R.bySym.get(s), kind);
    if (!real || pay <= 0) continue;
    const ways = counts.reduce((a, x) => a * x, 1);
    out.push({
      label: `${s} on ${kind} reels · ${ways} way${ways > 1 ? 's' : ''}`,
      pay: round2(pay * ways),
      cells,
    });
  }
  return out;
}

/** Paylines, left to right; the first non-wild symbol of the line decides. */
function linesWins(b: BoardCell[][], R: Rules): Win[] {
  const out: Win[] = [];
  (R.info.paylines ?? []).forEach((line, li) => {
    if (line.length !== b.length) return;
    const names = line.map((row, c) => b[c][row + 1]?.name ?? '');
    const run = (ok: (n: string) => boolean) => {
      let n = 0;
      for (const x of names) {
        if (!ok(x)) break;
        n++;
      }
      return n;
    };
    const cellsOf = (n: number): Pos[] => line.slice(0, n).map((row, c) => [c, row + 1]);
    let best: Win | null = null;
    const base = names.find((n) => !R.wild.has(n));
    if (base && R.paying.includes(base)) {
      const n = run((x) => x === base || R.wild.has(x));
      const pay = payFor(R.bySym.get(base), n);
      if (pay > 0) best = { label: `${base} ×${n} on line ${li + 1}`, pay, cells: cellsOf(n) };
    }
    const first = names[0];
    if (R.wild.has(first) && (R.bySym.get(first)?.pays.length ?? 0) > 0) {
      const n = run((x) => R.wild.has(x));
      const pay = payFor(R.bySym.get(first), n);
      if (pay > 0 && (!best || pay > best.pay)) {
        best = { label: `${first} ×${n} on line ${li + 1}`, pay, cells: cellsOf(n) };
      }
    }
    if (best) out.push(best);
  });
  return out;
}

/** Clusters: orthogonal neighbours of one symbol, wilds joining any cluster. */
function clusterWins(b: BoardCell[][], R: Rules): Win[] {
  const out: Win[] = [];
  const inside = (c: number, r: number) => c >= 0 && c < b.length && r >= 1 && r < b[c].length - 1;
  for (const s of R.paying) {
    const seen = new Set<string>();
    for (let c = 0; c < b.length; c++) {
      for (const r of visibleRows(b[c])) {
        if (b[c][r].name !== s || seen.has(`${c},${r}`)) continue;
        const local = new Set([`${c},${r}`]);
        const stack: Pos[] = [[c, r]];
        const group: Pos[] = [];
        while (stack.length) {
          const [x, y] = stack.pop()!;
          group.push([x, y]);
          for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
            const nx = x + dx;
            const ny = y + dy;
            const k = `${nx},${ny}`;
            if (!inside(nx, ny) || local.has(k)) continue;
            const n = b[nx][ny].name;
            if (n === s || R.wild.has(n)) {
              local.add(k);
              stack.push([nx, ny]);
            }
          }
        }
        for (const [x, y] of group) if (b[x][y].name === s) seen.add(`${x},${y}`);
        const pay = payFor(R.bySym.get(s), group.length);
        if (pay > 0) out.push({ label: `${s} cluster of ${group.length}`, pay, cells: group });
      }
    }
  }
  return out;
}

/**
 * Scatters with a pay of their own pay by count, anywhere in the frame. A wild
 * that is also a scatter pays through the lines/ways it completes instead.
 */
function scatterWins(b: BoardCell[][], R: Rules): Win[] {
  const out: Win[] = [];
  for (const s of R.scatter) {
    if (R.wild.has(s)) continue;
    const cells: Pos[] = [];
    b.forEach((reel, c) => visibleRows(reel).forEach((r) => reel[r].name === s && cells.push([c, r])));
    const pay = payFor(R.bySym.get(s), cells.length);
    if (pay > 0) out.push({ label: `${s} ×${cells.length} scatter`, pay, cells });
  }
  return out;
}

export function findWins(b: BoardCell[][], R: Rules): Win[] {
  const main =
    R.type === 'lines' ? linesWins(b, R) : R.type === 'cluster' ? clusterWins(b, R) : waysWins(b, R);
  return [...main, ...scatterWins(b, R)];
}

/** Symbols on reels no strip gives them, specials in the padding, unknown placements. */
export function placementIssues(b: BoardCell[][], R: Rules): Issue[] {
  const out: Issue[] = [];
  const strips = R.info.stripSymbols;
  if (strips) {
    const bySym = new Map<string, Pos[]>();
    b.forEach((reel, c) =>
      reel.forEach((cell, r) => {
        if (R.onStrips.has(cell.name) && !strips[c]?.includes(cell.name)) {
          bySym.set(cell.name, [...(bySym.get(cell.name) ?? []), [c, r]]);
        }
      }),
    );
    for (const [sym, cells] of bySym) {
      const reels = [...new Set(cells.map(([c]) => c + 1))].join(', ');
      const allowed = strips
        .map((s, c) => (s.includes(sym) ? c + 1 : 0))
        .filter(Boolean)
        .join(', ');
      out.push({
        level: 'fail',
        title: `${sym} cannot land on reel ${reels}`,
        detail: `No reel strip carries it there (${sym} lands on reels ${allowed}).`,
        cells,
      });
    }
  }
  const padSpecial: Pos[] = [];
  b.forEach((reel, c) => {
    for (const r of [0, reel.length - 1]) if (R.special.has(reel[r].name)) padSpecial.push([c, r]);
  });
  if (padSpecial.length) {
    out.push({
      level: 'warn',
      title: 'Special symbol in a padding row',
      detail: 'Padding rows sit outside the frame and scroll in at the first spin.',
      cells: padSpecial,
    });
  }
  if (strips) {
    const used = new Set(b.flat().map((c) => c.name));
    const off = [...used].filter((n) => !R.onStrips.has(n)).sort(natural);
    if (off.length) {
      out.push({
        level: 'info',
        title: `Not on any reel strip: ${off.join(', ')}`,
        detail: 'Placed by game code — their placement is not checked here.',
      });
    }
  }
  return out;
}

/** The cell the symmetry pairs with `[c, r]`, or null (axis cell, or no such mode here). */
export function partner(mode: SymMode, c: number, r: number, b: BoardCell[][]): Pos | null {
  const R = b.length;
  const len = (x: number) => b[x]?.length ?? 0;
  const square = b.every((reel) => reel.length === R);
  switch (mode) {
    case 'rot': {
      const c2 = R - 1 - c;
      const r2 = len(c2) - 1 - r;
      return r2 >= 0 && r2 < len(c2) ? [c2, r2] : null;
    }
    case 'mirror': {
      const c2 = R - 1 - c;
      return r < len(c2) ? [c2, r] : null;
    }
    case 'flip':
      return [c, len(c) - 1 - r];
    case 'diag':
      return square ? [r, c] : null;
    case 'anti':
      return square ? [R - 1 - r, R - 1 - c] : null;
    default:
      return null;
  }
}

export function availableModes(b: BoardCell[][]): SymMode[] {
  const square = b.length > 0 && b.every((reel) => reel.length === b.length);
  return square ? ['none', 'rot', 'mirror', 'flip', 'diag', 'anti'] : ['none', 'rot', 'mirror', 'flip'];
}

/** Stable JSON: keys sorted, so two equal symbols compare equal whatever their key order. */
export function canon(x: unknown): string {
  return JSON.stringify(x, (_k, v) =>
    v && typeof v === 'object' && !Array.isArray(v)
      ? Object.fromEntries(Object.entries(v).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)))
      : v,
  );
}

export const sameCell = (a: BoardCell, b: BoardCell) => canon(a) === canon(b);

/** Pairs of cells the symmetry says should match but do not. */
export function mismatches(mode: SymMode, b: BoardCell[][]): number {
  let n = 0;
  b.forEach((reel, c) =>
    reel.forEach((cell, r) => {
      const p = partner(mode, c, r, b);
      if (!p) return;
      const [c2, r2] = p;
      if (c * 1000 + r < c2 * 1000 + r2 && !sameCell(cell, b[c2][r2])) n++;
    }),
  );
  return n;
}
