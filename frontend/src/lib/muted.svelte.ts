/**
 * The muted findings of the selected game, shared by every page that shows
 * findings (Analyze, Preflight, Crowd sim). The list lives on the server
 * (`/api/ui/games/{name}/muted`): the Mac and a remote UI read the same one.
 */
import { api, type MuteEntry } from './api';

class Mutes {
  game = $state<string | null>(null);
  list = $state<MuteEntry[]>([]);
  error = $state<string | null>(null);
  busy = $state(false);
  #seq = 0;

  /** Reads the game's list (again: another browser may have changed it). */
  async load(game: string) {
    const token = ++this.#seq;
    if (game !== this.game) {
      this.game = game;
      this.list = [];
    }
    try {
      const r = await api.muted(game);
      if (token === this.#seq) {
        this.list = r.muted.map((m) => ({ ...m, mode: m.mode ?? null }));
        this.error = null;
      }
    } catch (e) {
      if (token === this.#seq) this.error = e instanceof Error ? e.message : String(e);
    }
  }

  async #apply(call: () => Promise<{ muted: MuteEntry[] }>) {
    const token = ++this.#seq;
    this.busy = true;
    try {
      const r = await call();
      if (token === this.#seq) {
        this.list = r.muted.map((m) => ({ ...m, mode: m.mode ?? null }));
        this.error = null;
      }
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    } finally {
      this.busy = false;
    }
  }

  /** `mode: null` = every mode of the game. */
  mute(rule: string, mode: string | null) {
    const game = this.game;
    if (game) return this.#apply(() => api.mute(game, rule, mode));
  }

  unmute(rule: string, mode: string | null) {
    const game = this.game;
    if (game) return this.#apply(() => api.unmute(game, rule, mode));
  }
}

export const mutes = new Mutes();
