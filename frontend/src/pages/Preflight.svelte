<script lang="ts">
  // The pre-upload checklist, run instead of typed.
  //
  // The review that decides publication is dominated by the VISUAL, and every
  // refusal the studio has received is a one-line deviation somewhere in the
  // frontend or a mismatch between the LUT and the books. All of it is
  // greppable — so it is grepped here, with the file and the line, because a
  // finding nobody can locate is a finding nobody acts on.
  import { api, type PreflightReport, type PreflightFinding } from '../lib/api';
  import { app } from '../lib/stores.svelte';
  import { isPreflightKey, mutedGroups, splitBy } from '../lib/muted';
  import { mutes } from '../lib/muted.svelte';
  import MuteButton from '../components/MuteButton.svelte';
  import MutedList from '../components/MutedList.svelte';

  /** Filter keys: the statuses, plus `review` = blocking + warnings. */
  type Filter = 'all' | 'fail' | 'review' | 'skip' | 'pass';

  let report = $state<PreflightReport | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let filter = $state<Filter>('all');
  /** Per-finding disclosure; a finding not in the map uses its default
   * (failed ones start open, the rest closed) — and can be collapsed. */
  let open = $state<Record<string, boolean>>({});
  let seq = 0;

  $effect(() => {
    const name = app.selectedGame;
    if (name) void load(name, true);
    else {
      report = null;
      error = null;
    }
  });

  $effect(() => {
    if (app.selectedGame) void mutes.load(app.selectedGame);
  });

  /** Mute key of a checklist item; a blocking one (`fail`) or a clear one is never muted. */
  const keyOf = (f: PreflightFinding) => ({ rule: `preflight:${f.id}`, mode: null, blocking: f.severity === 'fail' || f.status === 'fail' || f.status === 'pass' });
  const split = $derived(splitBy(report?.findings ?? [], mutes.game === app.selectedGame ? mutes.list : [], keyOf));
  const muted = $derived(
    mutedGroups(mutes.list, split.muted, (f) => ({ rule: `preflight:${f.id}`, modes: [null], title: f.label }), true, (m) => isPreflightKey(m.rule)),
  );

  async function load(gameName: string, fresh = false) {
    const token = ++seq;
    loading = true;
    error = null;
    if (fresh) report = null;
    try {
      const r = await api.preflight(gameName);
      if (token !== seq) return;
      report = r;
      if (fresh) {
        open = {};
        // Land on what needs reading: the blocking and to-review findings,
        // not 30 green rows with the two warnings buried under them.
        filter = r.findings.some((f) => f.status === 'fail' || f.status === 'warn')
          ? 'review'
          : 'all';
      }
    } catch (e) {
      if (token !== seq) return;
      error = e instanceof Error ? e.message : String(e);
      report = null;
    } finally {
      if (token === seq) loading = false;
    }
  }

  const SCOPES: { id: string; label: string; hint: string }[] = [
    { id: 'appSrc', label: 'Game sources', hint: 'apps/<game>/src' },
    { id: 'appRoot', label: 'App folder', hint: 'package.json, static/, app.html' },
    { id: 'packages', label: 'Shared kit', hint: 'frontend/packages' },
    { id: 'build', label: 'Build', hint: 'what gets uploaded' },
    { id: 'publish', label: 'publish_files', hint: 'LUT, books, index.json' },
  ];

  /** One vocabulary for the summary pills, the filter tabs and the rows.
   * "To review" is everything that needs reading: blocking + warnings. */
  const WORDS: Record<PreflightFinding['status'], string> = {
    fail: 'Blocking',
    warn: 'To review',
    skip: 'Not checkable',
    pass: 'Clear',
  };

  // Muted items (to review / not checkable only) leave every count.
  const nOf = (st: PreflightFinding['status']) => split.shown.filter((f) => f.status === st).length;
  const counts = $derived({
    all: split.shown.length,
    fail: report?.failCount ?? 0,
    warn: nOf('warn'),
    review: (report?.failCount ?? 0) + nOf('warn'),
    skip: nOf('skip'),
    pass: nOf('pass'),
  });

  const FILTERS: { id: Filter; label: string }[] = [
    { id: 'all', label: 'All' },
    { id: 'fail', label: WORDS.fail },
    { id: 'review', label: WORDS.warn },
    { id: 'skip', label: WORDS.skip },
    { id: 'pass', label: WORDS.pass },
  ];

  const shown = $derived(
    split.shown.filter(
      (f) =>
        filter === 'all' ||
        f.status === filter ||
        (filter === 'review' && (f.status === 'fail' || f.status === 'warn')),
    ),
  );

  function inScope(scope: string): PreflightFinding[] {
    return shown.filter((f) => f.scope === scope);
  }

  function isOpen(f: PreflightFinding): boolean {
    return open[f.id] ?? f.status === 'fail';
  }

  function pill(status: string): string {
    return status === 'pass'
      ? 'green'
      : status === 'warn'
        ? 'yellow'
        : status === 'fail'
          ? 'red'
          : 'blue';
  }
</script>

{#if error}
  <div class="banner error" role="alert">Preflight failed: {error}</div>
{/if}

{#if !app.selectedGame}
  <div class="panel">
    <p class="dim">Select a game to run the pre-upload checklist.</p>
  </div>
{:else if !report}
  <div class="panel">
    <p class="dim" role="status">{loading ? 'Running the checklist…' : 'No report for this game.'}</p>
  </div>
{:else}
  <header class="verdict-bar">
    <div class="verdict-main">
      <span class="pill {report.passed ? 'green' : 'red'} big">
        {report.passed ? 'Ready to upload' : `${counts.fail} ${WORDS.fail.toLowerCase()}`}
      </span>
      {#if counts.warn > 0}<span class="pill yellow">{counts.review} {WORDS.warn.toLowerCase()}</span>{/if}
      {#if counts.skip > 0}<span class="pill blue">{counts.skip} {WORDS.skip.toLowerCase()}</span>{/if}
      <span class="pill green">{counts.pass} {WORDS.pass.toLowerCase()}</span>
    </div>
    <div class="verdict-actions">
      <button onclick={() => app.selectedGame && load(app.selectedGame)} disabled={loading}>
        {loading ? 'Running…' : 'Re-run'}
      </button>
    </div>
  </header>

  <div class="view-tabs" role="tablist" aria-label="Filter findings">
    {#each FILTERS as f (f.id)}
      <button
        role="tab"
        aria-selected={filter === f.id}
        class:active={filter === f.id}
        onclick={() => (filter = f.id)}
      >
        {f.label}<span class="count">{counts[f.id]}</span>
      </button>
    {/each}
    <span class="view-context faint">
      {report.appDir ? report.appDir.split('/').slice(-3).join('/') : 'no frontend registered'}
    </span>
  </div>

  {#if shown.length === 0}
    <p class="dim">No finding in “{FILTERS.find((f) => f.id === filter)?.label}”.</p>
  {/if}

  {#each SCOPES as s (s.id)}
    {@const list = inScope(s.id)}
    {#if list.length > 0}
      <section class="page scope">
        <p class="overline">{s.label} <span class="faint">{s.hint}</span></p>
        <ul class="findings">
          {#each list as f (f.id)}
            {@const expanded = isOpen(f)}
            <li class="finding {f.status}">
              <button
                class="fhead"
                aria-expanded={expanded}
                aria-controls="finding-{f.id}"
                onclick={() => (open[f.id] = !expanded)}
              >
                <span class="pill {pill(f.status)}">{WORDS[f.status]}</span>
                <span class="flabel">{f.label}</span>
                <span class="fdetail faint">{f.detail}</span>
                {#if f.doc}<span class="fdoc faint">{f.doc}</span>{/if}
                <span class="chev faint" aria-hidden="true">{expanded ? '−' : '+'}</span>
              </button>
              {#if expanded}
                <div class="fbody" id="finding-{f.id}">
                  {#if f.explain}<p class="explain">{f.explain}</p>{/if}
                  {#if f.hits.length > 0}
                    <ul class="hits">
                      {#each f.hits as h (h.file + ':' + h.line + h.text)}
                        <li>
                          <code class="where">{h.file}{h.line ? ':' + h.line : ''}</code>
                          {#if h.text}<code class="what">{h.text}</code>{/if}
                        </li>
                      {/each}
                      {#if f.extraHits > 0}
                        <li class="faint">+{f.extraHits} more</li>
                      {/if}
                    </ul>
                  {/if}
                  {#if !f.explain && f.hits.length === 0}
                    <p class="explain">{f.detail}</p>
                  {/if}
                  {#if !keyOf(f).blocking}
                    <div class="fmute"><MuteButton rule={`preflight:${f.id}`} mode={null} /></div>
                  {/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      </section>
    {/if}
  {/each}

  <MutedList groups={muted} count={split.muted.length} />

  <p class="disclaimer faint">
    A hit is not a verdict: the rules are literal, and a game that solves a
    problem differently shows up as a false positive — the file and line are
    there so the reading takes seconds. What this page cannot see stays on the
    human checklist: reading every image by eye, the tile thumbnail, the
    Popout S pass, and the sizes shown by the dashboard against the local ones.
  </p>
{/if}

<style>
  .fmute {
    display: flex;
    justify-content: flex-end;
    margin-top: 6px;
  }

  .verdict-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--sp-2) var(--sp-4);
    padding: var(--sp-1) 0 var(--sp-4);
  }

  .verdict-main {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--sp-3);
  }

  .scope + .scope {
    margin-top: var(--sp-5);
  }

  .findings {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }

  .finding {
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    background: var(--bg-panel);
    overflow: hidden;
  }

  .finding.fail {
    border-color: color-mix(in oklch, var(--fail) 40%, transparent);
  }

  .finding.warn {
    border-color: color-mix(in oklch, var(--warn) var(--tint-strong), transparent);
  }

  .fhead {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    border-radius: 0;
    padding: var(--sp-2) var(--sp-3);
    font-size: var(--fs-sm);
    font-weight: 400;
  }

  .fhead:hover:not(:disabled) {
    background: var(--bg-raised);
  }

  .fhead:active:not(:disabled) {
    transform: none;
  }

  .flabel {
    font-weight: 600;
    white-space: nowrap;
  }

  .fdetail {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-xs);
  }

  .fdoc {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
  }

  .chev {
    width: 12px;
    text-align: center;
  }

  .fbody {
    padding: 2px var(--sp-3) var(--sp-3);
    border-top: 1px solid var(--border);
  }

  .explain {
    margin: var(--sp-2) 0;
    font-size: var(--fs-sm);
    color: var(--text-dim);
    max-width: 92ch;
  }

  .hits {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .hits li {
    display: flex;
    gap: var(--sp-3);
    align-items: baseline;
    font-size: var(--fs-xs);
  }

  .where {
    font-family: var(--font-mono);
    color: var(--gold);
    white-space: nowrap;
  }

  .what {
    font-family: var(--font-mono);
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .disclaimer {
    margin-top: var(--sp-6);
    font-size: var(--fs-sm);
    max-width: 92ch;
  }
</style>
