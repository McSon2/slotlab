<script lang="ts">
  // Entry board editor: the board the game shows before the first spin
  // (`INITIAL_BOARD` in the frontend's constants.ts), edited by hand with the
  // game's own symbol images, checked live like the game would judge it, then
  // written back into the file.
  //
  // ⚠️ The check runs on what the board SHOWS: a paying connection on the entry
  // board is a win displayed and never paid — the review flags exactly that —
  // and a symbol on a reel no strip gives it is a board the game cannot produce.
  import { onMount, tick } from 'svelte';
  import { api, type BoardCell, type BoardInfo } from '../lib/api';
  import { app, errorText } from '../lib/stores.svelte';
  import { storageGetJson, storageRemove, storageSetJson } from '../lib/storage';
  import ConfirmButton from '../components/ConfirmButton.svelte';
  import Icon from '../components/Icon.svelte';
  import {
    SYM_ARIA,
    SYM_LABELS,
    availableModes,
    canon,
    findWins,
    makeRules,
    mismatches,
    partner,
    placementIssues,
    sameCell,
    type Issue,
    type Pos,
    type SymMode,
  } from '../lib/boardRules';

  type Tool = 'paint' | 'pick' | 'swap';
  type Props = Record<string, unknown>;

  let info = $state<BoardInfo | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let board = $state<BoardCell[][]>([]);
  /** Canonical JSON of the board as it is in the file. */
  let saved = $state('');
  let brush = $state('');
  let brushProps = $state<Record<string, Props>>({});
  let tool = $state<Tool>('paint');
  let symMode = $state<SymMode>('none');
  let hist = $state<string[]>([]);
  let fut = $state<string[]>([]);
  let hover = $state<Pos | null>(null);
  let swapA = $state<Pos | null>(null);
  let preview = $state('');
  let status = $state<{ kind: 'ok' | 'error' | 'info'; text: string } | null>(null);
  /** The board the last Apply replaced — what "Undo apply" writes back. */
  let replaced = $state<BoardCell[][] | null>(null);
  let writing = $state(false);
  let newKey = $state('');
  let newVal = $state('');
  /** The last Apply was refused: INITIAL_BOARD changed on disk since the load. */
  let staleOnDisk = $state(false);
  /** Keyboard cursor on the grid (roving tabindex). */
  let cursor = $state<Pos>([0, 1]);
  let gridEl = $state<HTMLDivElement>();

  let stroke: { before: string; changed: boolean } | null = null;
  let previewTimer: ReturnType<typeof setTimeout> | undefined;

  const draftKey = (g: string) => `slotlab.boardDraft.${g}`;
  const snap = () => $state.snapshot(board) as BoardCell[][];
  const key = (c: number, r: number) => `${c},${r}`;
  const natural = (a: string, b: string) =>
    a.localeCompare(b, undefined, { numeric: true, sensitivity: 'base' });

  $effect(() => {
    if (app.selectedGame) void load(app.selectedGame);
  });

  /** Most frequent extra fields per symbol — a W that is always `multiplier: 25` paints as one. */
  function defaultProps(i: BoardInfo): Record<string, Props> {
    const tally = new Map<string, Map<string, { props: Props; n: number }>>();
    for (const cell of [...i.consts.map((c) => c.value), ...i.board.flat()]) {
      const { name, ...rest } = cell;
      const byProps = tally.get(name) ?? new Map();
      const k = canon(rest);
      const e = byProps.get(k) ?? { props: rest, n: 0 };
      e.n++;
      byProps.set(k, e);
      tally.set(name, byProps);
    }
    const out: Record<string, Props> = {};
    for (const s of i.symbols) {
      const best = [...(tally.get(s.name)?.values() ?? [])].sort((a, b) => b.n - a.n)[0];
      out[s.name] = best ? { ...best.props } : {};
    }
    return out;
  }

  function detectSymmetry(b: BoardCell[][]): SymMode {
    return availableModes(b).find((m) => m !== 'none' && mismatches(m, b) === 0) ?? 'none';
  }

  async function load(game: string, ignoreDraft = false) {
    loading = true;
    error = null;
    status = null;
    try {
      const i = await api.gameBoard(game);
      info = i;
      saved = canon(i.board);
      let start = i.board;
      if (!ignoreDraft) {
        const d = storageGetJson<{ baseHash?: string; board?: unknown } | null>(draftKey(game), null);
        if (d && d.baseHash === i.baseHash && Array.isArray(d.board)) {
          start = d.board as BoardCell[][];
          status = { kind: 'info', text: 'Unsaved board restored — Apply writes it, Reload drops it.' };
        }
      } else {
        storageRemove(draftKey(game));
      }
      board = structuredClone(start);
      brushProps = defaultProps(i);
      const r = makeRules(i);
      brush = r.paying[0] ?? i.symbols[0]?.name ?? '';
      symMode = detectSymmetry(i.board);
      hist = [];
      fut = [];
      replaced = null;
      swapA = null;
      staleOnDisk = false;
    } catch (e) {
      error = errorText(e);
      info = null;
    } finally {
      loading = false;
    }
  }

  const rules = $derived(info ? makeRules(info) : null);
  const dirty = $derived(info ? canon(board) !== saved : false);
  const modes = $derived(availableModes(board));
  const rowsMax = $derived(Math.max(0, ...board.map((r) => r.length)));
  const uniform = $derived(board.length > 0 && board.every((r) => r.length === rowsMax));

  /** Palette: paying symbols first, then the specials, each in natural order. */
  const palette = $derived.by(() => {
    if (!info || !rules) return { paying: [] as string[], special: [] as string[] };
    const all = info.symbols.map((s) => s.name);
    return {
      paying: all.filter((n) => rules.paying.includes(n)).sort(natural),
      special: all.filter((n) => !rules.paying.includes(n)).sort(natural),
    };
  });

  const analysis = $derived.by(() => {
    const empty = {
      issues: [] as Issue[],
      bad: new Set<string>(),
      win: new Set<string>(),
      warn: new Set<string>(),
      notes: new Map<string, string[]>(),
    };
    if (!rules || board.length === 0) return empty;
    const wins = findWins(board, rules);
    const placement = placementIssues(board, rules);
    const issues: Issue[] = placement.filter((i) => i.level === 'fail');
    for (const w of wins) {
      issues.push({
        level: 'warn',
        title: `Paying connection: ${w.label}`,
        detail: `Pays ${w.pay}× before multipliers — the entry board would show a win that is never paid.`,
        cells: w.cells,
      });
    }
    if (wins.length === 0) {
      issues.push({
        level: 'pass',
        title: 'No paying connection',
        detail: `${rules.type[0].toUpperCase()}${rules.type.slice(1)} evaluation, wilds included.`,
      });
    }
    issues.push(...placement.filter((i) => i.level !== 'fail'));
    const counts = new Map<string, number>();
    board.forEach((reel) =>
      reel.slice(1, -1).forEach((c) => {
        if (rules.special.has(c.name)) counts.set(c.name, (counts.get(c.name) ?? 0) + 1);
      }),
    );
    if (counts.size) {
      issues.push({
        level: 'info',
        title: 'Specials in the frame',
        detail: [...counts].map(([n, k]) => `${k} × ${n}${rules.scatter.has(n) ? ' (scatter)' : ''}`).join(' · '),
      });
    }
    const bad = new Set<string>();
    const warn = new Set<string>();
    for (const i of placement) {
      for (const [c, r] of i.cells ?? []) (i.level === 'fail' ? bad : warn).add(key(c, r));
    }
    const win = new Set(wins.flatMap((w) => w.cells.map(([c, r]) => key(c, r))));
    // What each outlined cell is flagged for, spelled out for screen readers.
    const notes = new Map<string, string[]>();
    const note = (k: string, text: string) => notes.set(k, [...(notes.get(k) ?? []), text]);
    for (const w of wins) for (const [c, r] of w.cells) note(key(c, r), `paying connection: ${w.label}`);
    for (const i of placement) {
      for (const [c, r] of i.cells ?? []) note(key(c, r), `${i.level === 'fail' ? 'error' : 'warning'}: ${i.title}`);
    }
    return { issues, bad, win, warn, notes };
  });

  const symStatus = $derived(
    modes.filter((m) => m !== 'none').map((m) => ({ m, n: mismatches(m, board) })),
  );

  // Preview of the code Apply writes (rendered by the server, in the file's own
  // style), and the draft kept across tab switches while it differs from the file.
  $effect(() => {
    const g = info?.game;
    const hash = info?.baseHash;
    const body = canon(board);
    const isDirty = body !== saved;
    if (!g) return;
    clearTimeout(previewTimer);
    previewTimer = setTimeout(async () => {
      if (isDirty) storageSetJson(draftKey(g), { baseHash: hash, board: JSON.parse(body) });
      else storageRemove(draftKey(g));
      try {
        preview = (await api.previewBoard(g, JSON.parse(body))).text;
      } catch (e) {
        preview = `// ${errorText(e)}`;
      }
    }, 220);
  });

  // ── Editing ──────────────────────────────────────────────────────────────
  const brushCell = (name = brush): BoardCell => ({ name, ...(brushProps[name] ?? {}) });

  function setCell(c: number, r: number, v: BoardCell): boolean {
    if (!board[c]?.[r] || sameCell(board[c][r], v)) return false;
    board[c][r] = JSON.parse(JSON.stringify(v));
    return true;
  }

  function paintAt(c: number, r: number) {
    const v = brushCell();
    let changed = setCell(c, r, v);
    const p = partner(symMode, c, r, board);
    if (p && !(p[0] === c && p[1] === r)) changed = setCell(p[0], p[1], v) || changed;
    if (changed && stroke) stroke.changed = true;
  }

  function pickAt(c: number, r: number) {
    const { name, ...rest } = snap()[c][r];
    brush = name;
    brushProps[name] = rest;
    tool = 'paint';
    status = { kind: 'info', text: `Brush: ${name}${Object.keys(rest).length ? ` ${canon(rest)}` : ''}` };
  }

  function record(before: string) {
    hist = [...hist.slice(-199), before];
    fut = [];
  }

  function swapAt(c: number, r: number) {
    if (!swapA) {
      swapA = [c, r];
      return;
    }
    const [a, b] = swapA;
    swapA = null;
    if (a === c && b === r) return;
    const before = JSON.stringify(snap());
    const pairs: [Pos, Pos][] = [[[a, b], [c, r]]];
    const pa = partner(symMode, a, b, board);
    const pc = partner(symMode, c, r, board);
    if (pa && pc) {
      const used = new Set([key(a, b), key(c, r)]);
      if (!used.has(key(...pa)) && !used.has(key(...pc)) && key(...pa) !== key(...pc)) pairs.push([pa, pc]);
    }
    const cur = snap();
    for (const [[x1, y1], [x2, y2]] of pairs) {
      board[x1][y1] = cur[x2][y2];
      board[x2][y2] = cur[x1][y1];
    }
    record(before);
  }

  const cellAt = (el: Element | null): Pos | null => {
    const d = (el as HTMLElement | null)?.closest<HTMLElement>('[data-cell]');
    if (!d?.dataset.cell) return null;
    const [c, r] = d.dataset.cell.split(',').map(Number);
    return [c, r];
  };

  function onDown(e: PointerEvent) {
    const p = cellAt(e.target as Element);
    if (!p) return;
    e.preventDefault();
    cursor = p;
    if (e.button === 2 || e.altKey || tool === 'pick') return pickAt(...p);
    if (tool === 'swap') return swapAt(...p);
    stroke = { before: JSON.stringify(snap()), changed: false };
    paintAt(...p);
  }

  function onMove(e: PointerEvent) {
    const p = cellAt(document.elementFromPoint(e.clientX, e.clientY));
    if (stroke && p) paintAt(...p);
    hover = p;
  }

  function endStroke() {
    if (stroke?.changed) record(stroke.before);
    stroke = null;
  }

  // ── Keyboard: one focusable cell (roving tabindex), arrows move it, Enter /
  // Space apply the current tool, Alt+Enter picks, Escape drops a swap. ──────
  const cur = $derived.by((): Pos => {
    if (board.length === 0) return [0, 0];
    const c = Math.min(Math.max(cursor[0], 0), board.length - 1);
    const r = Math.min(Math.max(cursor[1], 0), board[c].length - 1);
    return [c, r];
  });

  async function moveCursor(c: number, r: number) {
    if (!board[c]) return;
    cursor = [c, Math.min(Math.max(r, 0), board[c].length - 1)];
    hover = cursor;
    await tick();
    gridEl?.querySelector<HTMLElement>(`[data-cell="${key(...cur)}"]`)?.focus();
  }

  function applyTool(c: number, r: number, pick = false) {
    if (pick || tool === 'pick') return pickAt(c, r);
    if (tool === 'swap') return swapAt(c, r);
    stroke = { before: JSON.stringify(snap()), changed: false };
    paintAt(c, r);
    endStroke();
  }

  function onGridKey(e: KeyboardEvent) {
    const p = cellAt(e.target as Element);
    if (!p) return;
    const [c, r] = p;
    const last = board.length - 1;
    switch (e.key) {
      case 'ArrowLeft':
        void moveCursor(Math.max(0, c - 1), r);
        break;
      case 'ArrowRight':
        void moveCursor(Math.min(last, c + 1), r);
        break;
      case 'ArrowUp':
        void moveCursor(c, r - 1);
        break;
      case 'ArrowDown':
        void moveCursor(c, r + 1);
        break;
      case 'Home':
        void moveCursor(0, r);
        break;
      case 'End':
        void moveCursor(last, r);
        break;
      case 'Enter':
      case ' ':
        applyTool(c, r, e.altKey);
        break;
      case 'Escape':
        if (!swapA) return;
        swapA = null;
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  /** What a screen reader hears for a cell: place, symbol, extra fields, flags. */
  function cellLabel(c: number, r: number, cell: BoardCell, len: number): string {
    const row = r === 0 ? 'top padding row' : r === len - 1 ? 'bottom padding row' : `row ${r}`;
    const parts = [`Reel ${c + 1}, ${row}: ${cell.name}`];
    const extra = badges(cell).map((b) => b.text);
    if (extra.length) parts.push(extra.join(', '));
    parts.push(...(analysis.notes.get(key(c, r)) ?? []));
    if (swapA && swapA[0] === c && swapA[1] === r) parts.push('selected for swap');
    return parts.join('; ');
  }

  function undo() {
    const prev = hist.at(-1);
    if (!prev) return;
    fut = [...fut, JSON.stringify(snap())];
    hist = hist.slice(0, -1);
    board = JSON.parse(prev);
  }

  function redo() {
    const next = fut.at(-1);
    if (!next) return;
    hist = [...hist, JSON.stringify(snap())];
    fut = fut.slice(0, -1);
    board = JSON.parse(next);
  }

  /** New ordinary symbols everywhere a paying symbol sits, until nothing connects. */
  function shuffle() {
    if (!rules) return;
    const before = JSON.stringify(snap());
    const strips = info?.stripSymbols;
    for (let t = 0; t < 800; t++) {
      const nb = snap();
      const done = new Set<string>();
      nb.forEach((reel, c) =>
        reel.forEach((cell, r) => {
          if (done.has(key(c, r)) || !rules.paying.includes(cell.name)) return;
          const pool = rules.paying.filter((s) => !strips || strips[c]?.includes(s));
          if (!pool.length) return;
          const pick = pool[Math.floor(Math.random() * pool.length)];
          nb[c][r] = { name: pick };
          done.add(key(c, r));
          const p = partner(symMode, c, r, nb);
          if (p && rules.paying.includes(nb[p[0]][p[1]].name) && (!strips || strips[p[0]]?.includes(pick))) {
            nb[p[0]][p[1]] = { name: pick };
            done.add(key(...p));
          }
        }),
      );
      if (findWins(nb, rules).length === 0) {
        board = nb;
        record(before);
        status = { kind: 'info', text: 'Ordinary symbols reshuffled — no paying connection.' };
        return;
      }
    }
    status = { kind: 'error', text: 'No reshuffle without a connection found — move a wild or change the symmetry.' };
  }

  function setProp(k: string, v: unknown) {
    brushProps[brush] = { ...(brushProps[brush] ?? {}), [k]: v };
  }

  function removeProp(k: string) {
    const { [k]: _, ...rest } = brushProps[brush] ?? {};
    brushProps[brush] = rest;
  }

  function addProp(e: SubmitEvent) {
    e.preventDefault();
    const k = newKey.trim();
    if (!k || k === 'name') return;
    const raw = newVal.trim();
    const v: unknown =
      raw === 'true' ? true : raw === 'false' ? false : raw !== '' && !Number.isNaN(Number(raw)) ? Number(raw) : raw;
    setProp(k, v);
    newKey = '';
    newVal = '';
  }

  // ── File ─────────────────────────────────────────────────────────────────
  const shortPath = (p: string) => p.split('/frontend/apps/').pop() ?? p;
  const now = () => new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

  async function apply(force = false) {
    if (!info) return;
    writing = true;
    const before = JSON.parse(JSON.stringify(info.board)) as BoardCell[][];
    try {
      const i = await api.writeBoard(info.game, snap(), info.baseHash, force);
      replaced = before;
      info = i;
      saved = canon(i.board);
      staleOnDisk = false;
      status = {
        kind: 'ok',
        text: `Written to ${shortPath(i.constantsPath)} at ${now()}. Rebuild the game's frontend to see it in Play.`,
      };
    } catch (e) {
      const msg = errorText(e);
      if (!force && msg.includes('changed on disk')) {
        // The Apply button turns into a two-step "Overwrite" (ConfirmButton).
        staleOnDisk = true;
        status = {
          kind: 'error',
          text: 'INITIAL_BOARD changed on disk since this board was loaded. Apply again to overwrite it with this board, or Reload from file.',
        };
      } else {
        status = { kind: 'error', text: msg };
      }
    } finally {
      writing = false;
    }
  }

  async function undoApply() {
    if (!info || !replaced) return;
    writing = true;
    try {
      const i = await api.writeBoard(info.game, replaced, info.baseHash);
      info = i;
      saved = canon(i.board);
      board = structuredClone(i.board);
      replaced = null;
      status = { kind: 'ok', text: `Previous board written back at ${now()}.` };
    } catch (e) {
      status = { kind: 'error', text: errorText(e) };
    } finally {
      writing = false;
    }
  }

  /** Confirmation (when dirty) is the ConfirmButton's job, not this one's. */
  function reload() {
    if (!info) return;
    void load(info.game, true);
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(preview);
      status = { kind: 'ok', text: 'Code copied.' };
    } catch {
      status = { kind: 'error', text: 'Clipboard unavailable — select the code and copy it.' };
    }
  }

  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement)?.closest('input, textarea, select')) return;
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key.toLowerCase() === 'z') {
        e.preventDefault();
        e.shiftKey ? redo() : undo();
      } else if (mod && e.key.toLowerCase() === 'y') {
        e.preventDefault();
        redo();
      } else if (!mod && e.key === 'p') tool = 'paint';
      else if (!mod && e.key === 'i') tool = 'pick';
      else if (!mod && e.key === 'e') tool = 'swap';
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener('pointerup', endStroke);
    window.addEventListener('pointercancel', endStroke);
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('pointerup', endStroke);
      window.removeEventListener('pointercancel', endStroke);
      clearTimeout(previewTimer);
    };
  });

  const mate = $derived.by(() => {
    if (!hover) return null;
    const p = partner(symMode, hover[0], hover[1], board);
    return p && !(p[0] === hover[0] && p[1] === hover[1]) ? key(...p) : null;
  });

  function badges(cell: BoardCell): { kind: string; text: string }[] {
    const out: { kind: string; text: string }[] = [];
    for (const [k, v] of Object.entries(cell)) {
      if (k === 'name' || typeof v === 'boolean') continue;
      if (k === 'multiplier' && typeof v === 'number') {
        if (v > 1) out.push({ kind: 'mult', text: `×${v}` });
      } else if (k === 'value' && typeof v === 'number') out.push({ kind: 'value', text: String(v) });
      else out.push({ kind: 'prop', text: `${k} ${typeof v === 'object' ? JSON.stringify(v) : v}` });
    }
    return out;
  }

  const paysTitle = (name: string) => {
    const s = info?.symbols.find((x) => x.name === name);
    if (!s) return name;
    const pays = s.pays.map((p) => `${p.min === p.max ? p.min : `${p.min}-${p.max}`}: ${p.pay}×`).join(' · ');
    return [name, s.special.join(', '), pays].filter(Boolean).join(' — ');
  };

  const MULTS = [2, 3, 5, 10, 25, 50, 100, 500];
  const TOOLS: { id: Tool; label: string; key: string; hint: string }[] = [
    { id: 'paint', label: 'Paint', key: 'P', hint: 'Paint cells with the brush' },
    { id: 'pick', label: 'Pick', key: 'I', hint: 'Pick a cell as the brush' },
    { id: 'swap', label: 'Swap', key: 'E', hint: 'Swap two cells' },
  ];
  const showMults = $derived(
    !!brush &&
      ('multiplier' in (brushProps[brush] ?? {}) ||
        !!info?.symbols.find((s) => s.name === brush)?.special.includes('multiplier')),
  );
</script>

{#snippet art(cell: BoardCell)}
  {#if info?.images[cell.name]}
    <img src={info.images[cell.name]} alt={cell.name} draggable="false" />
  {:else}
    <span class="sym-name">{cell.name}</span>
  {/if}
  {#each badges(cell) as b, i (i)}
    <span class="badge {b.kind}">{b.text}</span>
  {/each}
{/snippet}

<div class="board-page">
  <div class="head">
    <div>
      <h2>Entry board</h2>
      {#if info}
        <p class="dim path" title={info.constantsPath}>
          <code>INITIAL_BOARD</code> · {shortPath(info.constantsPath)}
          {#if rules}· {rules.type} · {info.numReels}×{Math.max(...info.numRows)}{/if}
        </p>
      {/if}
    </div>
    {#if info}
      <div class="head-actions">
        {#if dirty}<span class="pill yellow">unsaved</span>{:else}<span class="pill green">in sync with the file</span>{/if}
        {#if dirty}
          <ConfirmButton
            variant="plain"
            onconfirm={reload}
            disabled={loading}
            confirmLabel="Drop unsaved changes?"
            title="Reload INITIAL_BOARD from the file — your unsaved edits are dropped"
          >
            Reload from file
          </ConfirmButton>
        {:else}
          <button onclick={reload} disabled={loading}>{loading ? 'Reloading…' : 'Reload from file'}</button>
        {/if}
        {#if staleOnDisk}
          <ConfirmButton
            variant="primary"
            onconfirm={() => apply(true)}
            disabled={!dirty || writing}
            confirmLabel="Overwrite the file on disk?"
            title="INITIAL_BOARD changed on disk since this board was loaded"
          >
            {writing ? 'Writing…' : 'Apply to constants.ts'}
          </ConfirmButton>
        {:else}
          <button class="primary" onclick={() => apply()} disabled={!dirty || writing}>
            {writing ? 'Writing…' : 'Apply to constants.ts'}
          </button>
        {/if}
      </div>
    {/if}
  </div>

  {#if error}
    <div class="banner error">{error}</div>
  {:else if loading && !info}
    <p class="dim">Loading the board…</p>
  {/if}

  {#if info && rules}
    {#if status}
      <div
        class="banner {status.kind === 'error' ? 'error' : status.kind === 'ok' ? 'ok' : 'info'}"
        role={status.kind === 'error' ? 'alert' : 'status'}
      >
        {status.text}
        {#if status.kind === 'ok' && replaced}
          <button class="ghost push" onclick={undoApply} disabled={writing}>
            {writing ? 'Writing…' : 'Undo apply'}
          </button>
        {/if}
      </div>
    {/if}

    <section class="panel palette">
      <div class="pal-group">
        <p class="overline">Paying symbols</p>
        <div class="tiles">
          {#each palette.paying as n (n)}
            <button
              class="tile"
              class:on={brush === n}
              aria-pressed={brush === n}
              title={paysTitle(n)}
              onclick={() => {
                brush = n;
                tool = 'paint';
              }}
            >
              <span class="thumb">{@render art(brushCell(n))}</span>
              <span class="tile-name">{n}</span>
            </button>
          {/each}
        </div>
      </div>
      {#if palette.special.length}
        <div class="pal-group">
          <p class="overline">Specials</p>
          <div class="tiles">
            {#each palette.special as n (n)}
              <button
                class="tile"
                class:on={brush === n}
                aria-pressed={brush === n}
                title={paysTitle(n)}
                onclick={() => {
                  brush = n;
                  tool = 'paint';
                }}
              >
                <span class="thumb">{@render art(brushCell(n))}</span>
                <span class="tile-name">{n}</span>
              </button>
            {/each}
          </div>
        </div>
      {/if}
      <div class="pal-group props">
        <p class="overline">Brush · {brush}</p>
        <p class="faint small brush-help">
          Extra fields written into every cell you paint with <strong>{brush}</strong>
          (e.g. <code>multiplier: 25</code>) — they end up in <code>INITIAL_BOARD</code>.
        </p>
        <div class="prop-list">
          {#each Object.entries(brushProps[brush] ?? {}) as [k, v] (k)}
            <div class="prop">
              <label class="prop-key" for="brush-prop-{k}">{k}</label>
              {#if typeof v === 'boolean'}
                <input id="brush-prop-{k}" type="checkbox" checked={v} onchange={(e) => setProp(k, e.currentTarget.checked)} />
              {:else if typeof v === 'number'}
                <input id="brush-prop-{k}" type="number" value={v} onchange={(e) => setProp(k, Number(e.currentTarget.value))} />
              {:else}
                <input id="brush-prop-{k}" type="text" value={typeof v === 'string' ? v : JSON.stringify(v)} onchange={(e) => setProp(k, e.currentTarget.value)} />
              {/if}
              <button class="ghost x" title="Remove {k}" aria-label="Remove the {k} field" onclick={() => removeProp(k)}>
                <Icon name="close" size={12} />
              </button>
            </div>
          {/each}
          {#if showMults}
            <div class="chips" role="group" aria-label="Multiplier">
              {#each MULTS as m (m)}
                <button
                  class="chip"
                  class:on={brushProps[brush]?.multiplier === m}
                  aria-pressed={brushProps[brush]?.multiplier === m}
                  onclick={() => setProp('multiplier', m)}>×{m}</button>
              {/each}
            </div>
          {/if}
          <form class="add-prop" onsubmit={addProp}>
            <label>
              Field name
              <input placeholder="e.g. multiplier" bind:value={newKey} />
            </label>
            <label>
              Value
              <input placeholder="25 · true · text" bind:value={newVal} />
            </label>
            <button type="submit" disabled={!newKey.trim()}>Add field</button>
          </form>
        </div>
      </div>
    </section>

    <div class="work">
      <section class="panel board-wrap">
        <!-- Rows are `display: contents` wrappers: the cells keep their explicit
             grid-area, the wrappers only give the grid its row semantics. -->
        <div
          class="grid"
          bind:this={gridEl}
          style="--reels: {board.length}; grid-template-columns: 30px repeat({board.length}, minmax(0, 1fr));"
          onpointerdown={onDown}
          onpointermove={onMove}
          onpointerleave={() => (hover = null)}
          onkeydown={onGridKey}
          onfocusin={(e) => (hover = cellAt(e.target as Element))}
          onfocusout={() => (hover = null)}
          oncontextmenu={(e) => {
            if (cellAt(e.target as Element)) e.preventDefault();
          }}
          role="grid"
          tabindex="-1"
          aria-label="Entry board, {board.length} reels — tool: {tool}{swapA ? ', one cell selected for swap' : ''}"
          aria-describedby="board-legend"
        >
          <div class="grid-row" role="row">
            <span class="corner" role="columnheader" style="grid-area: 1 / 1"><span class="sr-only">Row</span></span>
            {#each board as _reel, c (c)}
              <span class="col-label" role="columnheader" style="grid-area: 1 / {c + 2}">
                <span aria-hidden="true">R{c + 1}</span><span class="sr-only">Reel {c + 1}</span>
              </span>
            {/each}
          </div>
          {#each Array(rowsMax) as _row, r (r)}
            <div class="grid-row" role="row">
              <span
                class="row-label"
                class:pad={r === 0 || r === rowsMax - 1}
                style="grid-area: {r + 2} / 1"
                role="rowheader"
              >
                {#if r === 0 || r === rowsMax - 1}
                  <span aria-hidden="true">pad</span><span class="sr-only">Padding row, outside the frame</span>
                {:else}
                  <span aria-hidden="true">{r}</span><span class="sr-only">Row {r}</span>
                {/if}
              </span>
              {#each board as reel, c (c)}
                {#if r < reel.length}
                  {@const cell = reel[r]}
                  <div
                    class="cell"
                    class:pad={r === 0 || r === reel.length - 1}
                    class:hov={hover && hover[0] === c && hover[1] === r}
                    class:mate={mate === key(c, r)}
                    class:win={analysis.win.has(key(c, r))}
                    class:bad={analysis.bad.has(key(c, r))}
                    class:warn={analysis.warn.has(key(c, r))}
                    class:sel={swapA && swapA[0] === c && swapA[1] === r}
                    data-cell={key(c, r)}
                    style="grid-area: {r + 2} / {c + 2}"
                    role="gridcell"
                    tabindex={cur[0] === c && cur[1] === r ? 0 : -1}
                    aria-label={cellLabel(c, r, cell, reel.length)}
                  >
                    {@render art(cell)}
                  </div>
                {/if}
              {/each}
            </div>
          {/each}
          {#if uniform && rowsMax > 2}
            <div class="frame" aria-hidden="true" style="grid-area: 3 / 2 / {rowsMax + 1} / {board.length + 2}"></div>
          {/if}
        </div>
        <p class="faint legend" id="board-legend">
          Click: apply the tool · drag: paint several · Alt-click or right-click: pick ·
          Keyboard: arrows move, Enter / Space apply the tool, Alt+Enter picks ·
          Tools: <kbd>P</kbd> paint, <kbd>I</kbd> pick, <kbd>E</kbd> swap ·
          ⌘Z / ⇧⌘Z: undo / redo · pad rows sit outside the frame.
        </p>
      </section>

      <aside class="side">
        <section class="panel">
          <p class="overline" id="board-tools">Tools</p>
          <div class="seg" role="group" aria-labelledby="board-tools">
            {#each TOOLS as t (t.id)}
              <button
                class:on={tool === t.id}
                aria-pressed={tool === t.id}
                title="{t.hint} ({t.key})"
                onclick={() => (tool = t.id)}
              >
                {t.label} <kbd>{t.key}</kbd>
              </button>
            {/each}
          </div>
          <p class="overline sub" id="board-sym">Symmetry while painting</p>
          <div class="seg wrap" role="group" aria-labelledby="board-sym">
            {#each modes as m (m)}
              <button
                class:on={symMode === m}
                aria-pressed={symMode === m}
                aria-label={SYM_ARIA[m]}
                title={SYM_ARIA[m]}
                onclick={() => (symMode = m)}
              >
                {SYM_LABELS[m]}
              </button>
            {/each}
          </div>
          <div class="row">
            <button onclick={undo} disabled={hist.length === 0}>Undo</button>
            <button onclick={redo} disabled={fut.length === 0}>Redo</button>
            <button onclick={shuffle} title="New ordinary symbols, keeping wilds and specials, without any connection">Reshuffle ordinary</button>
          </div>
        </section>

        <section class="panel">
          <p class="overline">What the board shows</p>
          <ul class="issues">
            {#each analysis.issues as i, n (n)}
              <li class="issue {i.level}">
                <span class="pill {i.level === 'fail' ? 'red' : i.level === 'warn' ? 'yellow' : i.level === 'pass' ? 'green' : 'blue'}">{i.level}</span>
                <div>
                  <div>{i.title}</div>
                  {#if i.detail}<div class="faint small">{i.detail}</div>{/if}
                </div>
              </li>
            {/each}
          </ul>
          <p class="overline sub">Symmetries</p>
          <p class="faint small sym-help">
            Pairs of cells each symmetry says should match — how many differ.
          </p>
          <ul class="chips sym-list">
            {#each symStatus as s (s.m)}
              <li
                class="pill {s.n === 0 ? 'green' : 'neutral'}"
                title={s.n === 0
                  ? `${SYM_ARIA[s.m]}: the board is symmetric`
                  : `${SYM_ARIA[s.m]}: ${s.n} cell pair${s.n > 1 ? 's' : ''} do not match`}
              >
                <span aria-hidden="true">{SYM_LABELS[s.m]}</span>
                <span class="sr-only">{SYM_ARIA[s.m]}:</span>
                {s.n === 0 ? 'symmetric' : `${s.n} pair${s.n > 1 ? 's' : ''} off`}
              </li>
            {/each}
          </ul>
          {#each info.notes as note (note)}
            <p class="faint small note">{note}</p>
          {/each}
        </section>

        <section class="panel">
          <div class="code-head">
            <p class="overline">Code written by Apply</p>
            <button class="ghost" onclick={copy}>Copy</button>
          </div>
          <!-- Scrollable: focusable so the keyboard can scroll it. -->
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <pre class="code" tabindex="0" aria-label="Code written by Apply">{preview}</pre>
        </section>
      </aside>
    </div>
  {/if}
</div>

<style>
  .board-page {
    display: grid;
    gap: 14px;
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    justify-content: space-between;
    gap: 10px 20px;
  }

  .head h2 {
    margin: 0 0 4px;
  }

  .path {
    margin: 0;
    font-size: var(--fs-sm);
  }

  .head-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .banner .push {
    margin-left: auto;
  }

  .palette {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 16px 22px;
    padding: 14px 16px;
  }

  .pal-group .overline {
    margin-bottom: 8px;
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(54px, 1fr));
    gap: 6px;
  }

  .tile {
    display: grid;
    justify-items: center;
    gap: 2px;
    padding: 5px 3px 4px;
    background: var(--bg-deep);
    border: 1px solid var(--border);
    border-radius: var(--r-m);
  }

  .tile.on {
    border-color: var(--gold);
    box-shadow: inset 0 0 0 1px var(--gold);
  }

  .thumb {
    position: relative;
    width: 100%;
    aspect-ratio: 1;
    container-type: size;
    display: grid;
    place-items: center;
  }

  .tile-name {
    font-size: var(--fs-2xs);
    font-weight: 640;
    color: var(--text-dim);
    letter-spacing: 0.04em;
  }

  .prop-list {
    display: grid;
    gap: 6px;
  }

  .brush-help {
    margin: 0 0 var(--sp-2);
  }

  .prop {
    display: grid;
    grid-template-columns: 84px minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--sp-2);
    font-size: var(--fs-sm);
  }

  .prop-key {
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .prop input[type='checkbox'] {
    justify-self: start;
  }

  .x {
    display: inline-grid;
    place-items: center;
    padding: var(--sp-1) var(--sp-2);
  }

  .add-prop {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) auto;
    align-items: end;
    gap: var(--sp-2);
  }

  .add-prop label {
    display: grid;
    gap: 3px;
  }

  .add-prop input {
    min-width: 0;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-1);
  }

  .chip {
    padding: 2px 9px;
    border-radius: var(--r-pill);
    font-size: var(--fs-sm);
    font-variant-numeric: tabular-nums;
  }

  .chip.on {
    background: var(--gold);
    border-color: var(--gold);
    color: var(--gold-ink);
  }

  .work {
    display: grid;
    gap: 14px;
    grid-template-columns: minmax(0, 1fr);
    align-items: start;
  }

  @media (min-width: 1100px) {
    .work {
      grid-template-columns: minmax(0, 1fr) 380px;
    }
  }

  .board-wrap {
    padding: 14px 16px;
    background: radial-gradient(
      120% 80% at 50% -10%,
      color-mix(in oklch, var(--info) var(--tint), var(--bg-raised)),
      var(--bg-panel) 60%
    );
  }

  .grid {
    display: grid;
    gap: 6px;
    width: min(100%, calc(30px + var(--reels) * 112px));
    margin: 0 auto;
    user-select: none;
    touch-action: none;
  }

  /* Row wrappers carry the ARIA rows only; the cells sit on the grid itself. */
  .grid-row {
    display: contents;
  }

  .col-label,
  .row-label {
    display: grid;
    place-items: center;
    font-size: var(--fs-xs);
    font-weight: 640;
    letter-spacing: 0.08em;
    color: var(--text-faint);
  }

  .row-label.pad {
    text-transform: uppercase;
    font-size: var(--fs-2xs);
  }

  .cell {
    position: relative;
    aspect-ratio: 1;
    container-type: size;
    display: grid;
    place-items: center;
    border-radius: var(--r-m);
    background: color-mix(in oklch, var(--bg-deep) 70%, transparent);
    border: 1px solid color-mix(in oklch, var(--text) 4%, transparent);
    cursor: crosshair;
  }

  .cell.pad {
    opacity: 0.5;
    background: repeating-linear-gradient(
      135deg,
      color-mix(in oklch, var(--text) 3.5%, transparent) 0 6px,
      transparent 6px 12px
    );
  }

  .cell.hov {
    box-shadow: inset 0 0 0 2px var(--gold);
  }

  .cell.mate {
    box-shadow: inset 0 0 0 2px color-mix(in oklch, var(--gold) 45%, transparent);
  }

  .cell.win {
    box-shadow: inset 0 0 0 2px var(--warn), 0 0 12px color-mix(in oklch, var(--warn) 35%, transparent);
  }

  .cell.warn {
    box-shadow: inset 0 0 0 2px color-mix(in oklch, var(--warn) 60%, transparent);
  }

  .cell.bad {
    box-shadow: inset 0 0 0 2px var(--fail), 0 0 12px color-mix(in oklch, var(--fail) 40%, transparent);
  }

  .cell.sel {
    box-shadow: inset 0 0 0 3px var(--gold);
  }

  .cell :global(img),
  .thumb :global(img) {
    position: absolute;
    inset: 6%;
    width: 88%;
    height: 88%;
    object-fit: contain;
    pointer-events: none;
  }

  .sym-name {
    font-family: var(--font-mono);
    font-weight: 600;
    font-size: 26cqh;
    color: var(--text);
    pointer-events: none;
  }

  .badge {
    position: absolute;
    left: 50%;
    bottom: 6%;
    transform: translateX(-50%);
    padding: 0 0.45em;
    border-radius: var(--r-pill);
    font-family: var(--font-mono);
    font-weight: 700;
    font-size: max(10px, 17cqh);
    line-height: 1.25;
    white-space: nowrap;
    pointer-events: none;
    background: color-mix(in oklch, var(--bg-deep) 85%, transparent);
    color: var(--text);
    border: 1px solid var(--border-strong);
  }

  .badge.mult {
    color: var(--gold-ink);
    background: var(--gold);
    border-color: var(--gold-deep);
  }

  .badge.value {
    top: 50%;
    bottom: auto;
    transform: translate(-50%, -50%);
    font-size: max(12px, 30cqh);
    background: none;
    border: none;
    color: var(--text);
    text-shadow: 0 1px 3px color-mix(in oklch, black 90%, transparent);
  }

  .badge.prop {
    top: 6%;
    bottom: auto;
    font-size: max(9px, 11cqh);
    font-weight: 500;
  }

  .frame {
    margin: -4px;
    border: 2px solid var(--border-strong);
    border-radius: calc(var(--r-m) + 3px);
    pointer-events: none;
  }

  .legend {
    margin: 10px 0 0;
    font-size: var(--fs-sm);
    text-align: center;
  }

  kbd {
    font-family: var(--font-mono);
    font-size: 0.9em;
    padding: 0 4px;
    border: 1px solid var(--border);
    border-radius: var(--r-s);
    color: var(--text-dim);
  }

  .side {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 14px;
    min-width: 0;
  }

  .side .panel {
    padding: 14px var(--sp-4);
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 10px;
    min-width: 0;
  }

  .overline.sub {
    margin: 4px 0 0;
  }

  .seg {
    display: flex;
    gap: 4px;
    padding: 3px;
    background: var(--bg-deep);
    border: 1px solid var(--border);
    border-radius: var(--r-m);
  }

  .seg.wrap {
    flex-wrap: wrap;
  }

  .seg button {
    flex: 1 1 auto;
    background: none;
    border-color: transparent;
    color: var(--text-dim);
    white-space: nowrap;
  }

  .seg kbd {
    margin-left: var(--sp-1);
    font-size: var(--fs-2xs);
    color: var(--text-faint);
  }

  .seg button.on {
    background: var(--bg-raised);
    border-color: var(--border-strong);
    color: var(--text);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .sym-help {
    margin: 0;
  }

  .sym-list {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  /* Neutral LED: the count is the information, not a verdict. */
  .pill.neutral {
    color: var(--text-dim);
    border-color: var(--border);
  }

  .issues {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 8px;
  }

  .issue {
    display: grid;
    grid-template-columns: 48px minmax(0, 1fr);
    gap: 10px;
    align-items: start;
    font-size: var(--fs-base);
  }

  .issue .pill {
    justify-content: center;
  }

  .small {
    font-size: var(--fs-sm);
  }

  .note {
    margin: 0;
  }

  .code-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .code-head .overline {
    flex: 1;
    margin: 0;
  }

  .code {
    margin: 0;
    min-width: 0;
    max-width: 100%;
    max-height: 260px;
    overflow: auto;
    overflow-x: auto;
    padding: 10px 12px;
    background: var(--bg-deep);
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    line-height: 1.55;
    tab-size: 2;
    white-space: pre;
  }
</style>
