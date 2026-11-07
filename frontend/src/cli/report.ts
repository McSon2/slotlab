/**
 * SlotLab as seen by an agent — the same findings, numbers and market
 * comparison as the Analyze tab, read over the local API and written as
 * Markdown or JSON. Bundled by `pnpm --dir frontend build:cli` into
 * `scripts/slotlab-report.mjs`, used by the CLI and the MCP server
 * (`scripts/slotlab-mcp.mjs`) so Claude Code checks a math by itself.
 *
 * The rules are `lib/findings.ts`, untouched: what the agent reads is what the
 * tab shows, down to the sentence.
 */
import { allFindings, issuesOf, SEVERITIES, type Issue, type Severity } from '../lib/findings';
import { splitIssues } from '../lib/muted';
import { fx, pct, oneIn, type ModePopulations, type Population } from '../lib/populations';
import { FIGURES, position, inMiddleRow, type GameMarket, type ModeMarket } from '../lib/market';
import { fmtPct, type MuteEntry, type ModeStats, type GameCheckReport, type TiersReport, type GameVersion, type GameSummary, type SessionReach } from '../lib/api';

// Node only (the frontend never imports this file): the few globals it uses, untyped here.
declare const process: { env: Record<string, string | undefined>; argv: string[]; exit(code?: number): never };

export const DEFAULT_BASE = process.env.SLOTLAB_URL ?? 'http://localhost:3600';

async function get<T>(base: string, path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(base + path, { headers: { 'Content-Type': 'application/json' }, ...init });
  const body = await res.json().catch(() => null);
  if (!res.ok) throw new Error(body?.error ?? `${res.status} ${res.statusText} on ${path}`);
  return body as T;
}

const enc = encodeURIComponent;

export interface GameData {
  game: string;
  summary: GameSummary | undefined;
  version: GameVersion;
  stats: { mode: string; stats: ModeStats }[];
  checks: GameCheckReport;
  pops: ModePopulations[];
  tiers: TiersReport | null;
  market: GameMarket | null;
  /** Findings muted for this game in the UI (`/api/ui/games/{name}/muted`). */
  muted?: MuteEntry[];
}

export const games = (base = DEFAULT_BASE) => get<GameSummary[]>(base, '/api/ui/games');
export const version = (game: string, base = DEFAULT_BASE) => get<GameVersion>(base, `/api/ui/games/${enc(game)}/version`);
export const reload = (game: string, base = DEFAULT_BASE) => get<{ ok: boolean }>(base, `/api/ui/games/${enc(game)}/reload`, { method: 'POST' });
export const reach = (game: string, mode: string, spins: number, x: number, base = DEFAULT_BASE) =>
  get<SessionReach>(base, `/api/ui/games/${enc(game)}/reach?mode=${enc(mode)}&spins=${spins}&x=${x}`);

/** Waits until the books of the game are imported (a fresh publish re-imports them). */
export async function waitImported(game: string, base = DEFAULT_BASE, timeoutMs = 20 * 60_000): Promise<GameVersion> {
  const t0 = Date.now();
  for (;;) {
    const v = await version(game, base);
    if (v.imported && !v.sourceChanged) return v;
    if (Date.now() - t0 > timeoutMs) throw new Error(`${game}: books still not imported after ${Math.round(timeoutMs / 60000)} min`);
    await new Promise((r) => setTimeout(r, 5000));
  }
}

export async function fetchGame(game: string, base = DEFAULT_BASE): Promise<GameData> {
  const [list, v, s, checks, p, tiers, market, muted] = await Promise.all([
    games(base).catch(() => [] as GameSummary[]),
    version(game, base),
    get<{ modes: { mode: string; stats: ModeStats }[] }>(base, `/api/ui/games/${enc(game)}/stats`),
    get<GameCheckReport>(base, `/api/ui/games/${enc(game)}/checks`),
    get<{ modes: ModePopulations[] }>(base, `/api/ui/games/${enc(game)}/populations`),
    get<TiersReport>(base, `/api/ui/games/${enc(game)}/tiers`).catch(() => null),
    get<GameMarket>(base, `/api/ui/games/${enc(game)}/market`).catch(() => null),
    get<{ muted: MuteEntry[] }>(base, `/api/ui/games/${enc(game)}/muted`).then((r) => r.muted.map((m) => ({ ...m, mode: m.mode ?? null }))).catch(() => [] as MuteEntry[]),
  ]);
  return { game, summary: list.find((g) => g.name === game), version: v, stats: s.modes, checks, pops: p.modes, tiers, market, muted };
}

/** Degen Lab ships past the 3★ grid with Stake's agreement — the folder says which studio. */
export function isLenient(d: GameData, choice: boolean | 'auto' = 'auto'): boolean {
  if (choice !== 'auto') return choice;
  return /degen[-_ ]?lab/i.test(d.summary?.publishDir ?? '');
}

export interface ModeReport {
  mode: string;
  cost: number;
  rtp: number;
  hitRate: number;
  stdOverCost: number;
  maxWin: number;
  maxWinOneIn: number;
  mostLikelyOneIn: number;
  shortSession: ModeStats['shortSession'];
  nobonus: { rtpShare: number; mean: number; hitRate: number } | null;
  bonus: { oneIn: number; rtpShare: number; median: number; mean: number } | null;
  tiers: { label: string; key: string; oneIn: number; shareOfBonuses: number | null; min: number; median: number; mean: number; p90: number; rtpShare: number; books: number; effective: number }[];
  features: { label: string; oneIn: number; zeroRate: number; median: number; mean: number; rtpShare: number }[];
  /** Reels slowing for the missing scatter on the paid spin (books' first reveal, LUT-weighted). */
  anticipation: { all: number; withoutBonus: number; bonusAnnounced: number | null } | null;
  market: { key: string; label: string; value: string; peers: string; position: string; out: boolean }[];
  marketPeers: string | null;
}

export interface Report {
  game: string;
  readAt: string;
  version: GameVersion;
  lenient: boolean;
  compliant: boolean;
  starTier: number;
  counts: Record<Severity, number>;
  issues: Issue[];
  /** Muted in the UI for this game: out of `issues` and `counts`. */
  muted: Issue[];
  modes: ModeReport[];
}

const q = (p: Population, v: number) => p.quantiles.find((x) => Math.abs(x.q - v) < 1e-9)?.value ?? 0;

export function buildReport(d: GameData, lenient: boolean): Report {
  const tiers = d.tiers?.modes ?? [];
  const split = splitIssues(allFindings({ stats: d.stats, checks: d.checks, pops: d.pops, tiers, market: d.market, lenient }), d.muted ?? []);
  const issues = split.shown;
  const counts = Object.fromEntries(SEVERITIES.map((s) => [s, issues.filter((i) => i.severity === s).length])) as Record<Severity, number>;
  const modes: ModeReport[] = d.stats.map(({ mode, stats: s }) => {
    const mp = d.pops.find((m) => m.mode === mode);
    const pops = mp?.populations ?? [];
    const nb = pops.find((p) => p.kind === 'nobonus');
    const b = pops.find((p) => p.kind === 'bonus');
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
      tiers: pops
        .filter((p) => p.kind === 'tier')
        .map((p) => ({ label: p.label, key: p.key, oneIn: p.oneIn, shareOfBonuses: p.shareOfBonuses, min: p.min, median: p.median, mean: p.mean, p90: q(p, 0.9), rtpShare: p.rtpShare, books: p.drawableBooks, effective: p.effectiveOutcomes })),
      features: pops
        .filter((p) => p.kind === 'criteria' && p.criteria !== 'base game')
        .map((p) => ({ label: p.label, oneIn: p.oneIn, zeroRate: p.zeroRate, median: p.median, mean: p.mean, rtpShare: p.rtpShare })),
      anticipation: mp?.anticipation ?? null,
      market: (mm?.rows ?? [])
        .filter((r) => FIGURES[r.key])
        .map((r) => {
          const spec = FIGURES[r.key];
          return { key: r.key, label: spec.label(mm as ModeMarket), value: spec.format(r.value), peers: `${spec.format(r.p25)} · ${spec.format(r.median)} · ${spec.format(r.p75)}`, position: position(r, spec), out: !inMiddleRow(r) };
        }),
      marketPeers: mm ? `${mm.count} ${mm.kind} peers, max win ${fx(mm.window[0])}–${fx(mm.window[1])}, cost ${mm.costs[0]}×–${mm.costs[1]}×` : null,
    };
  });
  return { game: d.game, readAt: new Date().toISOString(), version: d.version, lenient, compliant: d.checks.passed, starTier: d.checks.starTier, counts, issues, muted: split.muted, modes };
}

const n0 = (v: number) => (isFinite(v) ? Math.round(v).toLocaleString('en-US') : '—');

/** The report as Markdown — what an agent reads, what a human can paste. */
export function toMarkdown(r: Report, opts: { mode?: string; context?: boolean; market?: boolean } = {}): string {
  const L: string[] = [];
  const v = r.version;
  L.push(`# SlotLab — ${r.game}`);
  L.push(`Build read at ${new Date(v.loadedAt).toLocaleTimeString('en-GB')} · build ${v.fingerprint}${v.sourceChanged ? ' · ⚠️ FILES CHANGED ON DISK SINCE (reloading)' : ''}${v.imported ? '' : ' · ⚠️ BOOKS NOT IMPORTED (bonus findings missing)'}`);
  L.push(`Compliance: ${r.compliant ? 'COMPLIANT' : 'FAIL'} · estimated ${r.starTier}★ · 3★ grid ${r.lenient ? 'exceptions agreed for this studio (lenient)' : 'required (strict)'}`);
  L.push('');
  const faults = r.counts.blocking + r.counts.serious + r.counts.minor;
  L.push(`## Worth a look — ${faults === 0 ? 'nothing: no blocking, serious or minor finding' : `${r.counts.blocking} blocking · ${r.counts.serious} serious · ${r.counts.minor} minor`}${opts.context ? ` · ${r.counts.context} context` : ` (${r.counts.context} context hidden)`}${r.muted.length ? ` · ${r.muted.length} muted in the UI (${[...new Set(r.muted.map((i) => i.rule))].join(', ')})` : ''}`);
  const modesShown = [...r.modes.map((m) => m.mode), null].filter((m) => !opts.mode || m === opts.mode || m === null);
  for (const mode of modesShown) {
    const list = issuesOf(r.issues, mode).filter((i) => opts.context || i.severity !== 'context');
    if (!list.length) continue;
    L.push('');
    L.push(`### ${mode ?? 'Whole game'}`);
    for (const it of list) {
      L.push(`- **${it.severity}** — ${it.title} — \`${it.big}\` (${it.ref})`);
      L.push(`  ${it.text}${it.also?.length ? ` Same in ${it.also.join(', ')}.` : ''}`);
    }
  }
  L.push('');
  L.push('## Modes');
  for (const m of r.modes.filter((m) => !opts.mode || m.mode === opts.mode)) {
    L.push('');
    L.push(`### ${m.mode} — ${fx(m.cost)} · RTP ${fmtPct(m.rtp, 2)} · hit ${fmtPct(m.hitRate, 2)} · std÷cost ${m.stdOverCost.toFixed(2)} · max win ${fx(m.maxWin)} ${oneIn(m.maxWinOneIn)} · most likely ${oneIn(m.mostLikelyOneIn)}`);
    if (m.nobonus && m.bonus) L.push(`- RTP split: plain spins ${pct(m.nobonus.rtpShare)} / bonuses ${pct(m.bonus.rtpShare)} · a bonus ${oneIn(m.bonus.oneIn)} (median ${fx(m.bonus.median)}, average ${fx(m.bonus.mean)})`);
    if (m.shortSession) L.push(`- 100 rounds: median return ${n0(m.shortSession.median)} of 100 · ${pct(m.shortSession.win)} of sessions ahead · ${pct(m.shortSession.bust)} give back under 20 · middle half ${n0(m.shortSession.p25)}–${n0(m.shortSession.p75)}`);
    if (m.tiers.length) {
      L.push('- Tiers: | tier | frequency | share of bonuses | min | median | average | P90 | RTP share | books (effective) |');
      L.push('  |---|---|---|---|---|---|---|---|---|');
      for (const t of m.tiers) L.push(`  | ${t.label} | ${oneIn(t.oneIn)} | ${t.shareOfBonuses === null ? '—' : pct(t.shareOfBonuses)} | ${fx(t.min)} | ${fx(t.median)} | ${fx(t.mean)} | ${fx(t.p90)} | ${pct(t.rtpShare)} | ${n0(t.books)} (${n0(t.effective)}) |`);
    }
    for (const f of m.features) L.push(`- Feature ${f.label}: ${oneIn(f.oneIn)} · ${pct(f.zeroRate)} pay nothing · median ${fx(f.median)} · average ${fx(f.mean)} · ${pct(f.rtpShare)} of the RTP`);
    if (m.anticipation) {
      const a = m.anticipation;
      L.push(`- Anticipation (reels slowing for the missing scatter, read on the books): ${oneIn(1 / a.all)} spins · without a bonus (near misses felt, forced or natural) ${a.withoutBonus > 0 ? oneIn(1 / a.withoutBonus) : 'never'}${a.bonusAnnounced !== null ? ` · ${pct(a.bonusAnnounced)} of the bonuses announced` : ''}`);
    }
    if (opts.market !== false && m.market.length) {
      const out = m.market.filter((x) => x.out);
      L.push(`- Market (${m.marketPeers}): ${out.length ? `${out.length} of ${m.market.length} figures outside the middle half` : 'every figure inside the middle half of the peers'}`);
      for (const x of out) L.push(`  - ${x.label}: **${x.value}** — ${x.position} (peers p25 · median · p75: ${x.peers})`);
    }
  }
  return L.join('\n');
}

/** Markdown of one population — percentiles, thresholds, bands. */
export function populationMarkdown(mp: ModePopulations, p: Population): string {
  const L: string[] = [];
  L.push(`### ${mp.mode} › ${p.label} (${p.kind}, key \`${p.key}\`)`);
  L.push(`${p.kind === 'all' ? 'every spin' : oneIn(p.oneIn)} · ${pct(p.probability)} per spin · RTP share ${pct(p.rtpShare)} (${pct(p.rtpContribution)} points) · median ${fx(p.median)} · average ${fx(p.mean)} · min ${fx(p.min)} · max ${fx(p.max)} · 0x ${pct(p.zeroRate)} · ≥ cost ${pct(p.profitableRate)} · top 1 % carries ${pct(p.top1Share)} · ${n0(p.drawableBooks)} books, ${n0(p.effectiveOutcomes)} effective`);
  L.push('Percentiles: ' + p.quantiles.map((x) => `P${Math.round(x.q * 100)} ${fx(x.value)} (#${x.simId})`).join(' · '));
  L.push('Under: ' + p.thresholds.filter((t) => t.below > 0 && t.below < 1).map((t) => `${fx(t.x)} ${pct(t.below)}`).join(' · '));
  L.push('Bands (share · value share · books/effective): ' + p.bands.filter((b) => b.share > 0).map((b) => `${b.start === 0 && b.end === 0 ? '0' : `${b.start}–${b.end ?? '∞'}`}: ${pct(b.share)} · ${pct(b.valueShare)} · ${b.books}/${Math.round(b.effective)}`).join(' · '));
  if (p.retriggers) L.push(`Retriggers: ${pct(p.retriggers.rate)} retrigger, ${p.retriggers.mean.toFixed(2)} on average, max ${p.retriggers.max}`);
  if (p.spins) L.push(`Free spins: ${p.spins.meanInitial.toFixed(2)} awarded, ${p.spins.meanFinal.toFixed(2)} played`);
  return L.join('\n');
}

export function marketMarkdown(r: Report, mode?: string): string {
  const L: string[] = [`# Market — ${r.game}`];
  for (const m of r.modes.filter((m) => !mode || m.mode === mode)) {
    L.push('');
    L.push(`## ${m.mode} (${fx(m.cost)}) — ${m.marketPeers ?? 'no peers'}`);
    L.push('| figure | mode | peers p25 · median · p75 | position |');
    L.push('|---|---|---|---|');
    for (const x of m.market) L.push(`| ${x.label} | ${x.out ? `**${x.value}**` : x.value} | ${x.peers} | ${x.position} |`);
  }
  return L.join('\n');
}

export async function report(game: string, opts: { base?: string; lenient?: boolean | 'auto' } = {}): Promise<Report> {
  const d = await fetchGame(game, opts.base ?? DEFAULT_BASE);
  return buildReport(d, isLenient(d, opts.lenient ?? 'auto'));
}

// ── CLI ──────────────────────────────────────────────────────────────────────

function usage(): never {
  console.error(`usage: slotlab-report <game> [--mode M] [--json] [--context] [--no-market] [--strict|--lenient] [--wait] [--reload] [--population KEY] [--market] [--base URL]
       slotlab-report --list`);
  process.exit(2);
}

export async function main(argv: string[]): Promise<void> {
  const args = [...argv];
  const flag = (name: string) => {
    const i = args.indexOf(name);
    if (i < 0) return undefined;
    const v = args[i + 1];
    args.splice(i, 2);
    return v;
  };
  const has = (name: string) => {
    const i = args.indexOf(name);
    if (i < 0) return false;
    args.splice(i, 1);
    return true;
  };
  const base = flag('--base') ?? DEFAULT_BASE;
  if (has('--list')) {
    const g = await games(base);
    for (const x of g) console.log(`${x.name}\t${x.loaded ? 'loaded' : 'NOT LOADED'}\t${x.imported ? 'imported' : 'not imported'}\t${x.modes.map((m) => `${m.name} ${m.cost}x`).join(', ')}\t${x.publishDir}`);
    return;
  }
  const json = has('--json');
  const context = has('--context');
  const noMarket = has('--no-market');
  const strict = has('--strict');
  const lenient = has('--lenient');
  const wait = has('--wait');
  const doReload = has('--reload');
  const marketOnly = has('--market');
  const mode = flag('--mode');
  const population = flag('--population');
  const game = args.find((a) => !a.startsWith('--'));
  if (!game) usage();
  if (doReload) await reload(game, base);
  if (wait || doReload) await waitImported(game, base);
  const d = await fetchGame(game, base);
  const r = buildReport(d, isLenient(d, strict ? false : lenient ? true : 'auto'));
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
