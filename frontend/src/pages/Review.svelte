<script lang="ts">
  // Two things get copied out of SlotLab before a delivery: the event IDs the
  // Stake submission form asks for, and an account of the game for whoever is
  // paying for it — the second in whole sentences, because a client does not
  // read in fences and av_win, and an intermediate RTP over 100 % is a tuning
  // state rather than a result.
  import { api, fmtOneIn, fmtX } from '../lib/api';
  import { app, errorText, goTo, notify, run } from '../lib/stores.svelte';

  type View = 'ids' | 'client';

  /** `slotlab-core/src/review.rs` — `oneIn` is null (JSON has no ∞) when the
   * picked book carries a zero LUT weight, or the mode has no winning book. */
  interface EventPick {
    simId: number;
    payout: number;
    oneIn: number | null;
  }
  interface ReviewPick {
    mode: string;
    average: EventPick;
    high: EventPick;
    maxWin: EventPick;
  }

  const ROWS: { key: 'average' | 'high' | 'maxWin'; label: string; hint: string }[] = [
    { key: 'average', label: 'Average win', hint: 'winning book closest to the mean win' },
    { key: 'high', label: 'High win', hint: '99.9th percentile of the wins' },
    { key: 'maxWin', label: 'Max win', hint: 'highest payout of the mode' },
  ];

  let view = $state<View>('ids');
  let text = $state('');
  let picks = $state<ReviewPick[]>([]);
  let markdown = $state('');
  let error = $state<string | null>(null);
  let reportError = $state<string | null>(null);
  let loading = $state(false);
  let forcing = $state<string | null>(null);
  let seq = 0;

  $effect(() => {
    const name = app.selectedGame;
    if (name) void load(name);
    else {
      text = '';
      picks = [];
      markdown = '';
      error = null;
      reportError = null;
    }
  });

  async function load(gameName: string) {
    const token = ++seq;
    loading = true;
    error = null;
    reportError = null;
    text = '';
    picks = [];
    markdown = '';
    const [ids, report] = await Promise.allSettled([
      api.reviewIds(gameName),
      api.clientReport(gameName),
    ]);
    if (token !== seq) return; // the game changed meanwhile
    if (ids.status === 'fulfilled') {
      text = ids.value.text;
      picks = ids.value.picks as ReviewPick[];
    } else {
      error = errorText(ids.reason);
    }
    if (report.status === 'fulfilled') markdown = report.value.markdown;
    else reportError = errorText(report.reason);
    loading = false;
  }

  const current = $derived(view === 'ids' ? text : markdown);
  const currentError = $derived(view === 'ids' ? error : reportError);

  function isEmpty(p: EventPick): boolean {
    return p.payout === 0 && p.oneIn == null;
  }

  function odds(p: EventPick): string {
    return p.oneIn == null ? 'never drawn (LUT weight 0)' : fmtOneIn(p.oneIn);
  }

  async function play(mode: string, simId: number) {
    const game = app.selectedGame;
    if (!game) return;
    forcing = `${mode}:${simId}`;
    const r = await run(() => api.setForce({ game, mode, simId }), {
      error: 'Could not queue the event',
    });
    forcing = null;
    if (r !== undefined) goTo('play');
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(current);
      notify('Copied');
    } catch (e) {
      notify(`Could not copy: ${errorText(e)}`, 'error');
    }
  }
</script>

<div class="view-tabs" role="tablist" aria-label="Review output">
  <button
    role="tab"
    aria-selected={view === 'ids'}
    class:active={view === 'ids'}
    onclick={() => (view = 'ids')}
  >
    Event IDs
  </button>
  <button
    role="tab"
    aria-selected={view === 'client'}
    class:active={view === 'client'}
    onclick={() => (view = 'client')}
  >
    Client report
  </button>
  <span class="view-context faint">{app.selectedGame ?? ''}</span>
</div>

{#if !app.selectedGame}
  <div class="panel">
    <p class="dim">Select a game to generate its review material.</p>
  </div>
{:else}
  <div class="panel" role="tabpanel">
    <div class="head">
      <h2>
        {view === 'ids' ? 'Event IDs for the Stake review' : 'What the game does'}
      </h2>
      <button class="primary" onclick={copy} disabled={!current}>Copy</button>
    </div>
    {#if view === 'ids'}
      <p class="dim">
        Automatic selection per mode — average win, high win (p99.9), max win —
        from the published LUTs. Play any of them to check it on screen.
      </p>
      <p class="banner warn">
        <span>
          <strong>Note:</strong> a future <code>run.py prod</code> re-sorts the sims
          and changes the IDs — regenerate this block from the books actually uploaded.
        </span>
      </p>
    {:else}
      <p class="dim">
        Markdown, ready to paste into a message: how often the game pays, what it
        pays, how long before something big, and where the ceiling is — per bet
        mode, read from the published files.
      </p>
    {/if}

    {#if loading && !current}
      <p class="dim" role="status">Loading…</p>
    {:else if currentError}
      <p class="banner error" role="alert">
        {view === 'ids' ? 'Could not pick the event IDs' : 'Could not build the client report'}:
        {currentError}
      </p>
    {:else}
      {#if view === 'ids' && picks.length > 0}
        <div class="table-wrap">
          <table class="picks">
            <thead>
              <tr>
                <th scope="col">Mode</th>
                <th scope="col">Pick</th>
                <th scope="col" class="num">Event</th>
                <th scope="col" class="num">Payout</th>
                <th scope="col" class="num">Odds</th>
                <th scope="col"><span class="sr-only">Actions</span></th>
              </tr>
            </thead>
            {#each picks as p (p.mode)}
              <tbody>
                {#each ROWS as r, i (r.key)}
                  {@const e = p[r.key]}
                  <tr>
                    {#if i === 0}
                      <th scope="rowgroup" rowspan={ROWS.length} class="mode">{p.mode}</th>
                    {/if}
                    <td title={r.hint}>{r.label}</td>
                    {#if isEmpty(e)}
                      <td colspan="4" class="faint">No winning book in this mode</td>
                    {:else}
                      <td class="num">{e.simId}</td>
                      <td class="num">{fmtX(e.payout)}</td>
                      <td class="num faint">{odds(e)}</td>
                      <td class="act">
                        <button
                          class="ghost"
                          disabled={forcing !== null}
                          aria-label="Play {p.mode} {r.label.toLowerCase()}, event {e.simId}"
                          onclick={() => play(p.mode, e.simId)}
                        >
                          {forcing === `${p.mode}:${e.simId}` ? 'Opening…' : '▶ Play'}
                        </button>
                      </td>
                    {/if}
                  </tr>
                {/each}
              </tbody>
            {/each}
          </table>
        </div>
        <p class="overline block-title">Text to paste</p>
      {/if}
      {#if current}
        <pre>{current}</pre>
      {:else}
        <p class="dim">Nothing to show for this game.</p>
      {/if}
    {/if}
  </div>
{/if}

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--sp-3);
  }

  .table-wrap {
    overflow-x: auto;
    margin-bottom: var(--sp-5);
  }

  .picks th.mode {
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
    color: var(--gold);
    text-transform: none;
    letter-spacing: 0;
    vertical-align: top;
  }

  .picks tbody + tbody tr:first-child > * {
    border-top: 1px solid var(--border);
  }

  .picks td.act {
    text-align: right;
    white-space: nowrap;
  }

  .picks td.act button {
    padding: 2px var(--sp-2);
  }

  .block-title {
    margin-top: var(--sp-2);
  }

  pre {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    padding: var(--sp-4);
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
    overflow-x: auto;
    white-space: pre-wrap;
    margin: 0;
  }
</style>
