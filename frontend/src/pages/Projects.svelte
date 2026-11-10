<script lang="ts">
  /**
   * Every slot project on the VPS, from the `slot` CLI: where its math lives
   * (VPS / Google Drive / nowhere yet), what it weighs, and the actions that
   * free the disk. The CLI does the work; this page only drives it.
   */
  import {
    api,
    fmtBytes,
    type Project,
    type ProjectAction,
    type ProjectJob,
    type ProjectList,
    type ProjectStatus,
  } from '../lib/api';
  import { app, errorText, goTo, notify, run } from '../lib/stores.svelte';
  import ConfirmButton from '../components/ConfirmButton.svelte';
  import Icon from '../components/Icon.svelte';

  type Filter = 'all' | ProjectStatus;

  let data = $state<ProjectList | null>(null);
  let loadError = $state<string | null>(null);
  let loading = $state(false);
  let jobs = $state<Record<string, ProjectJob>>({});
  let filter = $state<Filter>('all');
  let query = $state('');
  /** Keys whose action was just sent (buttons disabled before the first poll). */
  let pending = $state<Record<string, boolean>>({});

  const STATUS_LABEL: Record<ProjectStatus, string> = {
    en_cours: 'In progress',
    pause: 'Paused',
    livre: 'Delivered',
  };
  const STATUS_PILL: Record<ProjectStatus, string> = {
    en_cours: 'green',
    pause: 'yellow',
    livre: 'blue',
  };
  const ACTION_LABEL: Record<ProjectAction, string> = {
    archive: 'Archive',
    restore: 'Restore',
    clean: 'Clean',
    status: 'Status',
  };
  const CLIENT_ORDER = ['DegenLab', 'Gaspi', 'Yassuo', 'Stepico', 'BurstPilot', 'Inkration'];

  async function load(refresh = false) {
    loading = true;
    try {
      const d = await api.projects(refresh);
      data = d;
      loadError = null;
      const next = { ...jobs };
      for (const p of d.projects) if (p.job) next[p.key] = p.job;
      jobs = next;
    } catch (e) {
      loadError = errorText(e);
    } finally {
      loading = false;
    }
  }

  const anyRunning = $derived(Object.values(jobs).some((j) => j.state === 'running'));

  async function pollJobs() {
    try {
      const { jobs: list } = await api.projectJobs();
      let finished = false;
      const next: Record<string, ProjectJob> = {};
      for (const j of list) {
        next[j.key] = j;
        const before = jobs[j.key];
        if (before?.state === 'running' && j.state !== 'running') {
          finished = true;
          const name = j.key.split('/').pop();
          if (j.state === 'done') notify(`${ACTION_LABEL[j.action]} ${name}: done`, 'ok', 6000);
          else notify(`${ACTION_LABEL[j.action]} ${name} failed: ${j.message ?? 'error'}`, 'error');
        }
      }
      jobs = next;
      if (finished) await load(true);
    } catch {
      /* next tick */
    }
  }

  $effect(() => {
    void load();
  });

  // Live progress: every 2 s while a job runs, nothing otherwise.
  $effect(() => {
    if (!anyRunning) return;
    const iv = setInterval(pollJobs, 2000);
    return () => clearInterval(iv);
  });

  async function act(p: Project, action: Exclude<ProjectAction, 'status'>) {
    pending[p.key] = true;
    const r = await run(() => api.projectAction(p.key, action), {
      error: `Could not start ${ACTION_LABEL[action].toLowerCase()} on ${p.game}`,
    });
    if (r?.job) {
      jobs = { ...jobs, [p.key]: r.job };
      notify(`${ACTION_LABEL[action]} ${p.game} started`, 'info');
    }
    pending[p.key] = false;
  }

  async function setStatus(p: Project, status: ProjectStatus) {
    if (status === p.status) return;
    pending[p.key] = true;
    const r = await run(() => api.projectAction(p.key, 'status', status), {
      ok: `${p.game}: ${STATUS_LABEL[status]}`,
      error: `Could not change the status of ${p.game}`,
    });
    if (r) await load(true);
    pending[p.key] = false;
  }

  function open(p: Project, tab: 'play' | 'publish') {
    if (!p.slotlab_game) return;
    app.selectedGame = p.slotlab_game;
    if (tab === 'play') goTo('play');
    else goTo('publish', 'link');
  }

  function ago(unixSecs: number): string {
    if (!unixSecs) return '—';
    const s = Math.max(0, Date.now() / 1000 - unixSecs);
    if (s < 60) return 'just now';
    if (s < 3600) return `${Math.floor(s / 60)} min ago`;
    if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
    if (s < 86400 * 60) return `${Math.floor(s / 86400)} d ago`;
    return `${Math.floor(s / (86400 * 30))} mo ago`;
  }

  function where(p: Project): { label: string; cls: string; title: string } {
    if (p.has_publish && p.archived)
      return { label: 'VPS + Drive', cls: 'green', title: `On the VPS, copy on ${p.archived.remote}` };
    if (p.has_publish) return { label: 'VPS', cls: 'green', title: p.publish_files };
    if (p.archived) return { label: 'Drive', cls: 'blue', title: p.archived.remote };
    return { label: 'No math yet', cls: '', title: 'No publish_files on the VPS, nothing archived' };
  }

  function busy(p: Project): boolean {
    return p.running || pending[p.key] || jobs[p.key]?.state === 'running';
  }

  const counts = $derived.by(() => {
    const c: Record<Filter, number> = { all: 0, en_cours: 0, pause: 0, livre: 0 };
    for (const p of data?.projects ?? []) {
      c.all++;
      c[p.status]++;
    }
    return c;
  });

  const groups = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const list = (data?.projects ?? []).filter(
      (p) =>
        (filter === 'all' || p.status === filter) &&
        (!q ||
          [p.game, p.client, p.math_id, p.slotlab_game ?? ''].some((s) =>
            s.toLowerCase().includes(q),
          )),
    );
    const by = new Map<string, Project[]>();
    for (const p of list) {
      if (!by.has(p.client)) by.set(p.client, []);
      by.get(p.client)!.push(p);
    }
    const rank = (c: string) => {
      const i = CLIENT_ORDER.indexOf(c);
      return i < 0 ? CLIENT_ORDER.length : i;
    };
    return [...by.entries()]
      .sort(([a], [b]) => rank(a) - rank(b) || a.localeCompare(b))
      .map(([client, projects]) => ({
        client,
        projects: projects.sort((a, b) => b.last_modified - a.last_modified),
        bytes: projects.reduce((s, p) => s + p.bytes_publish + p.bytes_intermediate, 0),
      }));
  });

  const disk = $derived(data?.disk);
  const usedPct = $derived(disk && disk.total > 0 ? disk.used / disk.total : 0);
  const reclaimable = $derived(
    (data?.projects ?? []).reduce((s, p) => s + p.bytes_intermediate, 0),
  );

  const FILTERS: { id: Filter; label: string }[] = [
    { id: 'all', label: 'All' },
    { id: 'en_cours', label: 'In progress' },
    { id: 'pause', label: 'Paused' },
    { id: 'livre', label: 'Delivered' },
  ];
</script>

<section class="disk panel" aria-label="VPS disk">
  {#if disk}
    <div class="disk-head">
      <span class="disk-title">VPS disk</span>
      <span class="disk-figs" class:hot={usedPct > 0.85}>
        <strong>{fmtBytes(disk.used)}</strong> used · {fmtBytes(disk.free)} free
        <span class="faint">of {fmtBytes(disk.total)}</span>
      </span>
      <button
        class="ghost icon-btn"
        onclick={() => load(true)}
        disabled={loading}
        aria-label="Refresh the list"
        title="Refresh the list"
      >
        <Icon name="refresh" size={14} />
      </button>
    </div>
    <div
      class="bar"
      role="meter"
      aria-label="Disk used"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(usedPct * 100)}
    >
      <span class="fill" class:hot={usedPct > 0.85} style="width: {Math.min(100, usedPct * 100)}%"></span>
    </div>
    <p class="legend faint">
      <b>Clean</b> deletes regenerable intermediates ({fmtBytes(reclaimable)} in total), keeps
      publish_files. <b>Archive</b> moves the whole library to {data?.remote ?? 'Google Drive'};
      <b>Restore</b> brings publish_files back.
    </p>
  {:else if loadError}
    <p class="field-error" role="alert">{loadError}</p>
  {:else}
    <p class="faint" aria-busy="true">Reading the projects…</p>
  {/if}
</section>

{#if data}
  <div class="toolbar">
    <div class="filters" role="group" aria-label="Filter by status">
      {#each FILTERS as f (f.id)}
        <button class="fchip" class:active={filter === f.id} aria-pressed={filter === f.id} onclick={() => (filter = f.id)}>
          {f.label} <span class="count">{counts[f.id]}</span>
        </button>
      {/each}
    </div>
    <input class="search" type="search" bind:value={query} placeholder="Search a game…" aria-label="Search a game" />
  </div>

  {#if loadError}
    <p class="field-error" role="alert">{loadError}</p>
  {/if}

  {#each groups as g (g.client)}
    <section class="client">
      <p class="overline">
        {g.client} <span class="faint">· {g.projects.length} · {fmtBytes(g.bytes)}</span>
      </p>
      <div class="grid">
        {#each g.projects as p (p.key)}
          {@const w = where(p)}
          {@const job = jobs[p.key]}
          {@const isBusy = busy(p)}
          <article class="card">
            <header>
              <h3 title={p.key}>{p.game}</h3>
              <label class="status-sel pill {STATUS_PILL[p.status]}">
                <span class="sr-only">Status of {p.game}</span>
                <select
                  value={p.status}
                  disabled={isBusy}
                  onchange={(e) => setStatus(p, (e.currentTarget as HTMLSelectElement).value as ProjectStatus)}
                >
                  {#each Object.entries(STATUS_LABEL) as [v, l] (v)}
                    <option value={v}>{l}</option>
                  {/each}
                </select>
              </label>
            </header>
            <div class="tags">
              <span class="pill tiny {w.cls}" class:none={!w.cls} title={w.title}>{w.label}</span>
              {#if p.slotlab_game}
                <span class="pill tiny gold" title="Registered in SlotLab">SlotLab: {p.slotlab_game}</span>
              {/if}
              {#if p.running}
                <span class="pill tiny yellow" title="A simulation or transfer is running">running</span>
              {/if}
              {#if p.math_id !== p.game}
                <span class="faint mono">{p.math_id}</span>
              {/if}
            </div>
            <dl class="sizes">
              <div>
                <dt>Publish</dt>
                <dd>{p.has_publish ? fmtBytes(p.bytes_publish) : p.archived ? fmtBytes(p.archived.bytes) : '—'}</dd>
              </div>
              <div>
                <dt>Intermediates</dt>
                <dd class:warn={p.bytes_intermediate > 1e9}>
                  {p.bytes_intermediate > 0 ? fmtBytes(p.bytes_intermediate) : '—'}
                </dd>
              </div>
              <div>
                <dt>Modified</dt>
                <dd title={p.last_modified ? new Date(p.last_modified * 1000).toLocaleString() : ''}>
                  {ago(p.last_modified)}
                </dd>
              </div>
            </dl>
            {#if job && job.action !== 'status' && (job.state === 'running' || Date.now() - (job.finished_at ?? 0) < 120_000)}
              <p class="job {job.state}" role="status">
                {#if job.state === 'running'}
                  <span class="spin" aria-hidden="true"></span>
                  {ACTION_LABEL[job.action]}… <span class="mono">{job.last_progress ?? 'starting'}</span>
                {:else if job.state === 'done'}
                  {ACTION_LABEL[job.action]} done{job.message ? ` — ${job.message}` : ''}
                {:else}
                  {ACTION_LABEL[job.action]} failed — {job.message}
                {/if}
              </p>
            {/if}
            <div class="actions">
              {#if p.slotlab_game}
                <button class="primary" onclick={() => open(p, 'play')}>
                  <Icon name="play" size={12} /> Play
                </button>
                <button onclick={() => open(p, 'publish')}>
                  <Icon name="link" size={12} /> Share
                </button>
              {/if}
              {#if p.bytes_intermediate > 0}
                <ConfirmButton
                  onconfirm={() => act(p, 'clean')}
                  variant="plain"
                  disabled={isBusy}
                  confirmLabel="Delete {fmtBytes(p.bytes_intermediate)}?"
                  title="Delete books, forces, lookup tables and temp files (regenerable)"
                  ariaLabel="Clean {p.game}"
                >
                  Clean
                </ConfirmButton>
              {/if}
              {#if p.has_publish}
                <ConfirmButton
                  onconfirm={() => act(p, 'archive')}
                  variant="plain"
                  disabled={isBusy}
                  confirmLabel="Move {fmtBytes(p.bytes_publish)} to Drive?"
                  title="Clean, upload the library to Google Drive, verify, then free the VPS"
                  ariaLabel="Archive {p.game} to Drive"
                >
                  Archive to Drive
                </ConfirmButton>
              {/if}
              {#if p.archived && !p.has_publish}
                <ConfirmButton
                  onconfirm={() => act(p, 'restore')}
                  variant="plain"
                  disabled={isBusy}
                  confirmLabel="Download {fmtBytes(p.archived.bytes)}?"
                  title="Download publish_files back from Google Drive"
                  ariaLabel="Restore {p.game}"
                >
                  Restore
                </ConfirmButton>
              {/if}
            </div>
          </article>
        {/each}
      </div>
    </section>
  {:else}
    <p class="dim">No project matches.</p>
  {/each}
{/if}

<style>
  .disk {
    margin-bottom: var(--sp-5);
  }

  .disk-head {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--sp-1) var(--sp-3);
  }

  .disk-title {
    font-weight: 660;
    font-size: var(--fs-md);
  }

  .disk-figs {
    font-size: var(--fs-sm);
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
  }

  .disk-figs strong {
    color: var(--text);
  }

  .disk-figs.hot strong {
    color: var(--fail);
  }

  .icon-btn {
    margin-left: auto;
    padding: 4px 8px;
    display: inline-flex;
  }

  .bar {
    height: 8px;
    margin: 10px 0 8px;
    border-radius: var(--r-pill);
    background: var(--bg-deep);
    border: 1px solid var(--border);
    overflow: hidden;
  }

  .fill {
    display: block;
    height: 100%;
    background: var(--pass);
    border-radius: inherit;
  }

  .fill.hot {
    background: var(--fail);
  }

  .legend {
    margin: 0;
    font-size: var(--fs-xs);
    line-height: 1.5;
  }

  .legend b {
    color: var(--text-dim);
    font-weight: 640;
  }

  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2) var(--sp-3);
    margin-bottom: var(--sp-5);
  }

  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .fchip {
    border-radius: var(--r-pill);
    font-size: var(--fs-xs);
    padding: 4px 11px;
    background: transparent;
    color: var(--text-dim);
  }

  .fchip.active {
    color: var(--gold);
    border-color: color-mix(in oklch, var(--gold) 50%, transparent);
    background: color-mix(in oklch, var(--gold) var(--tint), transparent);
  }

  .fchip .count {
    font-family: var(--font-mono);
    font-size: 10px;
    opacity: 0.8;
    margin-left: 2px;
  }

  .search {
    flex: 1 1 180px;
    min-width: 0;
    max-width: 320px;
    margin-left: auto;
    font-size: var(--fs-sm);
  }

  .client {
    margin-bottom: var(--sp-6);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(290px, 100%), 1fr));
    gap: var(--sp-3);
  }

  .card {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding: 12px 14px;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--r-m);
  }

  .card header {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }

  .card h3 {
    margin: 0;
    flex: 1;
    min-width: 0;
    font-size: var(--fs-md);
    font-weight: 680;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* The status pill IS the select. */
  .status-sel {
    position: relative;
    flex-shrink: 0;
    padding-right: 4px;
  }

  .status-sel select {
    appearance: none;
    -webkit-appearance: none;
    background: transparent;
    border: none;
    color: inherit;
    font: inherit;
    padding: 0 14px 0 0;
    cursor: pointer;
    background-image: linear-gradient(45deg, transparent 50%, currentColor 50%),
      linear-gradient(135deg, currentColor 50%, transparent 50%);
    background-position: right 5px center, right 1px center;
    background-size: 4px 4px;
    background-repeat: no-repeat;
  }

  .status-sel select option {
    color: var(--text);
    background: var(--bg-panel);
  }

  .tags {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .pill.none {
    color: var(--text-faint);
    border-color: var(--border);
  }

  .mono {
    font-family: var(--font-mono);
    font-size: var(--fs-2xs);
  }

  .sizes {
    display: grid;
    grid-template-columns: repeat(3, max-content);
    gap: var(--sp-2) 22px;
    margin: 2px 0 0;
  }

  .sizes dt {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-faint);
  }

  .sizes dd {
    margin: 2px 0 0;
    font-size: var(--fs-sm);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sizes dd.warn {
    color: var(--warn);
  }

  .job {
    margin: 0;
    font-size: var(--fs-xs);
    color: var(--info);
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    overflow-wrap: anywhere;
  }

  .job.done {
    color: var(--pass);
  }

  .job.error {
    color: var(--fail);
  }

  .spin {
    width: 10px;
    height: 10px;
    border: 2px solid currentColor;
    border-right-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: auto;
    padding-top: 4px;
  }

  .actions :global(button) {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: var(--fs-xs);
    padding: 4px 10px;
  }

  @media (max-width: 520px) {
    .search {
      max-width: none;
      flex-basis: 100%;
    }

    .actions :global(button) {
      padding: 6px 11px;
    }
  }
</style>
