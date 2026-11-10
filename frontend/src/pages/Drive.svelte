<script lang="ts">
  /**
   * What is stored on Google Drive (`slot drive --json`): finished slots whose
   * configs — and, for older archives, publish_files — wait there. Restore puts
   * them back on the VPS; the Projects tab only shows the work in progress.
   */
  import { api, fmtBytes, type DriveGame, type DriveList, type ProjectJob } from '../lib/api';
  import { errorText, notify, run } from '../lib/stores.svelte';
  import ConfirmButton from '../components/ConfirmButton.svelte';
  import Icon from '../components/Icon.svelte';

  const CLIENT_ORDER = ['DegenLab', 'Gaspi', 'Yassuo', 'Stepico', 'BurstPilot', 'Inkration'];

  let data = $state<DriveList | null>(null);
  let loadError = $state<string | null>(null);
  let loading = $state(false);
  let query = $state('');
  let jobs = $state<Record<string, ProjectJob>>({});
  let pending = $state<Record<string, boolean>>({});

  async function load(refresh = false) {
    loading = true;
    try {
      data = await api.drive(refresh);
      loadError = null;
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
        if (j.action !== 'restore') continue;
        next[j.key] = j;
        const before = jobs[j.key];
        if (before?.state === 'running' && j.state !== 'running') {
          finished = true;
          const name = j.key.split('/').pop();
          if (j.state === 'done') notify(`Restore ${name}: done`, 'ok', 6000);
          else notify(`Restore ${name} failed: ${j.message ?? 'error'}`, 'error');
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
    void pollJobs();
  });

  $effect(() => {
    if (!anyRunning) return;
    const iv = setInterval(pollJobs, 2000);
    return () => clearInterval(iv);
  });

  async function restore(g: DriveGame) {
    pending[g.key] = true;
    const r = await run(() => api.projectAction(g.key, 'restore'), {
      error: `Could not start the restore of ${g.game}`,
    });
    if (r?.job) {
      jobs = { ...jobs, [g.key]: r.job };
      notify(`Restore ${g.game} started`, 'info');
    }
    pending[g.key] = false;
  }

  const groups = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const list = (data?.games ?? []).filter(
      (g) => !q || [g.game, g.client].some((s) => s.toLowerCase().includes(q)),
    );
    const by = new Map<string, DriveGame[]>();
    for (const g of list) {
      if (!by.has(g.client)) by.set(g.client, []);
      by.get(g.client)!.push(g);
    }
    const rank = (c: string) => {
      const i = CLIENT_ORDER.indexOf(c);
      return i < 0 ? 99 : i;
    };
    return [...by.entries()]
      .sort((a, b) => rank(a[0]) - rank(b[0]) || a[0].localeCompare(b[0]))
      .map(([client, games]) => ({ client, games, bytes: games.reduce((s, g) => s + g.bytes, 0) }));
  });

  const total = $derived((data?.games ?? []).reduce((s, g) => s + g.bytes, 0));

  function when(g: DriveGame): string {
    const t = g.archived_at ? g.archived_at * 1000 : Date.parse(g.modified);
    if (!t || Number.isNaN(t)) return '—';
    return new Date(t).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
  }

  function badge(g: DriveGame): { label: string; cls: string; title: string } {
    if (g.has_publish_vps) return { label: 'Also on VPS', cls: 'green', title: 'publish_files is on the VPS too' };
    if (!g.on_vps) return { label: 'Drive only', cls: 'yellow', title: 'The game folder is not on the VPS any more' };
    return { label: 'Archived', cls: 'blue', title: 'Code and configs on the VPS; publish_files cleared' };
  }
</script>

<section class="head panel">
  <div class="line">
    <p class="overline">Google Drive · {data?.remote ?? 'gdrive:StakeEngine-archive'}</p>
    <button class="plain" onclick={() => load(true)} disabled={loading} aria-label="Refresh">
      <Icon name="refresh" size={12} />
      {loading ? 'Reading Drive…' : 'Refresh'}
    </button>
  </div>
  {#if data}
    <p class="big">{data.games.length} games <span class="faint">· {fmtBytes(total)}</span></p>
  {/if}
  <p class="legend faint">
    Finished slots end up here when you click <b>Archive</b> in Projects. <b>Restore</b> puts the configs
    back on the VPS (and publish_files when Drive has it); then re-simulate to rebuild the books.
  </p>
</section>

{#if loadError}
  <p class="field-error" role="alert">{loadError}</p>
{:else if !data}
  <p class="faint" aria-busy="true">Reading Google Drive (about 20 s)…</p>
{:else if data.games.length === 0}
  <p class="faint">Nothing on Drive yet.</p>
{:else}
  <input class="search" type="search" bind:value={query} placeholder="Search a game…" aria-label="Search a game" />
  {#each groups as grp (grp.client)}
    <section class="client">
      <p class="overline">{grp.client} <span class="faint">· {grp.games.length} · {fmtBytes(grp.bytes)}</span></p>
      <div class="grid">
        {#each grp.games as g (g.key)}
          {@const s = badge(g)}
          {@const job = jobs[g.key]}
          {@const isBusy = pending[g.key] || job?.state === 'running'}
          <article class="card">
            <header>
              <h3 title={g.key}>{g.game}</h3>
              <span class="pill tiny {s.cls}" title={s.title}>{s.label}</span>
            </header>
            <dl>
              <div><dt>On Drive</dt><dd>{fmtBytes(g.bytes)}</dd></div>
              <div><dt>Content</dt><dd>{g.publish_bytes ? `publish ${fmtBytes(g.publish_bytes)}` : 'configs only'}</dd></div>
              <div><dt>Archived</dt><dd>{when(g)}</dd></div>
            </dl>
            {#if job}
              <p class="job" class:err={job.state === 'error'} class:ok={job.state === 'done'}>
                {#if job.state === 'running'}{job.last_progress ?? 'starting…'}
                {:else if job.state === 'done'}Restore done{job.message ? ` — ${job.message}` : ''}
                {:else}Restore failed — {job.message}{/if}
              </p>
            {/if}
            <div class="actions">
              <ConfirmButton
                onconfirm={() => restore(g)}
                variant="plain"
                disabled={isBusy}
                confirmLabel={g.publish_bytes ? `Download ${fmtBytes(g.publish_bytes)}?` : 'Bring the configs back?'}
                title="Put this game back on the VPS from Google Drive"
                ariaLabel="Restore {g.game}"
              >
                Restore
              </ConfirmButton>
            </div>
          </article>
        {/each}
      </div>
    </section>
  {/each}
{/if}

<style>
  .head {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    margin-bottom: var(--sp-5);
  }
  .line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-2);
  }
  .line .overline {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .big {
    margin: 0;
    font-size: var(--fs-lg);
    font-weight: 680;
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
  .search {
    width: 100%;
    max-width: 320px;
    margin-bottom: var(--sp-5);
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
  dl {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--sp-2);
    margin: 0;
  }
  dt {
    font-size: 10px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-faint);
  }
  dd {
    margin: 2px 0 0;
    font-size: var(--fs-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .job {
    margin: 0;
    font-size: var(--fs-xs);
    color: var(--text-dim);
    overflow-wrap: anywhere;
  }
  .job.ok {
    color: var(--pass);
  }
  .job.err {
    color: var(--fail);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: auto;
  }
</style>
