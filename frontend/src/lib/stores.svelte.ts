import type { GameSummary } from './api';
import { api } from './api';

export type Tab =
  | 'games'
  | 'play'
  | 'analyze'
  | 'balance'
  | 'crowdsim'
  | 'review'
  | 'share';

const SELECTED_GAME_KEY = 'slotlab.selectedGame';

export const app = $state({
  tab: 'play' as Tab,
  selectedGame: localStorage.getItem(SELECTED_GAME_KEY),
  games: [] as GameSummary[],
  loading: false,
  error: null as string | null,
});

$effect.root(() => {
  $effect(() => {
    if (app.selectedGame) {
      localStorage.setItem(SELECTED_GAME_KEY, app.selectedGame);
    } else {
      localStorage.removeItem(SELECTED_GAME_KEY);
    }
  });
});

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
    app.error = e instanceof Error ? e.message : String(e);
  } finally {
    app.loading = false;
  }
}

export function selectedGame(): GameSummary | undefined {
  return app.games.find((g) => g.name === app.selectedGame);
}
