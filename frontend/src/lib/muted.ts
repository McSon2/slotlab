/**
 * Muted findings — the rules a studio chose to stop seeing for one game.
 *
 * A mute is `{ rule, mode }` (`slotlab-app/src/muted.rs`, kept on the server):
 * `rule` is the stable key of the finding — `Issue.rule` of `findings.ts`
 * (`plaque-loss`, `symbols-fs`, `hit-low`…), `preflight:<id>` for the
 * pre-upload checklist, `sim:<key>` for the crowd simulation — never its
 * numbers, so a mute survives a rebuild of the math. `mode: null` mutes the
 * rule in every mode of the game (and the whole-game findings).
 *
 * A blocking finding is never muted, whatever the list says: the button is
 * absent and the server refuses it. Pure functions only (bundled by the CLI).
 */

import type { MuteEntry } from './api';
import type { Issue } from './findings';

/** `findings.ts` keys that are always a Stake rule (mirrors `UNMUTABLE_*` in `muted.rs`). */
export const UNMUTABLE_PREFIXES = ['check:', 'publish:', 'star:'];
export const UNMUTABLE_RULES = ['gap', 'most-likely', 'ladder-freq', 'ladder-mean', 'ante-tier'];

/** Whether the « Mute » button may be offered for this finding. */
export function canMute(rule: string, blocking: boolean): boolean {
  if (blocking || !rule) return false;
  return !UNMUTABLE_PREFIXES.some((p) => rule.startsWith(p)) && !UNMUTABLE_RULES.includes(rule);
}

/** The mute covering `rule` in `mode` (`null` = a whole-game finding: only an every-mode mute covers it). */
export function muteOf(mutes: MuteEntry[], rule: string, mode: string | null): MuteEntry | undefined {
  return (
    (mode !== null ? mutes.find((m) => m.rule === rule && m.mode === mode) : undefined) ??
    mutes.find((m) => m.rule === rule && m.mode === null)
  );
}

export interface Split<T> {
  shown: T[];
  muted: T[];
}

/** Findings of one mode (or of the whole game) apart from the muted ones. */
export function splitBy<T>(items: T[], mutes: MuteEntry[], key: (t: T) => { rule: string; mode: string | null; blocking: boolean }): Split<T> {
  const out: Split<T> = { shown: [], muted: [] };
  for (const it of items) {
    const k = key(it);
    (canMute(k.rule, k.blocking) && muteOf(mutes, k.rule, k.mode) ? out.muted : out.shown).push(it);
  }
  return out;
}

/**
 * `findings.ts` issues apart from the muted ones. An issue merged over several
 * modes (`mode` + `also`) muted in some of them only is cut in two: the modes
 * still shown, the modes muted.
 */
export function splitIssues(issues: Issue[], mutes: MuteEntry[]): Split<Issue> {
  const out: Split<Issue> = { shown: [], muted: [] };
  const withModes = (it: Issue, modes: string[]): Issue => ({ ...it, mode: modes[0], also: modes.length > 1 ? modes.slice(1) : undefined });
  for (const it of issues) {
    if (!mutes.length || !canMute(it.rule, it.severity === 'blocking')) {
      out.shown.push(it);
      continue;
    }
    if (it.mode === null) {
      (muteOf(mutes, it.rule, null) ? out.muted : out.shown).push(it);
      continue;
    }
    const modes = [it.mode, ...(it.also ?? [])];
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

/** One line of the « muted » section: a mute and what it hides right now. */
export interface MutedGroup {
  mute: MuteEntry;
  /** Titles of the findings it hides today (empty: the rule no longer fires). */
  titles: string[];
}

/** The mutes of a page, each with the findings it hides there; `all` keeps mutes that hide nothing today. */
export function mutedGroups<T>(
  mutes: MuteEntry[],
  muted: T[],
  key: (t: T) => { rule: string; modes: (string | null)[]; title: string },
  all = true,
  isOurs: (m: MuteEntry) => boolean = () => true,
): MutedGroup[] {
  const groups: MutedGroup[] = [];
  for (const m of mutes) {
    if (!isOurs(m)) continue;
    const titles = muted
      .map(key)
      .filter((k) => k.rule === m.rule && k.modes.some((md) => muteOf([m], m.rule, md)))
      .map((k) => k.title);
    if (titles.length || all) groups.push({ mute: m, titles: [...new Set(titles)] });
  }
  return groups;
}

/** Crowd-sim readings of one cohort (`mode`: its single mode, `null` for a mixed session); context lines are never muted. */
export function splitSim<T extends { key: string; severity: string }>(lines: T[], mutes: MuteEntry[], mode: string | null): Split<T> {
  return splitBy(lines, mutes, (l) => ({ rule: l.key, mode, blocking: l.severity === 'lived' }));
}

/** Which page a mute belongs to, by its key's prefix. */
export const isPreflightKey = (rule: string) => rule.startsWith('preflight:');
export const isSimKey = (rule: string) => rule.startsWith('sim:');
export const isAnalyzeKey = (rule: string) => !isPreflightKey(rule) && !isSimKey(rule);
