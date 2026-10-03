import type { ModePopulations } from './populations';

export interface ModeSummary {
  name: string;
  cost: number;
  aliases: string[];
  rtp: number;
  hitRate: number;
  maxPayout: number;
  outcomeCount: number;
}

export interface GameSummary {
  name: string;
  publishDir: string;
  frontUrl: string;
  /** Auto-detected build, served by SlotLab under /games/<name>/. */
  buildDir?: string;
  servedUrl?: string;
  loaded: boolean;
  imported: boolean;
  error?: string;
  modes: ModeSummary[];
}

export interface Check {
  id: string;
  label: string;
  status: 'pass' | 'warn' | 'fail' | 'info';
  value: string;
  threshold: string;
  details?: string;
}

export interface ModeCompliance {
  mode: string;
  cost: number;
  passed: number;
  total: number;
  checks: Check[];
}

export interface StarConstraint {
  label: string;
  value: string;
  limit2star: string;
  limit3star: string;
  pass2star: boolean;
  pass3star: boolean;
  computed: boolean;
}

export interface GameCheckReport {
  passed: boolean;
  failCount: number;
  warnCount: number;
  starTier: number;
  starTierPartial: boolean;
  crossModeVariance: number;
  betLevelConstraints: StarConstraint[];
  modes: ModeCompliance[];
  publishChecks: Check[];
}

export interface Bucket {
  start: number;
  end: number;
  count: number;
  weight: number;
  probability: number;
  oneIn: number;
  rtpContribution: number;
}

export interface TopPayout {
  payout: number;
  weight: number;
  oneIn: number;
  count: number;
  simIds: number[];
}

export interface ModeStats {
  cost: number;
  outcomeCount: number;
  totalWeight: number;
  rtp: number;
  hitRate: number;
  zeroRate: number;
  meanPayout: number;
  medianPayout: number;
  minNonzeroPayout: number;
  maxPayout: number;
  variance: number;
  stdDev: number;
  volatility: number;
  costAdjVolatility: number;
  breakevenRate: number;
  cvar001: number;
  etl40x: number;
  etl10kx: number;
  probWin5k: number;
  probWin10k: number;
  uniquePayoutCount: number;
  mostLikelyOneIn: number;
  maxPayoutOneIn: number;
  buckets: Bucket[];
  topPayouts: TopPayout[];
  granularityViolations: number;
}

export interface BonusGroup {
  count: number;
  probability: number;
  oneIn: number;
  avgWin: number;
  rtpContribution: number;
}

export interface BonusTriggerCount extends BonusGroup {
  triggers: number;
}

/**
 * Books à bonus d'un mode, par nombre de retriggers DANS le bonus (règle de
 * `slotlab-core/src/freespins.rs`). `probability` est par spin, comme ailleurs ;
 * `shareOfBonuses` est la part des bonus.
 */
export interface BonusRetriggerCount extends BonusGroup {
  retriggers: number;
  shareOfBonuses: number;
}

/** Un criteria du mode — `slotlab-core/src/provenance.rs`. */
export interface BonusCriteria extends BonusGroup {
  criteria: string;
  booksWithTrigger: number;
  /** Part du RTP du mode. */
  rtpShare: number;
  /** Part (pondérée) de ses books qui déclenchent un bonus. */
  bonusShare: number;
  /** Retriggers moyens de ses bonus ; null s'il n'en déclenche aucun. */
  retriggersMean: number | null;
}

/** Retriggers pondérés des bonus d'un palier. */
export interface RetriggerStats {
  mean: number;
  min: number;
  median: number;
  max: number;
  /** Part des bonus sans aucun retrigger. */
  noneShare: number;
}

/**
 * Un PALIER de bonus : le nombre de symboles qui l'ont déclenché (positions du
 * premier `freeSpinTrigger`), toutes portes d'entrée confondues (`freegame_3`,
 * `wave_3`, un `wincap`…) — aucun nom de criteria n'est interprété.
 * `tier` null : bonus dont le déclenchement ne dit pas ses positions.
 */
export interface BonusTier extends BonusGroup {
  tier: number | null;
  /** Criteria des books du palier, du plus lourd au plus léger. */
  criteria: string[];
  rtpShare: number;
  retriggers: RetriggerStats | null;
}

/** Une tranche de la distribution (mêmes bornes que `Bucket`) et ce qui la porte. */
export interface BucketSplit {
  start: number;
  end: number;
  /** Part du poids de la tranche portée par des books à bonus. */
  bonusShare: number;
  /** Part portée par chaque palier ; leur somme vaut `bonusShare`. */
  tiers: { tier: number | null; share: number }[];
}

/** One value of a mechanic field — cf. `slotlab-core/src/mechanics.rs`. */
export interface MechValue {
  value: string;
  /** Occurrences per round, weighted by the LUT (what players see). */
  perRound: number;
  /** Occurrences per book, unweighted (what the simulation produced). */
  perRoundRaw: number;
  /** Share of rounds where it shows up at least once (LUT). */
  roundsWith: number;
}

export interface MechFacet {
  /** Event type (`sharkFeed`), or `#events` for the count of events by type. */
  event: string;
  /** Path inside the event (`hunts[].fish`); empty for `#events`. */
  path: string;
  values: MechValue[];
  perRound: number;
  perRoundRaw: number;
  roundsWith: number;
  /** Largest share gap between the LUT and the raw books, in points. */
  driftPoints: number;
  /** [occurrences in a round, probability] — rounds without any included. */
  countDist: [number, number][];
}

/** A key stat declared by the game (`slotlab.mechanics.json`), read in the books. */
export interface KeyStatRow {
  value: string;
  label: string;
  /** Share seen in game (LUT), 0-1. */
  seen: number;
  /** Planned share, 0-1. */
  planned: number | null;
  perRound: number;
  roundsWith: number;
}

export interface KeyStat {
  title: string;
  note: string | null;
  scope: 'all' | 'bonus' | 'nobonus';
  perRoundLabel: string | null;
  perRound: number;
  roundsWith: number;
  /** [median, 90th percentile, max] per round. */
  count: [number, number, number] | null;
  rows: KeyStatRow[];
  worstGap: number | null;
  tolerance: number;
  status: 'ok' | 'off' | 'info';
}

export interface ModeMechanics {
  mode: string;
  keyStats: KeyStat[];
  keyStatsFile: string | null;
  keyStatsError: string | null;
  scope: 'all' | 'bonus' | 'nobonus';
  books: number;
  /** Probability of the scope's rounds, on the LUT. */
  probability: number;
  facets: MechFacet[];
}

export interface ModeBonusStats {
  mode: string;
  imported: boolean;
  cost?: number;
  totalBooks?: number;
  booksWithTrigger?: number;
  triggerProbability?: number;
  triggerOneIn?: number;
  avgTriggerWin?: number;
  triggerRtpContribution?: number;
  triggerRtpShare?: number;
  byTriggerCount?: BonusTriggerCount[];
  byRetriggerCount?: BonusRetriggerCount[];
  byCriteria?: BonusCriteria[];
  byTier?: BonusTier[];
  byBucket?: BucketSplit[];
}

/** Asset budget of a game's frontend build — cf. `slotlab-core/src/assets.rs`. */
export interface AssetPage {
  path: string;
  bytes: number;
  width: number;
  height: number;
  vram: number;
}

export interface AssetVariant {
  label: string;
  bytes: number;
  vram: number;
  pages: AssetPage[];
}

/** A set of mutually exclusive files, of which the device loads exactly one. */
export interface AssetGroup {
  kind: 'atlasScale' | 'audioFormat';
  name: string;
  variants: AssetVariant[];
}

export interface AssetItem {
  path: string;
  category: 'code' | 'texture' | 'audio' | 'font' | 'other';
  bytes: number;
  vram: number;
  width: number;
  height: number;
}

export interface AssetBudget {
  download: number;
  vram: number;
}

/** Entry board editor — cf. `slotlab-app/src/board.rs`. */
export interface BoardSymbol {
  name: string;
  /** Pay per kind (cluster games: per cluster size); `min == max` for a single kind. */
  pays: { min: number; max: number; pay: number }[];
  /** `special_properties` of the frontend config: wild, scatter, multiplier… */
  special: string[];
}

/** One symbol of `INITIAL_BOARD`, extra fields (multiplier, value…) included. */
export type BoardCell = { name: string; [key: string]: unknown };

export interface BoardInfo {
  game: string;
  constantsPath: string;
  baseHash: string;
  /** `board[reel][row]`, padding rows included (first and last of each reel). */
  board: BoardCell[][];
  consts: { name: string; value: BoardCell }[];
  numReels: number;
  numRows: number[];
  symbols: BoardSymbol[];
  paylines: number[][] | null;
  stripSymbols: string[][] | null;
  stripFiles: string[];
  images: Record<string, string>;
  notes: string[];
}

export interface AssetReport {
  buildDir: string;
  fileCount: number;
  diskBytes: number;
  /** Lightest configuration a device can land on. */
  min: AssetBudget;
  /** Heaviest configuration. */
  max: AssetBudget;
  groups: AssetGroup[];
  items: AssetItem[];
  categories: { category: string; label: string; bytes: number; vram: number }[];
  warnings: { path: string; message: string; vram: number }[];
}


// ---- Celebration tiers (win levels measured on the published LUT) ----

export interface TierSpec {
  names: string[];
  thresholds: number[];
  durations: number[];
  firstPlaque: number;
}

export interface TierRow {
  level: number;
  name: string;
  threshold: number;
  atLeast: number;
  atLeastOneIn: number;
  exactly: number;
  exactlyOneIn: number;
  secondsPerSpin: number;
  duration: number;
  plaque: boolean;
  rtpContribution: number;
}

export interface ModeTiers {
  mode: string;
  cost: number;
  source: string;
  rows: TierRow[];
  plaqueRate: number;
  plaqueOneIn: number;
  secondsPerSpin: number;
  plaqueSecondsPerSpin: number;
  warnings: string[];
}

export interface TiersReport {
  game: string;
  maxWin: number;
  ladderSource: string;
  modes: { report: ModeTiers; spec: TierSpec }[];
}


// ---- Preflight (the pre-upload checklist, run against the files) ----

export type PreflightScope =
  | 'appSrc'
  | 'appRoot'
  | 'packages'
  | 'build'
  | 'publish';

export interface PreflightHit {
  file: string;
  line: number;
  text: string;
}

export interface PreflightFinding {
  id: string;
  label: string;
  explain: string;
  doc: string;
  severity: 'fail' | 'warn' | 'info';
  status: 'pass' | 'warn' | 'fail' | 'skip';
  scope: PreflightScope;
  detail: string;
  hits: PreflightHit[];
  extraHits: number;
}

export interface PreflightReport {
  game: string;
  appDir?: string;
  buildDir?: string;
  failCount: number;
  warnCount: number;
  skipCount: number;
  passed: boolean;
  findings: PreflightFinding[];
}


// ---- Fault injection (the states a reviewer reaches and a dev cannot) ----

export type FaultEndpoint =
  | 'authenticate'
  | 'balance'
  | 'play'
  | 'endRound'
  | 'event'
  | 'replay';

export interface FailureRule {
  endpoint: FaultEndpoint;
  kind: 'rgsError' | 'http500' | 'hang';
  code: string;
  remaining: number;
  limited: boolean;
}

export interface Faults {
  latencyMs: number;
  keepRoundOpen: boolean;
  keepRoundOpenOnce: boolean;
  failures: FailureRule[];
}

export const EMPTY_FAULTS: Faults = {
  latencyMs: 0,
  keepRoundOpen: false,
  keepRoundOpenOnce: false,
  failures: [],
};


// ---- Book scan: staging invariants, weighted by the LUT ----

export interface ScanShape {
  triggerScatters: number;
  retriggerScatters: number;
  visibleRows: number;
  boardRows: number;
  reels: number;
  pad: number;
  derived: boolean;
}

export interface ScanTimings {
  reveal: number;
  anticipationStep: number;
  tumble: number;
  win: number;
  bonusIntro: number;
  bonusOutro: number;
}

export interface ScanParams {
  triggerScatters: number;
  retriggerScatters: number;
  visibleRows: number;
  maxBaseScatters: number;
  timings: ScanTimings;
}

export interface ScanGroup {
  books: number;
  weight: number;
  probability: number;
  oneIn: number;
  avgPayout: number;
}

export interface ScanEvent extends ScanGroup {
  name: string;
  perSpin: number;
}

export interface ScanBucket extends ScanGroup {
  value: number;
}

export interface ScanReport {
  mode: string;
  cost: number;
  shape: ScanShape;
  params: ScanParams;
  books: number;
  totalWeight: number;
  events: ScanEvent[];
  anticipation: {
    emitted: number;
    emittedOneIn: number;
    deserved: number;
    deservedOneIn: number;
    missing: number;
    early: number;
    late: number;
    spurious: number;
    byStartReel: ScanBucket[];
  };
  scattersBase: ScanBucket[];
  scattersFree: ScanBucket[];
  bonusTiers: ScanBucket[];
  tiersMonotonic: boolean;
  systematicAnticipation: boolean;
  integrity: {
    tierBoardMismatch: number;
    tierCriteriaMismatch: number;
    wildMovesOffgrid: number;
    revealsWithoutBoard: number;
    overCap: number;
    unweightedBooks: number;
    parseErrors: number;
  };
  retriggers: ScanBucket[];
  wildMovesPerSpin: number;
  pacing: { mean: number; p50: number; p95: number };
  warnings: string[];
}

export interface ScanJob {
  state: 'running' | 'done' | 'failed';
  mode: string;
  booksDone: number;
  booksTotal: number;
  error: string | null;
}

export const DEFAULT_TIMINGS: ScanTimings = {
  reveal: 1.2,
  anticipationStep: 1.1,
  tumble: 0.45,
  win: 0.3,
  bonusIntro: 2,
  bonusOutro: 2.5,
};


// ---- Build history: what a `run.py prod` changed ----

export interface ModeSnapshot {
  name: string;
  cost: number;
  rtp: number;
  hitRate: number;
  zeroRate: number;
  volatility: number;
  maxPayout: number;
  maxPayoutOneIn: number;
  mostLikelyOneIn: number;
  outcomes: number;
  totalWeight: number;
}

export interface Snapshot {
  fingerprint: number;
  takenAt: number;
  modes: ModeSnapshot[];
}

export interface HistoryChange {
  field: string;
  label: string;
  before: number;
  after: number;
  delta: number;
  notable: boolean;
}

export interface ModeDiff {
  mode: string;
  status: 'added' | 'removed' | 'changed';
  changes: HistoryChange[];
}

export interface HistoryReport {
  game: string;
  snapshots: Snapshot[];
  diff: ModeDiff[];
  current: number;
}

async function request<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  });
  if (!res.ok) {
    let msg = `${res.status} ${res.statusText}`;
    try {
      const body = await res.json();
      if (body.error) msg = body.error;
    } catch {
      /* non-JSON body */
    }
    throw new Error(msg);
  }
  return res.json();
}

/** Best win of a session — `slotlab-core/src/reach.rs`. */
export interface SessionReach {
  spins: number;
  /** Threshold, in multiples of the base bet. */
  x: number;
  /** Share of spins paying at least `x`. */
  p: number;
  /** One spin in … pays at least `x` (null: none does). */
  oneIn: number | null;
  /** Spins paying at least `x`, per session on average. */
  expected: number;
  /** Share of sessions that never pay `x` or more. */
  never: number;
}

export const api = {
  listGames: () => request<GameSummary[]>('/api/ui/games'),
  registerGame: (name: string, publishDir: string, frontUrl: string) =>
    request<{ ok: boolean; modes: string[] }>('/api/ui/games', {
      method: 'POST',
      body: JSON.stringify({ name, publishDir, frontUrl }),
    }),
  removeGame: (name: string) =>
    request<{ ok: boolean }>(`/api/ui/games/${encodeURIComponent(name)}`, {
      method: 'DELETE',
    }),
  reloadGame: (name: string) =>
    request<{ ok: boolean }>(
      `/api/ui/games/${encodeURIComponent(name)}/reload`,
      { method: 'POST' },
    ),
  updateFrontUrl: (name: string, frontUrl: string) =>
    request<{ ok: boolean }>(`/api/ui/games/${encodeURIComponent(name)}`, {
      method: 'PATCH',
      body: JSON.stringify({ frontUrl }),
    }),
  gameStats: (name: string) =>
    request<{ game: string; modes: { mode: string; stats: ModeStats }[] }>(
      `/api/ui/games/${encodeURIComponent(name)}/stats`,
    ),
  bonusStats: (name: string) =>
    request<{ game: string; modes: ModeBonusStats[] }>(
      `/api/ui/games/${encodeURIComponent(name)}/bonus-stats`,
    ),
  /** Mechanic fields of a mode's events, weighted by the LUT. */
  mechanics: (name: string, mode: string, scope: string) =>
    request<ModeMechanics>(
      `/api/ui/games/${encodeURIComponent(name)}/mechanics?mode=${encodeURIComponent(mode)}&scope=${scope}`,
    ),
  /** Every population of every mode — cf. `slotlab-core/src/population.rs`. */
  populations: (name: string, thresholds: number[] = []) =>
    request<{ game: string; modes: ModePopulations[] }>(
      `/api/ui/games/${encodeURIComponent(name)}/populations` +
        (thresholds.length ? `?thresholds=${thresholds.join(',')}` : ''),
    ),
  /** « Sur N spins, combien de sessions n'atteignent jamais X ? » — exact, sur la LUT. */
  sessionReach: (name: string, mode: string, spins: number, x: number) =>
    request<SessionReach>(
      `/api/ui/games/${encodeURIComponent(name)}/reach?mode=${encodeURIComponent(mode)}&spins=${spins}&x=${x}`,
    ),
  gameAssets: (name: string) =>
    request<AssetReport>(`/api/ui/games/${encodeURIComponent(name)}/assets`),
  gameBoard: (name: string) =>
    request<BoardInfo>(`/api/ui/games/${encodeURIComponent(name)}/board`),
  writeBoard: (name: string, board: BoardCell[][], baseHash: string, force = false) =>
    request<BoardInfo>(`/api/ui/games/${encodeURIComponent(name)}/board`, {
      method: 'PUT',
      body: JSON.stringify({ board, baseHash, force }),
    }),
  previewBoard: (name: string, board: BoardCell[][]) =>
    request<{ text: string }>(`/api/ui/games/${encodeURIComponent(name)}/board/preview`, {
      method: 'POST',
      body: JSON.stringify({ board }),
    }),
  gameChecks: (name: string) =>
    request<GameCheckReport>(
      `/api/ui/games/${encodeURIComponent(name)}/checks`,
    ),
  gameTiers: (name: string) =>
    request<TiersReport>(`/api/ui/games/${encodeURIComponent(name)}/tiers`),
  customTiers: (
    name: string,
    body: { mode: string; steps: number[]; durations?: number[] },
  ) =>
    request<{ report: ModeTiers; spec: TierSpec }>(
      `/api/ui/games/${encodeURIComponent(name)}/tiers`,
      { method: 'POST', body: JSON.stringify(body) },
    ),
  clientReport: (name: string) =>
    request<{ game: string; markdown: string }>(
      `/api/ui/games/${encodeURIComponent(name)}/report`,
    ),
  history: (name: string) =>
    request<HistoryReport>(`/api/ui/games/${encodeURIComponent(name)}/history`),
  scanStatus: (name: string) =>
    request<{ game: string; job: ScanJob | null; reports: ScanReport[] }>(
      `/api/ui/games/${encodeURIComponent(name)}/scan`,
    ),
  startScan: (
    name: string,
    body: { mode?: string; params?: ScanParams; force?: boolean } = {},
  ) =>
    request<{ ok: boolean; alreadyRunning?: boolean }>(
      `/api/ui/games/${encodeURIComponent(name)}/scan`,
      { method: 'POST', body: JSON.stringify(body) },
    ),
  preflight: (name: string) =>
    request<PreflightReport>(
      `/api/ui/games/${encodeURIComponent(name)}/preflight`,
    ),
  reviewIds: (name: string) =>
    request<{ picks: unknown[]; text: string }>(
      `/api/ui/games/${encodeURIComponent(name)}/review-ids`,
    ),
  startImport: (name: string) =>
    request<{ ok: boolean }>(
      `/api/ui/games/${encodeURIComponent(name)}/import`,
      { method: 'POST' },
    ),
  importStatus: (name: string) =>
    request<{ imported: boolean; job: ImportJob | null }>(
      `/api/ui/games/${encodeURIComponent(name)}/import`,
    ),
  prepareSession: (body: {
    sessionId: string;
    game: string;
    balance?: number;
    currency?: string;
    social?: boolean;
    language?: string;
  }) =>
    request<{ ok: boolean }>('/api/ui/sessions/prepare', {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  setForce: (body: {
    game: string;
    mode?: string;
    simId?: number;
    sessionId?: string;
  }) =>
    request<{ ok: boolean }>('/api/ui/force', {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  getPool: (game: string) =>
    request<{ modes: PoolOptions[]; active: PoolSummary[] }>(
      `/api/ui/pool?game=${encodeURIComponent(game)}`,
    ),
  setPool: (body: { game: string; enabled: boolean; mode: string; tier?: number | null }) =>
    request<{ ok: boolean; active: PoolSummary | null }>('/api/ui/pool', {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  setBalance: (body: { sessionId: string; game: string; balance: number }) =>
    request<{ ok: boolean; balance: number }>('/api/ui/sessions/balance', {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  getFaults: (game: string) =>
    request<{ game: string; faults: Faults }>(
      `/api/ui/faults?game=${encodeURIComponent(game)}`,
    ),
  setFaults: (game: string, faults: Faults) =>
    request<{ ok: boolean; faults: Faults }>('/api/ui/faults', {
      method: 'POST',
      body: JSON.stringify({ game, ...faults }),
    }),
  resetSessions: (game?: string) =>
    request<{ ok: boolean }>('/api/ui/sessions/reset', {
      method: 'POST',
      body: JSON.stringify({ game }),
    }),
  tlsStatus: () => request<{ trusted: boolean }>('/api/ui/tls/status'),
  tlsInstall: () =>
    request<{ ok: boolean; trusted: boolean }>('/api/ui/tls/install', {
      method: 'POST',
    }),
  pickFolder: () =>
    request<FolderPick>('/api/ui/pick-folder', { method: 'POST' }),
  resolvePath: (path: string) =>
    request<{ games: FoundGame[] }>('/api/ui/resolve', {
      method: 'POST',
      body: JSON.stringify({ path }),
    }),
};

/** Bonus-only draw: the RGS picks among the mode's bonus books, LUT-weighted. */
export interface PoolSummary {
  mode: string;
  /** Scatters of the trigger; null = any bonus. */
  tier: number | null;
  books: number;
  /** How rare the pool is in real play: one round in `oneIn`. */
  oneIn: number;
}

export interface PoolOptions {
  mode: string;
  any: PoolSummary;
  tiers: PoolSummary[];
}

export interface FoundGame {
  path: string;
  name: string;
}

export interface FolderPick {
  cancelled?: boolean;
  error?: string;
  picked?: string;
  games?: FoundGame[];
}

export interface Bookmark {
  id: number;
  game: string;
  mode: string;
  simId: number;
  description: string;
  payoutX100: number;
}

export interface SpinEvent {
  type: 'spin';
  game: string;
  sessionID: string;
  mode: string;
  simId: number;
  payoutX100: number;
  amount: number;
  forced: boolean;
  /** Drawn by the bonus-only restriction. */
  pooled?: boolean;
}

export const bookmarksApi = {
  list: (game?: string) =>
    request<Bookmark[]>(
      `/api/ui/bookmarks${game ? `?game=${encodeURIComponent(game)}` : ''}`,
    ),
  add: (body: {
    game: string;
    mode: string;
    simId: number;
    description?: string;
    payoutX100?: number;
  }) =>
    request<Bookmark>('/api/ui/bookmarks', {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  remove: (id: number) =>
    request<{ ok: boolean }>(`/api/ui/bookmarks/${id}`, { method: 'DELETE' }),
};

export interface ImportJob {
  state: 'idle' | 'running' | 'done' | 'failed';
  currentMode: string | null;
  modesDone: number;
  modesTotal: number;
  bytesDone: number;
  bytesTotal: number;
  booksDone: number;
  error?: string;
}

/** SlotLab's emulated RGS port (HTTPS). */
export const RGS_PORT = 3601;

export function rgsUrlFor(game: string): string {
  return `localhost:${RGS_PORT}/api/rgs/${encodeURIComponent(game)}`;
}

export interface Feasibility {
  currentRtp: number;
  currentVol: number;
  minRtp: number;
  maxRtp: number;
  minVol: number;
  maxVol: number;
}

export interface OptimizeResult {
  ok: boolean;
  message: string;
  rtpHit: boolean;
  volHit: boolean;
  achievedRtp: number;
  achievedVol: number;
  achievedStd: number;
  mostLikelyOneIn: number;
  feasibility: Feasibility;
}

export interface OptimizePreview {
  result: OptimizeResult;
  before: ModeStats;
  after: ModeStats;
}

export interface OptimizeBody {
  targetRtp: number;
  targetVol?: number | null;
  capMostLikely: boolean;
  targetZeroRate?: number | null;
  maxWinOneIn?: number | null;
  targetTail10k?: number | null;
}

export const optimizeApi = {
  feasibility: (game: string, mode: string) =>
    request<{ feasibility: Feasibility; cost: number }>(
      `/api/ui/games/${encodeURIComponent(game)}/optimize/${encodeURIComponent(mode)}`,
    ),
  preview: (game: string, mode: string, body: OptimizeBody) =>
    request<OptimizePreview>(
      `/api/ui/games/${encodeURIComponent(game)}/optimize/${encodeURIComponent(mode)}`,
      { method: 'POST', body: JSON.stringify(body) },
    ),
  apply: (game: string, mode: string, body: OptimizeBody) =>
    request<{ ok?: boolean; error?: string; achievedRtp: number; achievedVol: number; backup: string }>(
      `/api/ui/games/${encodeURIComponent(game)}/optimize/${encodeURIComponent(mode)}/apply`,
      { method: 'POST', body: JSON.stringify(body) },
    ),
};

// ---- Challenge calculator ----

export interface ChallengeTargetInfo {
  target: number;
  probability: number;
  oneIn: number;
  expectedRounds: number;
  expectedTurnover: number;
  hunterGrindCost: number;
}

export interface ChallengeSuggested {
  target: number;
  probability: number;
  oneIn: number;
  /** award ÷ expected turnover (0.004 = +0.4 % RTP). */
  boost: number;
}

export interface ChallengeAwardInfo {
  award: number;
  breakEvenTurnover: number;
  breakEvenRounds: number;
  suggested: ChallengeSuggested | null;
}

export type ChallengeVerdict =
  | 'dead'
  | 'thin'
  | 'sweet'
  | 'generous'
  | 'giveaway';

export interface ChallengeReport {
  mode: string;
  cost: number;
  rtp: number;
  houseEdge: number;
  minBet: number;
  maxMult: number;
  maxMultOneIn: number;
  targets: ChallengeTargetInfo[];
  awards: ChallengeAwardInfo[];
  /** boostMatrix[targetIdx][awardIdx] = award ÷ expected turnover. */
  boostMatrix: number[][];
  verdictMatrix: ChallengeVerdict[][];
}

export const challengeApi = {
  analyze: (
    game: string,
    body: {
      mode?: string;
      minBet?: number;
      awards?: number[];
      targets?: number[];
    },
  ) =>
    request<ChallengeReport>(
      `/api/ui/games/${encodeURIComponent(game)}/challenge`,
      { method: 'POST', body: JSON.stringify(body) },
    ),
};

/**
 * A game's full playable URL: the frontend needs `rgs_url` +
 * `sessionID` to reach the RGS, otherwise its fetch fails
 * (ERR_NAME_NOT_RESOLVED). `base` = servedUrl or Vite dev URL.
 */
export function playUrlFor(
  game: string,
  base: string,
  opts: {
    sessionId?: string;
    currency?: string;
    lang?: string;
    device?: string;
    social?: boolean;
  } = {},
): string {
  const b = base.replace(/\/$/, '');
  const params = new URLSearchParams({
    sessionID: opts.sessionId ?? `slotlab-${game}-${Date.now()}`,
    rgs_url: rgsUrlFor(game),
    lang: opts.lang ?? 'en',
    currency: opts.currency ?? 'USD',
    device: opts.device ?? 'desktop',
    social: String(opts.social ?? false),
    demo: 'true',
  });
  return `${b}/?${params.toString()}`;
}

// ---- Number formatting: the only formatters the UI should use ----
//
// Conventions: RTP / rates → fmtPct (2 decimals everywhere: "96.50 %"),
// multipliers → fmtX with thousands separators ("50,000x"), sizes →
// fmtBytes ("1.4 MB"), counts → fmtInt ("350,000").

/** Fraction (0.965) → "96.50 %". */
export function fmtPct(v: number, digits = 2): string {
  if (!isFinite(v)) return '—';
  return (v * 100).toFixed(digits) + ' %';
}

/** Multiplier → "50,000x" / "1.25x". */
export function fmtX(v: number, digits = 2): string {
  if (!isFinite(v)) return '—';
  return (
    v.toLocaleString('en-US', { minimumFractionDigits: digits, maximumFractionDigits: digits }) +
    'x'
  );
}

/** Integer count → "350,000". */
export function fmtInt(v: number): string {
  if (!isFinite(v)) return '—';
  return Math.round(v).toLocaleString('en-US');
}

/** Bytes → "820 KB" / "13.0 MB" (decimal units, like Finder and the Stake dashboard). */
export function fmtBytes(bytes: number): string {
  if (!isFinite(bytes)) return '—';
  if (bytes < 1000) return `${Math.round(bytes)} B`;
  if (bytes < 1_000_000) return `${Math.round(bytes / 1000)} KB`;
  if (bytes < 1_000_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  return `${(bytes / 1_000_000_000).toFixed(2)} GB`;
}

export function fmtOneIn(v: number): string {
  if (!isFinite(v)) return 'never';
  return '1 / ' + Math.round(v).toLocaleString('en-US');
}
