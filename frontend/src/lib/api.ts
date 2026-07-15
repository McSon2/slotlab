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

export interface BonusCriteria extends BonusGroup {
  criteria: string;
  booksWithTrigger: number;
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
  byCriteria?: BonusCriteria[];
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
  gameChecks: (name: string) =>
    request<GameCheckReport>(
      `/api/ui/games/${encodeURIComponent(name)}/checks`,
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
  weights: number[];
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

export function fmtPct(v: number, digits = 2): string {
  return (v * 100).toFixed(digits) + ' %';
}

export function fmtX(v: number, digits = 2): string {
  return v.toFixed(digits) + 'x';
}

export function fmtOneIn(v: number): string {
  if (!isFinite(v)) return 'never';
  return '1 / ' + Math.round(v).toLocaleString('en-US');
}
