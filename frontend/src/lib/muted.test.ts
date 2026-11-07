// Run by `pnpm test` (vite.test.config.ts → node --test).
// @ts-nocheck — node:test types are not in this project's tsconfig.
import test from 'node:test';
import assert from 'node:assert/strict';
import { canMute, muteOf, splitBy, splitIssues, splitSim, mutedGroups } from './muted';
import type { Issue } from './findings';

const issue = (rule: string, mode: string | null, severity: Issue['severity'] = 'serious', also?: string[]): Issue => ({
  rule, severity, category: 'Celebrations', mode, also, title: `${rule} ${mode}`, big: '', ref: '', excess: 2, text: '', why: '',
});

test('blocking findings and Stake rules are never mutable', () => {
  assert.equal(canMute('plaque-loss', false), true);
  assert.equal(canMute('plaque-loss', true), false);
  for (const r of ['check:rtp', 'publish:x', 'star:CVaR', 'gap', 'most-likely', 'ladder-freq', 'ladder-mean', 'ante-tier']) assert.equal(canMute(r, false), false, r);
});

test('a mode mute covers that mode only; an every-mode mute covers all and the whole game', () => {
  const m = [{ rule: 'hit-low', mode: 'MAXWIN' }, { rule: 'symbols-fs', mode: null }];
  assert.ok(muteOf(m, 'hit-low', 'MAXWIN'));
  assert.equal(muteOf(m, 'hit-low', 'BASE'), undefined);
  assert.equal(muteOf(m, 'hit-low', null), undefined);
  assert.ok(muteOf(m, 'symbols-fs', 'BONUS'));
  assert.ok(muteOf(m, 'symbols-fs', null));
});

test('splitIssues hides muted issues, keeps blocking ones, cuts merged issues', () => {
  const issues = [
    issue('plaque-loss', 'MYSTERY'),
    issue('plaque-loss', 'HIDDEN'),
    issue('symbols-fs', 'MYSTERY', 'serious', ['HIDDEN', 'SUPERBONUS']),
    issue('gap', 'BASE', 'blocking'),
    issue('hit-low', 'MAXWIN', 'minor'),
  ];
  const mutes = [
    { rule: 'plaque-loss', mode: 'MYSTERY' },
    { rule: 'symbols-fs', mode: 'HIDDEN' },
    { rule: 'gap', mode: null },
  ];
  const s = splitIssues(issues, mutes);
  assert.deepEqual(s.shown.map((i) => `${i.rule}@${[i.mode, ...(i.also ?? [])].join('+')}`), ['plaque-loss@HIDDEN', 'symbols-fs@MYSTERY+SUPERBONUS', 'gap@BASE', 'hit-low@MAXWIN']);
  assert.deepEqual(s.muted.map((i) => `${i.rule}@${[i.mode, ...(i.also ?? [])].join('+')}`), ['plaque-loss@MYSTERY', 'symbols-fs@HIDDEN']);
  assert.equal(splitIssues(issues, [{ rule: 'symbols-fs', mode: null }]).muted.length, 1);
  assert.equal(splitIssues(issues, []).shown.length, issues.length);
});

test('preflight / crowd sim readings', () => {
  const pf = [{ id: 'unused_assets', fail: false }, { id: 'lut_book_match', fail: true }];
  const mutes = [{ rule: 'preflight:unused_assets', mode: null }, { rule: 'preflight:lut_book_match', mode: null }, { rule: 'sim:dry-spell', mode: 'BASE' }];
  const s = splitBy(pf, mutes, (f) => ({ rule: `preflight:${f.id}`, mode: null, blocking: f.fail }));
  assert.deepEqual(s.shown.map((f) => f.id), ['lut_book_match']);
  const lines = [{ key: 'sim:dry-spell', severity: 'minor' }, { key: 'sim:best-win', severity: 'lived' }];
  assert.equal(splitSim(lines, mutes, 'BASE').muted.length, 1);
  assert.equal(splitSim(lines, mutes, 'BONUS').muted.length, 0);
  assert.equal(splitSim(lines, mutes, null).muted.length, 0);
});

test('mutedGroups lists each mute with what it hides, stale ones included on demand', () => {
  const mutes = [{ rule: 'plaque-loss', mode: 'MYSTERY' }, { rule: 'steps', mode: null }];
  const muted = [issue('plaque-loss', 'MYSTERY')];
  const key = (i: Issue) => ({ rule: i.rule, modes: [i.mode], title: i.title });
  assert.deepEqual(mutedGroups(mutes, muted, key).map((g) => g.titles.length), [1, 0]);
  assert.equal(mutedGroups(mutes, muted, key, false).length, 1);
});
