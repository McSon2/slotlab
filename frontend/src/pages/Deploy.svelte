<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { app, errorText, notify, selectedGame } from '../lib/stores.svelte';
  import ConfirmButton from '../components/ConfirmButton.svelte';

  interface Cfg {
    sshTarget: string; remoteDir: string; hubUrl: string;
    adminToken: string; cfToken: string; cfZoneId: string;
    cfAccountId: string; cfTunnelId: string; baseDomain: string;
    tunnelTarget: string;
    hubSourceDir: string; hubRemoteSrc: string; hubCt: string;
    hubBuildDir: string; hubStackDir: string; hubImage: string;
    pauseDuringDeploy: string;
    configured?: boolean; localIdxVersion?: number;
  }
  interface Step { label: string; state: 'pending' | 'running' | 'done' | 'failed'; detail?: string }
  interface Job { running: boolean; game: string; steps: Step[]; url?: string; error?: string }
  interface Published {
    name: string; host: string; url: string; balance: number;
    currency: string; language: string; social: boolean; accessCode: string;
    console: boolean;
  }

  const EMPTY: Cfg = {
    sshTarget: '', remoteDir: '', hubUrl: '', adminToken: '', cfToken: '',
    cfZoneId: '', cfAccountId: '', cfTunnelId: '', baseDomain: '', tunnelTarget: '',
    hubSourceDir: '', hubRemoteSrc: '', hubCt: '', hubBuildDir: '', hubStackDir: '', hubImage: '',
    pauseDuringDeploy: '',
  };

  const STATE_TEXT: Record<Step['state'], string> = {
    done: 'done', failed: 'failed', running: 'running', pending: 'pending',
  };

  let cfg = $state<Cfg>({ ...EMPTY });
  let loaded = $state(false);
  let loadError = $state<string | null>(null);
  let showSettings = $state(false);
  let published = $state<Published[]>([]);
  /** Book-index format the hub reads (null = unknown / hub unreachable). */
  let hubIdx = $state<number | null>(null);
  let publishedError = $state<string | null>(null);
  let job = $state<Job>({ running: false, game: '', steps: [] });
  let jobError = $state<string | null>(null);
  let error = $state<string | null>(null);
  let publishing = $state(false);
  let saving = $state(false);
  let saved = $state(false);
  let showAdminToken = $state(false);
  let showCfToken = $state(false);
  let alive = true;

  // Publish settings
  let subdomain = $state('');
  let balance = $state(10000);
  let currency = $state('USD');
  let language = $state('en');
  let social = $state(false);
  let accessCode = $state('');
  let showConsole = $state(false);
  let touched = $state(false);
  let prefillFor = '';

  const game = $derived(selectedGame());
  const existing = $derived(published.find((p) => p.name === app.selectedGame));

  onMount(() => {
    void refresh();
  });
  onDestroy(() => {
    alive = false;
  });

  // The 1 s job timer only runs while a deployment does — and stops with the page.
  $effect(() => {
    if (!job.running) return;
    const t = setInterval(() => void pollJob(), 1000);
    return () => clearInterval(t);
  });

  $effect(() => {
    // Another game: its own settings, pre-filled again.
    const name = game?.name ?? '';
    if (name !== prefillFor) {
      prefillFor = name;
      touched = false;
      subdomain = '';
    }
  });

  $effect(() => {
    // Pre-fill from what is already live, as long as nothing was touched.
    if (!touched && game) {
      if (existing) {
        subdomain = existing.host.replace(`.${cfg.baseDomain}`, '');
        balance = existing.balance;
        currency = existing.currency;
        language = existing.language;
        social = existing.social;
        accessCode = existing.accessCode;
        showConsole = !!existing.console;
      } else if (!subdomain) {
        subdomain = game.name.replace(/_/g, '-');
      }
    }
  });

  /** Deploy routes answer 200 with `{ error }` on failure: surface it. */
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

  async function refresh() {
    try {
      const c = await json<Cfg>('/api/ui/deploy/config');
      if (!alive) return;
      cfg = c;
      loadError = null;
    } catch (e) {
      if (alive) loadError = errorText(e);
    }
    await Promise.all([loadPublished(), pollJob()]);
    loaded = true;
  }

  async function loadPublished() {
    try {
      // On a hub error the route still lists `games: []` next to `error`.
      const r = await fetch('/api/ui/deploy/published');
      const p: { games?: Published[]; error?: string; idxVersion?: number } = await r.json();
      if (!alive) return;
      published = p.games ?? [];
      // A hub older than the field reads version 1.
      hubIdx = p.error ? null : (p.idxVersion ?? 1);
      publishedError = p.error ?? (r.ok ? null : `${r.status} ${r.statusText}`);
    } catch (e) {
      if (alive) publishedError = errorText(e);
    }
  }

  async function pollJob() {
    try {
      const wasRunning = job.running;
      // Not `json()`: a failed job carries its own `error`, which is not a
      // failure to read the status.
      const r = await fetch('/api/ui/deploy/status');
      if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
      const j = (await r.json()) as Job;
      if (!alive) return;
      job = j;
      jobError = null;
      // A deployment just ended: the live list changed.
      if (wasRunning && !j.running) void loadPublished();
    } catch (e) {
      if (!alive) return;
      jobError = errorText(e);
      job = { ...job, running: false };
    }
  }

  async function saveConfig(e: SubmitEvent) {
    e.preventDefault();
    saving = true;
    try {
      cfg = await json<Cfg>('/api/ui/deploy/config', {
        method: 'POST',
        body: JSON.stringify(cfg),
      });
      saved = true;
      setTimeout(() => (saved = false), 2000);
      notify('Deployment settings saved');
      await refresh();
    } catch (err) {
      notify(`Could not save the settings: ${errorText(err)}`, 'error');
    } finally {
      saving = false;
    }
  }

  function onPublishSubmit(e: SubmitEvent) {
    e.preventDefault();
    // Replacing a live game goes through the two-step button, never a bare Enter.
    if (!existing) void publish();
  }

  async function publish() {
    if (!app.selectedGame) return;
    error = null;
    publishing = true;
    try {
      await json('/api/ui/deploy/start', {
        method: 'POST',
        body: JSON.stringify({
          game: app.selectedGame, subdomain, balance, currency, language, social,
          accessCode, console: showConsole,
        }),
      });
      await pollJob();
    } catch (err) {
      error = errorText(err);
    } finally {
      publishing = false;
    }
  }

  async function copyLink(url: string) {
    try {
      await navigator.clipboard.writeText(url);
      notify('Link copied');
    } catch (e) {
      notify(`Could not copy: ${errorText(e)}`, 'error');
    }
  }

  function glyph(s: Step['state']): string {
    return s === 'done' ? '✓' : s === 'failed' ? '✗' : s === 'running' ? '…' : '·';
  }

</script>

<div class="panel">
  <h2 class="section-title">
    {#if game}Publish <span class="game-name">{game.name}</span> to the homelab{:else}Publish to the homelab{/if}
  </h2>
  <p class="dim intro">
    Sends math, build and books to the permanent server over the LAN. The hub picks
    the game up <strong>without restarting</strong> (current visitors keep their
    session), then the Cloudflare subdomain is created. A game already live is
    updated in place.
  </p>

  {#if !loaded}
    <p class="dim" role="status">Loading the deployment status…</p>
  {:else}
    {#if loadError}
      <p class="banner error" role="alert">Could not read the deployment settings: {loadError}</p>
    {:else if !cfg.configured}
      <div class="banner warn">
        <span>
          Settings incomplete — fill in at least the SSH target, the remote folder,
          the hub URL and its token.
        </span>
        {#if !showSettings}
          <button onclick={() => (showSettings = true)} aria-controls="deploy-settings">
            Open deployment settings
          </button>
        {/if}
      </div>
    {/if}

    {#if hubIdx != null && cfg.localIdxVersion && hubIdx !== cfg.localIdxVersion}
      <div class="banner info" role="status">
        <span>
          The hub reads book index v{hubIdx}, this SlotLab writes v{cfg.localIdxVersion}.
          Publishing updates the hub first: sources sent, image rebuilt (~5 min), then it
          restarts — visitors in progress lose their session, and the other live games get
          their books re-sent in the new format.
        </span>
      </div>
    {/if}

    {#if game}
      <form class="form" onsubmit={onPublishSubmit}>
        <label>
          <span>Subdomain</span>
          <input bind:value={subdomain} oninput={() => (touched = true)} placeholder="skullperch" />
        </label>
        <label>
          <span>Balance per visitor</span>
          <input type="number" min="1" step="any" bind:value={balance} oninput={() => (touched = true)} />
        </label>
        <label>
          <span>Currency</span>
          <select bind:value={currency} onchange={() => (touched = true)}>
            {#each ['USD', 'EUR', 'GBP', 'CAD', 'BRL', 'JPY'] as c}<option>{c}</option>{/each}
          </select>
        </label>
        <label>
          <span>Language</span>
          <select bind:value={language} onchange={() => (touched = true)}>
            {#each ['en', 'fr', 'es', 'pt', 'de'] as l}<option>{l}</option>{/each}
          </select>
        </label>
        <label>
          <span>Access code (empty = open)</span>
          <input bind:value={accessCode} oninput={() => (touched = true)} placeholder="SKULL-1234" />
        </label>
        <label class="check">
          <span>Social mode</span>
          <span class="box">
            <input type="checkbox" bind:checked={social} onchange={() => (touched = true)} />
          </span>
        </label>
        <label class="check" title="Floating panel in the game to pick the next round's outcome">
          <span>Demo console</span>
          <span class="box">
            <input type="checkbox" bind:checked={showConsole} onchange={() => (touched = true)} />
          </span>
        </label>
        {#if existing}
          <ConfirmButton
            variant="primary"
            onconfirm={publish}
            confirmLabel="Replace the live version?"
            disabled={job.running || publishing || !cfg.configured}
          >
            {job.running || publishing ? 'Publishing…' : 'Update'}
          </ConfirmButton>
        {:else}
          <button
            type="submit"
            class="primary"
            disabled={job.running || publishing || !cfg.configured}
          >
            {job.running || publishing ? 'Publishing…' : 'Publish'}
          </button>
        {/if}
      </form>
      {#if existing}
        <p class="dim live-now">
          Already live: <a href={existing.url} target="_blank" rel="noreferrer">{existing.url}</a>
        </p>
      {/if}
    {:else}
      <p class="dim">Select a game.</p>
    {/if}

    {#if error}<p class="field-error" role="alert">{error}</p>{/if}
  {/if}
</div>

{#if job.steps.length || jobError}
  <div class="panel">
    <h2 class="section-title">
      {job.running ? 'Publishing' : 'Last deployment'}{job.game ? ` — ${job.game}` : ''}
    </h2>
    {#if jobError}
      <p class="field-error">Could not read the deployment progress: {jobError}</p>
    {/if}
    <ul class="steps" aria-live="polite">
      {#each job.steps as s}
        <li class={s.state}>
          <span class="g" aria-hidden="true">{glyph(s.state)}</span>
          <span class="l">{s.label}</span>
          <span class="sr-only">({STATE_TEXT[s.state]})</span>
          {#if s.detail}<span class="d">{s.detail}</span>{/if}
        </li>
      {/each}
    </ul>
    {#if job.url}
      <p><a href={job.url} target="_blank" rel="noreferrer">{job.url}</a></p>
    {/if}
    {#if job.error}<p class="field-error" role="alert">{job.error}</p>{/if}
  </div>
{/if}

{#if published.length || publishedError}
  <div class="panel">
    <h2 class="section-title">Live ({published.length})</h2>
    {#if publishedError}
      <p class="field-error">Could not list the live games: {publishedError}</p>
    {/if}
    {#if published.length}
      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              <th>Game</th><th>Link</th><th>Access</th><th class="num">Balance</th>
              <th><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {#each published as p (p.host)}
              <tr>
                <td>{p.name}</td>
                <td><a href={p.url} target="_blank" rel="noreferrer">{p.host}</a></td>
                <td>{p.accessCode || 'open'}{p.console ? ' · console' : ''}</td>
                <td class="num">{p.balance.toLocaleString('en-US')} {p.currency}</td>
                <td class="act">
                  <button
                    class="ghost"
                    aria-label="Copy the link of {p.name}"
                    onclick={() => copyLink(p.url)}
                  >
                    Copy link
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
{/if}

<div class="panel">
  <button
    class="ghost disclosure"
    aria-expanded={showSettings}
    aria-controls="deploy-settings"
    onclick={() => (showSettings = !showSettings)}
  >
    <span class="chev" aria-hidden="true">{showSettings ? '▾' : '▸'}</span>
    Deployment settings
  </button>
  {#if showSettings}
    <div id="deploy-settings">
      <p class="dim settings-note">
        The Cloudflare token can edit the whole zone's DNS: it stays
        <strong>here</strong>, in <code>deploy.json</code> (chmod 600), and is never
        sent to the server. Saved secrets show masked; leaving them as they are
        keeps them.
      </p>
      <form class="form settings" onsubmit={saveConfig}>
        <label class="wide"><span>SSH target</span>
          <input bind:value={cfg.sshTarget} placeholder="root@192.168.1.181" /></label>
        <label class="wide"><span>Remote folder (games)</span>
          <input bind:value={cfg.remoteDir} placeholder="/media/usb-disk/slotlab/games" /></label>
        <label class="wide"><span>Hub URL (LAN)</span>
          <input bind:value={cfg.hubUrl} placeholder="http://192.168.1.200:3610" /></label>
        <label><span>Hub token</span>
          <span class="secret">
            <input
              type={showAdminToken ? 'text' : 'password'}
              autocomplete="off"
              spellcheck="false"
              bind:value={cfg.adminToken}
            />
            <button
              type="button"
              class="ghost"
              aria-pressed={showAdminToken}
              aria-label={showAdminToken ? 'Hide the hub token' : 'Show the hub token'}
              onclick={() => (showAdminToken = !showAdminToken)}
            >
              {showAdminToken ? 'Hide' : 'Show'}
            </button>
          </span>
        </label>
        <label class="wide"><span>Tunnel target</span>
          <input bind:value={cfg.tunnelTarget} placeholder="http://192.168.1.200:3610" /></label>
        <label><span>Domain</span>
          <input bind:value={cfg.baseDomain} placeholder="mcberry.fr" /></label>
        <label><span>Cloudflare token</span>
          <span class="secret">
            <input
              type={showCfToken ? 'text' : 'password'}
              autocomplete="off"
              spellcheck="false"
              bind:value={cfg.cfToken}
            />
            <button
              type="button"
              class="ghost"
              aria-pressed={showCfToken}
              aria-label={showCfToken ? 'Hide the Cloudflare token' : 'Show the Cloudflare token'}
              onclick={() => (showCfToken = !showCfToken)}
            >
              {showCfToken ? 'Hide' : 'Show'}
            </button>
          </span>
        </label>
        <label class="wide"><span>Zone ID</span><input bind:value={cfg.cfZoneId} /></label>
        <label class="wide"><span>Account ID</span><input bind:value={cfg.cfAccountId} /></label>
        <label class="wide"><span>Tunnel ID</span><input bind:value={cfg.cfTunnelId} /></label>
        <p class="settings-group">
          Hub rebuild — only used when the hub must be updated to read this SlotLab's
          books. Empty = the default shown.
        </p>
        <label class="wide"><span>Hub source folder (this Mac)</span>
          <input bind:value={cfg.hubSourceDir} placeholder="the checkout SlotLab was built from" /></label>
        <label class="wide"><span>Sources on the server</span>
          <input bind:value={cfg.hubRemoteSrc} placeholder="next to the games folder: …/slotlab/src" /></label>
        <label><span>Proxmox container</span>
          <input bind:value={cfg.hubCt} placeholder="102 (- = Docker on the SSH target)" /></label>
        <label class="wide"><span>Sources seen from Docker</span>
          <input bind:value={cfg.hubBuildDir} placeholder="/mnt/storage/slotlab/src" /></label>
        <label class="wide"><span>Compose stack</span>
          <input bind:value={cfg.hubStackDir} placeholder="/opt/stacks/slotlab-share" /></label>
        <label><span>Image</span>
          <input bind:value={cfg.hubImage} placeholder="slotlab-share:latest" /></label>
        <p class="settings-group">
          Containers paused during a deployment, resumed at the end (a video render
          and the books upload don't fit in the server's memory together).
        </p>
        <label class="wide"><span>Pause during upload</span>
          <input bind:value={cfg.pauseDuringDeploy} placeholder="tiktok-bot (comma-separated)" /></label>
        <button type="submit" class="primary" disabled={saving}>
          {saving ? 'Saving…' : saved ? 'Saved ✓' : 'Save'}
        </button>
      </form>
    </div>
  {/if}
</div>

<style>
  .panel + .panel { margin-top: var(--sp-4); }
  .section-title { font-size: var(--fs-md); margin: 0 0 var(--sp-2); }
  .game-name { font-family: var(--font-mono); color: var(--gold); }
  .intro { margin: 0 0 var(--sp-4); max-width: 80ch; }
  .live-now { margin: var(--sp-3) 0 0; }
  .form { display: flex; flex-wrap: wrap; gap: var(--sp-3); align-items: flex-end; }
  .form label { display: flex; flex-direction: column; gap: var(--sp-1); font-size: var(--fs-xs); color: var(--text-dim); }
  .form label.wide { flex: 1; min-width: min(320px, 100%); }
  /* Same label-on-top layout as the fields; the box gets an input's height. */
  .form label.check .box { display: flex; align-items: center; height: 31px; }
  .form.settings { margin-top: var(--sp-3); }
  .secret { display: flex; gap: var(--sp-1); }
  .secret input { flex: 1; min-width: 0; }
  .disclosure { display: inline-flex; align-items: center; gap: var(--sp-2); padding-left: var(--sp-1); color: var(--text); }
  .disclosure .chev { width: 1em; color: var(--text-dim); }
  .settings-note { margin: var(--sp-3) 0 0; max-width: 80ch; }
  .steps { list-style: none; padding: 0; margin: var(--sp-2) 0 0; }
  .steps li { display: flex; align-items: baseline; gap: var(--sp-3); padding: var(--sp-1) 0; font-size: var(--fs-sm); }
  .steps .g { width: 14px; text-align: center; color: var(--text-dim); }
  .steps li.done .g { color: var(--pass); }
  .steps li.failed .g { color: var(--fail); }
  .steps li.running .g { color: var(--gold); }
  .steps li.pending .l { color: var(--text-dim); }
  .steps .d { color: var(--text-dim); font-size: var(--fs-xs); }
  .table-wrap { overflow-x: auto; }
  td.act { text-align: right; white-space: nowrap; }
  td.act button { padding: 2px var(--sp-2); }

  .settings-group {
    flex-basis: 100%;
    margin: var(--sp-3) 0 0;
    padding-top: var(--sp-3);
    border-top: 1px solid var(--border);
    font-size: var(--fs-sm);
    color: var(--text-dim);
  }
</style>
