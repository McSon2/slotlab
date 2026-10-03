<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { api } from '../lib/api';
  import { app, errorText, notify, run, selectedGame } from '../lib/stores.svelte';
  import ConfirmButton from '../components/ConfirmButton.svelte';
  import Icon from '../components/Icon.svelte';

  interface ShareStatus {
    active: boolean;
    game?: string;
    url?: string;
    playUrl?: string;
    balance?: number;
    currency?: string;
    language?: string;
    social?: boolean;
  }

  interface ShareSession {
    sessionID: string;
    balance: number;
    currency: string;
    spins: number;
    wagered: number;
    won: number;
    inRound: boolean;
  }

  const MICRO = 1_000_000;

  let status = $state<ShareStatus>({ active: false });
  let statusLoaded = $state(false);
  let statusError = $state<string | null>(null);
  let sessions = $state<ShareSession[]>([]);
  let sessionsError = $state<string | null>(null);
  let buildDir = $state('');
  /** Game the build folder was filled for: switching game re-suggests it. */
  let buildDirGame = '';
  let balanceDollars = $state(10000);
  let currency = $state('USD');
  let language = $state('en');
  let social = $state(false);
  let busy = $state(false);
  let browsing = $state(false);
  let error = $state<string | null>(null);
  let poll: ReturnType<typeof setInterval> | null = null;
  let alive = true;

  const game = $derived(selectedGame());
  const buildFound = $derived(!!(game?.servedUrl || game?.buildDir));

  onMount(() => {
    void refresh();
    // The sessions panel is the proof the link is multi-player: one row
    // appears per visitor who opened it.
    poll = setInterval(() => {
      if (status.active) void refreshSessions();
    }, 2000);
  });

  onDestroy(() => {
    alive = false;
    if (poll) clearInterval(poll);
  });

  $effect(() => {
    // Build folder: the auto-detected build, else a guess from publish_dir.
    if (game && game.name !== buildDirGame) {
      buildDirGame = game.name;
      const m = game.publishDir.match(/^(.*)\/math\/games\//);
      buildDir = game.buildDir ?? (m ? `${m[1]}/frontend/apps/${game.name}/build` : '');
    }
  });

  /** Share routes answer 200 with `{ error }` on failure: surface it. */
  async function json<T>(url: string, init?: RequestInit): Promise<T> {
    const r = await fetch(url, {
      ...init,
      headers: init?.body ? { 'Content-Type': 'application/json' } : undefined,
    });
    let body: unknown = null;
    try {
      body = await r.json();
    } catch {
      /* empty or non-JSON body */
    }
    const err = (body as { error?: string } | null)?.error;
    if (err) throw new Error(err);
    if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
    return body as T;
  }

  function money(micro: number, cur: string): string {
    return `${(micro / MICRO).toLocaleString('en-US', {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    })} ${cur}`;
  }

  function shortId(id: string): string {
    return id.replace(/^share-/, '').slice(0, 8);
  }

  async function refresh() {
    try {
      const s = await json<ShareStatus>('/api/ui/share/status');
      if (!alive) return;
      status = s;
      statusError = null;
      if (s.active) {
        if (s.balance) balanceDollars = s.balance / MICRO;
        if (s.currency) currency = s.currency;
        if (s.language) language = s.language;
        social = !!s.social;
        await refreshSessions();
      } else {
        sessions = [];
      }
    } catch (e) {
      if (alive) statusError = errorText(e);
    } finally {
      statusLoaded = true;
    }
  }

  /** Called every 2 s: a failure is shown in place, never thrown. */
  async function refreshSessions() {
    try {
      const r = await json<{ sessions?: ShareSession[] }>('/api/ui/share/sessions');
      if (!alive) return;
      sessions = r.sessions ?? [];
      sessionsError = null;
    } catch (e) {
      if (alive) sessionsError = errorText(e);
    }
  }

  async function start(e?: SubmitEvent) {
    e?.preventDefault();
    if (!app.selectedGame || !buildDir.trim()) {
      error = 'Provide the game build folder.';
      return;
    }
    busy = true;
    error = null;
    try {
      await json('/api/ui/share/start', {
        method: 'POST',
        body: JSON.stringify({
          game: app.selectedGame,
          buildDir: buildDir.trim(),
          balance: Math.max(1, Math.round(balanceDollars * MICRO)),
          currency,
          language,
          social,
        }),
      });
      await refresh();
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  async function stop() {
    const r = await run(() => json('/api/ui/share/stop', { method: 'POST' }), {
      ok: 'Sharing stopped',
      error: 'Could not stop sharing',
    });
    if (r !== undefined) await refresh();
  }

  async function resetSession(sessionID: string) {
    const r = await run(
      () =>
        json('/api/ui/share/sessions/reset', {
          method: 'POST',
          body: JSON.stringify({ sessionID }),
        }),
      { ok: 'Balance reset', error: 'Could not reset the balance' },
    );
    if (r !== undefined) await refreshSessions();
  }

  async function copy() {
    if (!status.playUrl) return;
    try {
      await navigator.clipboard.writeText(status.playUrl);
      notify('Link copied');
    } catch (e) {
      notify(`Could not copy: ${errorText(e)}`, 'error');
    }
  }

  async function browse() {
    browsing = true;
    const r = await run(() => api.pickFolder(), { error: 'Could not open the folder picker' });
    browsing = false;
    if (!r || r.cancelled) return;
    if (r.error) notify(r.error, 'error');
    else if (r.picked) buildDir = r.picked;
  }
</script>

<div class="panel">
  <h2 class="section-title">
    {#if status.active}
      Sharing <span class="game-name">{status.game}</span>
    {:else if game}
      Share <span class="game-name">{game.name}</span>
    {:else}
      Share a game
    {/if}
  </h2>
  <p class="dim intro">
    Serves the game's static build and the emulated RGS through a free Cloudflare
    quick tunnel. Every visitor gets their own session and demo balance; the link
    dies when you stop sharing or close SlotLab.
  </p>

  {#if statusError}
    <p class="banner error" role="alert">Could not read the share status: {statusError}</p>
  {/if}

  {#if !statusLoaded}
    <p class="dim" role="status">Checking the share status…</p>
  {:else if status.active}
    <div class="active-share">
      <p>
        <span class="pill green">Active</span>
        <span class="dim">
          {money(status.balance ?? 0, status.currency ?? 'USD')} per visitor,
          {status.language}{status.social ? ', social mode' : ''}
        </span>
      </p>
      <div class="link-row">
        <input readonly value={status.playUrl} aria-label="Public link" />
        <button class="primary" onclick={copy}>Copy link</button>
        <a class="button-like" href={status.playUrl} target="_blank" rel="noreferrer">Open</a>
        <ConfirmButton onconfirm={stop} confirmLabel="Stop sharing for everyone?">
          Stop
        </ConfirmButton>
      </div>
    </div>
  {:else if game}
    <ul class="checklist" aria-label="Prerequisites">
      <li>
        <span class="pill blue">Required</span>
        <span>
          <code>cloudflared</code> installed —
          <code>brew install cloudflared</code> <span class="faint">(checked when sharing starts)</span>
        </span>
      </li>
      <li>
        {#if game.imported}
          <span class="pill green">Pass</span>
          <span>Books imported</span>
        {:else}
          <span class="pill red">Fail</span>
          <span>Books not imported — use Import books on the Games page</span>
        {/if}
      </li>
      <li>
        {#if buildFound}
          <span class="pill green">Pass</span>
          <span>Frontend build found</span>
        {:else}
          <span class="pill red">Fail</span>
          <span>
            No frontend build detected — run
            <code>pnpm --filter {game.name} build</code>, or point to it below
          </span>
        {/if}
      </li>
    </ul>

    <form class="form" onsubmit={start}>
      <label class="wide">
        <span>Frontend build folder</span>
        <span class="path-row">
          <input
            bind:value={buildDir}
            placeholder="/…/frontend/apps/{game.name}/build"
          />
          <button type="button" onclick={browse} disabled={browsing}>
            <Icon name="folder" size={14} /> Browse…
          </button>
        </span>
      </label>
      <label>
        <span>Balance per visitor</span>
        <input type="number" min="1" step="any" bind:value={balanceDollars} />
      </label>
      <label>
        <span>Currency</span>
        <select bind:value={currency}>
          {#each ['USD', 'EUR', 'GBP', 'CAD', 'BRL', 'JPY'] as c}
            <option value={c}>{c}</option>
          {/each}
        </select>
      </label>
      <label>
        <span>Language</span>
        <select bind:value={language}>
          {#each ['en', 'fr', 'es', 'pt', 'de'] as l}
            <option value={l}>{l}</option>
          {/each}
        </select>
      </label>
      <label class="check">
        <span>Social mode</span>
        <span class="box"><input type="checkbox" bind:checked={social} /></span>
      </label>
      <button type="submit" class="primary" disabled={busy || !buildDir.trim()}>
        {busy ? 'Starting the tunnel…' : 'Start sharing'}
      </button>
    </form>
  {:else}
    <p class="dim">Select a loaded game.</p>
  {/if}

  {#if error}
    <p class="field-error" role="alert">{error}</p>
  {/if}
</div>

{#if status.active}
  <div class="panel">
    <h2 class="section-title">Visitors ({sessions.length})</h2>
    {#if sessionsError}
      <p class="field-error">Visitor list unavailable: {sessionsError}</p>
    {/if}
    {#if sessions.length === 0}
      <p class="dim">
        Nobody has opened the link yet. A row appears as soon as someone does.
      </p>
    {:else}
      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              <th>Session</th>
              <th class="num">Spins</th>
              <th class="num">Wagered</th>
              <th class="num">Won</th>
              <th class="num">RTP</th>
              <th class="num">Balance</th>
              <th><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {#each sessions as s (s.sessionID)}
              <tr>
                <td class="mono">
                  {shortId(s.sessionID)}
                  {#if s.inRound}<span class="pill blue tiny">Spinning</span>{/if}
                </td>
                <td class="num">{s.spins.toLocaleString('en-US')}</td>
                <td class="num">{money(s.wagered, s.currency)}</td>
                <td class="num">{money(s.won, s.currency)}</td>
                <td class="num">
                  {s.wagered > 0 ? `${((s.won / s.wagered) * 100).toFixed(1)} %` : '—'}
                </td>
                <td class="num">{money(s.balance, s.currency)}</td>
                <td class="num">
                  <ConfirmButton
                    onconfirm={() => resetSession(s.sessionID)}
                    confirmLabel="Reset this balance?"
                    ariaLabel="Reset balance of session {shortId(s.sessionID)}"
                  >
                    Reset balance
                  </ConfirmButton>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
{/if}

<style>
  .panel + .panel {
    margin-top: var(--sp-4);
  }

  .section-title {
    font-size: var(--fs-md);
    margin: 0 0 var(--sp-2);
  }

  .game-name {
    font-family: var(--font-mono);
    color: var(--gold);
  }

  .intro {
    margin: 0 0 var(--sp-4);
    max-width: 80ch;
  }

  .checklist {
    list-style: none;
    padding: 0;
    margin: 0 0 var(--sp-4);
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    font-size: var(--fs-sm);
  }

  .checklist li {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
  }

  .checklist .pill {
    flex-shrink: 0;
    min-width: 5.5em;
    justify-content: flex-start;
  }

  .form {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-3);
    align-items: flex-end;
  }

  .form label {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    font-size: var(--fs-xs);
    color: var(--text-dim);
  }

  .form label.wide {
    flex: 1;
    min-width: min(380px, 100%);
  }

  .path-row {
    display: flex;
    gap: var(--sp-2);
  }

  .path-row input {
    flex: 1;
    min-width: 0;
  }

  .path-row button {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    white-space: nowrap;
  }

  /* Same label-on-top layout as the fields; the box gets an input's height
     so every control sits on one baseline. */
  .form label.check .box {
    display: flex;
    align-items: center;
    height: 31px;
  }

  .active-share p {
    margin: 0 0 var(--sp-3);
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex-wrap: wrap;
  }

  .active-share .link-row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
    align-items: center;
  }

  .link-row input {
    flex: 1;
    min-width: min(280px, 100%);
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
  }

  /* A link that reads as a ghost button (never <a><button>). */
  .button-like {
    display: inline-flex;
    align-items: center;
    padding: 6px 14px;
    border: 1px solid transparent;
    border-radius: var(--r-s);
    color: var(--text-dim);
    font-size: var(--fs-sm);
    font-weight: 560;
    text-decoration: none;
  }

  .button-like:hover,
  .button-like:visited:hover {
    color: var(--text);
    background: var(--bg-raised);
  }

  .button-like:visited {
    color: var(--text-dim);
  }

  .table-wrap {
    overflow-x: auto;
  }

  td.mono {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
  }

  td .pill.tiny {
    margin-left: var(--sp-1);
  }
</style>
