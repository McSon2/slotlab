import type { GameSummary } from './api';
import { api } from './api';
import { storageGet, storageRemove, storageSet } from './storage';

export type Tab =
  | 'projects'
  | 'drive'
  | 'games'
  | 'play'
  | 'board'
  | 'analyze'
  | 'balance'
  | 'challenge'
  | 'crowdsim'
  | 'scan'
  | 'preflight'
  | 'assets'
  | 'review'
  | 'publish';

/** Publish page: temporary Cloudflare link or the permanent homelab. */
export type PublishView = 'link' | 'homelab';

const TABS: Tab[] = [
  'projects',
  'drive',
  'games',
  'play',
  'board',
  'analyze',
  'balance',
  'challenge',
  'crowdsim',
  'scan',
  'preflight',
  'assets',
  'review',
  'publish',
];

const SELECTED_GAME_KEY = 'slotlab.selectedGame';

/** `#/analyze`, `#/publish/homelab` (old `#/share`, `#/deploy` still work). */
function parseHash(hash: string): { tab: Tab; publishView: PublishView } | null {
  const [head, sub] = hash.replace(/^#\/?/, '').split('/');
  if (head === 'share') return { tab: 'publish', publishView: 'link' };
  if (head === 'deploy') return { tab: 'publish', publishView: 'homelab' };
  if (!TABS.includes(head as Tab)) return null;
  return { tab: head as Tab, publishView: sub === 'homelab' ? 'homelab' : 'link' };
}

const initial = parseHash(typeof location !== 'undefined' ? location.hash : '');

export const app = $state({
  tab: (initial?.tab ?? 'play') as Tab,
  publishView: (initial?.publishView ?? 'link') as PublishView,
  /** True when the tab came from the URL (a reload keeps it). */
  tabFromUrl: initial != null,
  selectedGame: storageGet(SELECTED_GAME_KEY),
  games: [] as GameSummary[],
  loading: false,
  error: null as string | null,
});

$effect.root(() => {
  $effect(() => {
    if (app.selectedGame) {
      storageSet(SELECTED_GAME_KEY, app.selectedGame);
    } else {
      storageRemove(SELECTED_GAME_KEY);
    }
  });

  // Tab ⇄ URL hash: a reload lands on the same page, Back/Forward work.
  $effect(() => {
    const want = app.tab === 'publish' ? `#/publish/${app.publishView}` : `#/${app.tab}`;
    if (location.hash === want) return;
    if (location.hash === '') history.replaceState(null, '', want);
    else history.pushState(null, '', want);
  });
});

if (typeof window !== 'undefined') {
  window.addEventListener('popstate', () => {
    const r = parseHash(location.hash);
    if (!r) return;
    app.tab = r.tab;
    app.publishView = r.publishView;
  });
}

export function goTo(tab: Tab, publishView?: PublishView): void {
  app.tab = tab;
  if (publishView) app.publishView = publishView;
}

export async function refreshGames(): Promise<void> {
  app.loading = true;
  app.error = null;
  try {
    app.games = await api.listGames();
    if (app.selectedGame && !app.games.some((g) => g.name === app.selectedGame)) {
      app.selectedGame = null;
    }
    if (!app.selectedGame && app.games.length > 0) {
      app.selectedGame = app.games[0].name;
    }
  } catch (e) {
    app.error = errorText(e);
  } finally {
    app.loading = false;
  }
}

export function selectedGame(): GameSummary | undefined {
  return app.games.find((g) => g.name === app.selectedGame);
}

// ---- Notices: the one place async outcomes are reported ----
//
// Rendered by App.svelte in a live region (errors: role=alert, the rest:
// role=status), so a click that fails never fails silently.

export type NoticeKind = 'ok' | 'info' | 'error';

export interface Notice {
  id: number;
  kind: NoticeKind;
  text: string;
}

export const notices = $state<Notice[]>([]);

let noticeSeq = 0;

export function dismiss(id: number): void {
  const i = notices.findIndex((n) => n.id === id);
  if (i >= 0) notices.splice(i, 1);
}

/** Shows a transient notice (errors stay longer). Returns its id. */
export function notify(text: string, kind: NoticeKind = 'ok', ms?: number): number {
  const id = ++noticeSeq;
  notices.push({ id, kind, text });
  if (notices.length > 4) notices.splice(0, notices.length - 4);
  const ttl = ms ?? (kind === 'error' ? 9000 : 3500);
  if (ttl > 0) setTimeout(() => dismiss(id), ttl);
  return id;
}

export function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/**
 * Runs an async action and reports its outcome: `ok` shown on success,
 * the error (prefixed by `error` when given) on failure. Never throws —
 * returns undefined when the action failed.
 */
export async function run<T>(
  fn: () => Promise<T>,
  opts: { ok?: string; error?: string } = {},
): Promise<T | undefined> {
  try {
    const r = await fn();
    if (opts.ok) notify(opts.ok, 'ok');
    return r;
  } catch (e) {
    notify(opts.error ? `${opts.error}: ${errorText(e)}` : errorText(e), 'error');
    return undefined;
  }
}
