// Adding a game to SlotLab — shared by the Games page and the onboarding
// dialog: browse for a folder (or paste a path), resolve it to every
// `publish_files` inside, then add the single fresh game right away or offer
// the list when there are several.
//
// Every outcome lands in `error` / `info` (rendered inline by the caller);
// `toast: true` additionally reports successes as notices.

import { api, type FoundGame } from './api';
import { app, errorText, notify, refreshGames } from './stores.svelte';

/** game_id of `<repo>/math/games/<id>/library/publish_files`, or ''. */
export function nameFromPublishDir(path: string): string {
  const m = path.trim().match(/\/math\/games\/([^/]+)\/library\/publish_files\/?$/);
  return m?.[1] ?? '';
}

/** Games not registered yet (by name or path). */
export function freshOf(games: FoundGame[]): FoundGame[] {
  return games.filter(
    (g) => !app.games.some((x) => x.name === g.name || x.publishDir === g.path),
  );
}

export class GameAdder {
  /** The typed / picked path. */
  path = $state('');
  busy = $state(false);
  error = $state<string | null>(null);
  info = $state<string | null>(null);
  /** Several fresh games found in one folder → the user picks. */
  candidates = $state<FoundGame[]>([]);
  /** Names added through this adder, in order. */
  added = $state<string[]>([]);
  /** Name derived from the typed path when it is a publish_files itself. */
  derivedName = $derived(nameFromPublishDir(this.path));

  #toast: boolean;

  constructor(opts: { toast?: boolean } = {}) {
    this.#toast = opts.toast ?? false;
  }

  #reset() {
    this.error = null;
    this.info = null;
    this.candidates = [];
  }

  /** Native folder picker (served by the local backend). */
  browse = async () => {
    this.#reset();
    this.busy = true;
    let games: FoundGame[] | null = null;
    try {
      const r = await api.pickFolder();
      if (r.cancelled) return;
      if (r.error) {
        this.error = r.error;
        return;
      }
      games = r.games ?? [];
      if (games.length === 0) {
        this.path = r.picked ?? '';
        this.error =
          'No publish_files found in this folder. Pick a game folder (or any parent folder — SlotLab searches inside).';
        return;
      }
    } catch (e) {
      this.error = `Could not open the folder picker: ${errorText(e)}`;
      return;
    } finally {
      this.busy = false;
    }
    await this.#handleFound(games);
  };

  /** The typed path: a publish_files (fast path), or any folder to search. */
  addTyped = async () => {
    const path = this.path.trim();
    if (!path || this.busy) return;
    this.#reset();
    if (this.derivedName) {
      await this.addOne(this.derivedName, path);
      return;
    }
    this.busy = true;
    let games: FoundGame[];
    try {
      games = (await api.resolvePath(path)).games;
    } catch (e) {
      this.error = `Could not read this path: ${errorText(e)}`;
      return;
    } finally {
      this.busy = false;
    }
    if (games.length === 0) {
      this.error =
        'No publish_files found at this path. Use "Browse" or paste a game folder (or any parent folder).';
      return;
    }
    await this.#handleFound(games);
  };

  /** Single fresh game → add it right away; several → offer the list. */
  async #handleFound(games: FoundGame[]) {
    const fresh = freshOf(games);
    if (fresh.length === 0) {
      this.info =
        games.length === 1
          ? 'This game is already added.'
          : `All ${games.length} games in this folder are already added.`;
      return;
    }
    if (fresh.length === 1) await this.addOne(fresh[0].name, fresh[0].path);
    else this.candidates = fresh;
  }

  addOne = async (name: string, path: string): Promise<boolean> => {
    this.busy = true;
    this.error = null;
    this.info = null;
    try {
      await api.registerGame(name, path, '');
    } catch (e) {
      this.error = `Could not add ${name}: ${errorText(e)}`;
      return false;
    } finally {
      this.busy = false;
    }
    await refreshGames();
    app.selectedGame = name;
    this.added = [...this.added, name];
    this.candidates = this.candidates.filter((c) => c.path !== path);
    this.path = '';
    if (this.#toast) notify(`${name} added — importing its books.`);
    return true;
  };

  /** Adds every candidate; the failed ones stay listed for a retry. */
  addAll = async () => {
    const list = [...this.candidates];
    if (list.length === 0) return;
    this.busy = true;
    this.error = null;
    this.info = null;
    const ok: string[] = [];
    const failed: { game: FoundGame; message: string }[] = [];
    for (const g of list) {
      try {
        await api.registerGame(g.name, g.path, '');
        ok.push(g.name);
      } catch (e) {
        failed.push({ game: g, message: errorText(e) });
      }
    }
    await refreshGames();
    this.busy = false;
    if (ok.length > 0) {
      app.selectedGame = ok[0];
      this.added = [...this.added, ...ok];
      this.path = '';
    }
    this.candidates = failed.map((f) => f.game);
    if (failed.length > 0) {
      this.error =
        `${ok.length} of ${list.length} games added — ` +
        failed.map((f) => `${f.game.name}: ${f.message}`).join(' · ');
    } else if (this.#toast) {
      notify(`${ok.length} games added — importing their books.`);
    }
  };
}
