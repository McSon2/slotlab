#!/usr/bin/env node
/**
 * SlotLab as MCP tools for Claude Code — stdio JSON-RPC, no dependency.
 *
 * Register once (user scope, every project):
 *   claude mcp add --scope user slotlab -- node <this file>
 *
 * Every tool reads the local SlotLab (http://localhost:3600, or $SLOTLAB_URL)
 * through `slotlab-report.mjs`, the Analyze findings bundled for Node: what
 * the agent reads is exactly what the Worth a look cards say.
 */
import { createInterface } from 'node:readline';
import * as R from './slotlab-report.mjs';

const SERVER = { name: 'slotlab', version: '1.0.0' };

const str = (description, extra = {}) => ({ type: 'string', description, ...extra });
const num = (description) => ({ type: 'number', description });
const bool = (description) => ({ type: 'boolean', description });

const TOOLS = [
  {
    name: 'slotlab_games',
    description: 'The games registered in SlotLab: name, modes with their cost, whether the books are imported, publish folder.',
    inputSchema: { type: 'object', properties: {} },
  },
  {
    name: 'slotlab_findings',
    description:
      'Worth a look — everything SlotLab finds wrong with a game\'s math (blocking / serious / minor, context on demand), with the key numbers of every mode and the market deviations. Read it after every run.py prod / opt and iterate until no blocking, serious or minor finding remains (except a decision the client took knowingly). Waits for the books import when the publish just changed.',
    inputSchema: {
      type: 'object',
      properties: {
        game: str('Game name as registered in SlotLab (slotlab_games).'),
        mode: str('One mode only (optional).'),
        context: bool('Also list the context lines (worth knowing, not faults). Default false.'),
        compliance: str('"strict": the 3★ grid is a rule; "lenient": Stake agreed this studio may ship past it (Degen Lab); "auto" (default): lenient for a Degen Lab folder.', { enum: ['auto', 'strict', 'lenient'] }),
        wait: bool('Wait until the books are imported and the files on disk are the ones read (default true).'),
      },
      required: ['game'],
    },
  },
  {
    name: 'slotlab_mode',
    description: 'One mode in detail: RTP, hit rate, volatility, max win odds, 100-round session, RTP split, bonus tiers (frequency, min, median, average, P90, RTP share, books), plain-spin features, and every market figure against the peers (p25 · median · p75, position).',
    inputSchema: { type: 'object', properties: { game: str('Game name.'), mode: str('Mode name.') }, required: ['game', 'mode'] },
  },
  {
    name: 'slotlab_population',
    description: 'A population of a mode — all / nobonus / bonus / tier:N / crit:<family> — with percentiles (and a replayable book id at each), shares under thresholds, bands with books and effective books, retriggers, free spins.',
    inputSchema: { type: 'object', properties: { game: str('Game name.'), mode: str('Mode name.'), key: str('Population key: all, nobonus, bonus, tier:3, tier:4, crit:multi_spin…') }, required: ['game', 'mode', 'key'] },
  },
  {
    name: 'slotlab_market',
    description: 'Every figure of every mode against the modes of the same kind the market plays (daily 30-day top): hit rate, win ladder, max win odds, volatility, 100-round session, value in the top 1 %… with p25 · median · p75 and the mode\'s position. Aim: inside the middle half; max win on the rare side.',
    inputSchema: { type: 'object', properties: { game: str('Game name.'), mode: str('One mode only (optional).') }, required: ['game'] },
  },
  {
    name: 'slotlab_reach',
    description: 'Share of sessions of N spins that never pay X× or more (exact). Studio reference: 6 000 spins without 500× ≈ 20–30 % on a base game.',
    inputSchema: { type: 'object', properties: { game: str('Game name.'), mode: str('Mode name.'), spins: num('Spins in the session (e.g. 6000).'), x: num('Win threshold in multiples of the base bet (e.g. 500).') }, required: ['game', 'mode', 'spins', 'x'] },
  },
  {
    name: 'slotlab_version',
    description: 'Which build of the math SlotLab shows: when the files were read, whether they changed on disk since, whether the books are imported. SlotLab reloads a changed publish folder by itself within ~20 s.',
    inputSchema: { type: 'object', properties: { game: str('Game name.') }, required: ['game'] },
  },
  {
    name: 'slotlab_reload',
    description: 'Reads the publish files of a game again now (after a run.py prod / opt) and re-imports the books if they changed; returns once the import is done.',
    inputSchema: { type: 'object', properties: { game: str('Game name.') }, required: ['game'] },
  },
];

async function call(name, a = {}) {
  const base = process.env.SLOTLAB_URL ?? R.DEFAULT_BASE;
  switch (name) {
    case 'slotlab_games': {
      const g = await R.games(base);
      return g.map((x) => `${x.name} — ${x.loaded ? 'loaded' : 'NOT LOADED'}, ${x.imported ? 'books imported' : 'books NOT imported'} — ${x.modes.map((m) => `${m.name} ${m.cost}x`).join(', ')} — ${x.publishDir}`).join('\n');
    }
    case 'slotlab_findings': {
      if (a.wait !== false) await R.waitImported(a.game, base);
      const d = await R.fetchGame(a.game, base);
      const r = R.buildReport(d, R.isLenient(d, a.compliance === 'strict' ? false : a.compliance === 'lenient' ? true : 'auto'));
      return R.toMarkdown(r, { mode: a.mode, context: !!a.context, market: true });
    }
    case 'slotlab_mode': {
      const d = await R.fetchGame(a.game, base);
      const r = R.buildReport(d, R.isLenient(d));
      if (!r.modes.some((m) => m.mode === a.mode)) throw new Error(`unknown mode ${a.mode}; modes: ${r.modes.map((m) => m.mode).join(', ')}`);
      return R.toMarkdown(r, { mode: a.mode, context: true, market: true });
    }
    case 'slotlab_population': {
      const d = await R.fetchGame(a.game, base);
      const mp = d.pops.find((m) => m.mode === a.mode);
      if (!mp) throw new Error(`unknown mode ${a.mode}`);
      const p = mp.populations.find((x) => x.key === a.key);
      if (!p) throw new Error(`no population ${a.key} in ${a.mode}; keys: ${mp.populations.map((x) => x.key).join(', ')}`);
      return R.populationMarkdown(mp, p);
    }
    case 'slotlab_market': {
      const d = await R.fetchGame(a.game, base);
      return R.marketMarkdown(R.buildReport(d, R.isLenient(d)), a.mode);
    }
    case 'slotlab_reach': {
      const s = await R.reach(a.game, a.mode, Math.round(a.spins), a.x, base);
      return `${(s.never * 100).toFixed(1)} % of ${a.mode} sessions of ${s.spins} spins never pay ${s.x}x or more (a spin pays it ${s.oneIn ? `1 in ${Math.round(s.oneIn).toLocaleString('en-US')}` : 'never'}; ${s.expected.toFixed(2)} per session on average).`;
    }
    case 'slotlab_version': {
      const v = await R.version(a.game, base);
      return JSON.stringify(v, null, 1);
    }
    case 'slotlab_reload': {
      await R.reload(a.game, base);
      const v = await R.waitImported(a.game, base);
      return `Reloaded and imported: ${JSON.stringify(v)}`;
    }
    default:
      throw new Error(`unknown tool ${name}`);
  }
}

const out = (msg) => process.stdout.write(JSON.stringify(msg) + '\n');
const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on('line', async (line) => {
  if (!line.trim()) return;
  let req;
  try {
    req = JSON.parse(line);
  } catch {
    return;
  }
  const { id, method, params } = req;
  if (id === undefined) return; // a notification
  try {
    let result;
    switch (method) {
      case 'initialize':
        result = { protocolVersion: params?.protocolVersion ?? '2024-11-05', capabilities: { tools: {} }, serverInfo: SERVER };
        break;
      case 'ping':
        result = {};
        break;
      case 'tools/list':
        result = { tools: TOOLS };
        break;
      case 'tools/call': {
        try {
          const text = await call(params?.name, params?.arguments ?? {});
          result = { content: [{ type: 'text', text }] };
        } catch (e) {
          result = { content: [{ type: 'text', text: `SlotLab error: ${e?.message ?? e}. Is SlotLab running (${process.env.SLOTLAB_URL ?? R.DEFAULT_BASE})?` }], isError: true };
        }
        break;
      }
      default:
        out({ jsonrpc: '2.0', id, error: { code: -32601, message: `method not found: ${method}` } });
        return;
    }
    out({ jsonrpc: '2.0', id, result });
  } catch (e) {
    out({ jsonrpc: '2.0', id, error: { code: -32000, message: String(e?.message ?? e) } });
  }
});
