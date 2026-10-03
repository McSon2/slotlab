<script lang="ts">
  // Asset budget of the frontend build: what a player downloads to start, and
  // what the textures cost in video memory once uploaded.
  //
  // ⚠️ Two numbers, not one, and the range is the point. Several assets ship in
  // mutually exclusive variants — a Spine skeleton at three atlas scales, a
  // sound in three containers — of which exactly one is loaded, chosen from the
  // device. A single total would describe nobody's session, so the report gives
  // the lightest and the heaviest configuration a device can land on.
  import { api, fmtBytes, fmtInt, type AssetReport } from '../lib/api';
  import { app, errorText } from '../lib/stores.svelte';

  let report = $state<AssetReport | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  /** Standalone files: the 20 heaviest, or all of them. */
  let showAllFiles = $state(false);

  const FILES_SHOWN = 20;

  $effect(() => {
    if (app.selectedGame) void load(app.selectedGame);
  });

  async function load(gameName: string) {
    loading = true;
    error = null;
    try {
      report = await api.gameAssets(gameName);
    } catch (e) {
      error = errorText(e);
      report = null;
    } finally {
      loading = false;
    }
  }

  /** "2.3 MB" → ["2.3", "MB"]. */
  const split = (b: number): [string, string] => {
    const t = fmtBytes(b);
    const i = t.lastIndexOf(' ');
    return i < 0 ? [t, ''] : [t.slice(0, i), t.slice(i + 1)];
  };

  /** Hero value + unit: one number, or a range when the two ends differ enough to matter. */
  function spread(lo: number, hi: number): { value: string; unit: string } {
    const [a, ua] = split(lo);
    if (hi - lo < 50_000) return { value: a, unit: ua };
    const [b, ub] = split(hi);
    return ua === ub
      ? { value: `${a} – ${b}`, unit: ua }
      : { value: `${a} ${ua} – ${b}`, unit: ub };
  }

  /** Long absolute path, cut in the middle (the full one goes in `title`). */
  function middle(path: string, max = 72): string {
    if (path.length <= max) return path;
    const keep = max - 1;
    const head = Math.ceil(keep * 0.4);
    return path.slice(0, head) + '…' + path.slice(path.length - (keep - head));
  }

  const r = $derived(report);

  /**
   * Every image the build ships, atlas pages included — the checklist asks for
   * each one to be LOOKED AT before upload (a placeholder, a stray gradient or
   * a leftover from the scaffold is invisible in any table of bytes), and they
   * are served by SlotLab itself at /games/<name>/<path>.
   */
  const images = $derived.by(() => {
    if (!r) return [];
    const seen = new Map<string, { path: string; width: number; height: number }>();
    for (const i of r.items) {
      if (i.category === 'texture') {
        seen.set(i.path, { path: i.path, width: i.width, height: i.height });
      }
    }
    for (const g of r.groups) {
      for (const v of g.variants) {
        for (const p of v.pages) {
          seen.set(p.path, { path: p.path, width: p.width, height: p.height });
        }
      }
    }
    return [...seen.values()].sort((a, b) => a.path.localeCompare(b.path));
  });
  /** Widest bar in a category chart — everything else is drawn against it. */
  const maxCategoryVram = $derived(
    Math.max(1, ...(r?.categories ?? []).map((c) => c.vram)),
  );
  const maxCategoryBytes = $derived(
    Math.max(1, ...(r?.categories ?? []).map((c) => c.bytes)),
  );
  /**
   * VRAM the variant choice puts at stake — the gap between the cheapest and the
   * priciest device profile. It is the single most actionable number here: it
   * says what the low-end device is spared, and what the high-end one pays.
   */
  const swing = $derived(r ? r.max.vram - r.min.vram : 0);
  const download = $derived(r ? spread(r.min.download, r.max.download) : null);
  const vram = $derived(r ? spread(r.min.vram, r.max.vram) : null);
  const files = $derived(
    r ? (showAllFiles ? r.items : r.items.slice(0, FILES_SHOWN)) : [],
  );
</script>

<div class="head">
  <div class="head-text">
    <h2>Asset budget</h2>
    {#if r}
      <p class="faint path" title={r.buildDir}>{middle(r.buildDir)}</p>
    {/if}
  </div>
  <button
    class="primary"
    onclick={() => app.selectedGame && load(app.selectedGame)}
    disabled={loading || !app.selectedGame}
    aria-busy={loading}
  >
    {loading ? 'Scanning…' : 'Rescan'}
  </button>
</div>

{#if error}
  <div class="banner error" role="alert">{error}</div>
{:else if !r}
  <p class="dim" role="status">{loading ? 'Scanning the build…' : 'Select a game.'}</p>
{:else}
  <!-- ── The two numbers that matter ──────────────────────────────────────── -->
  <div class="kpis">
    <div class="kpi">
      <span class="kpi-label">Download at startup</span>
      <span class="kpi-value">{download?.value}<i>{download?.unit}</i></span>
      <span class="kpi-note">
        Lightest device → heaviest. On disk: {fmtBytes(r.diskBytes)}
        across {fmtInt(r.fileCount)} files, every variant included.
      </span>
    </div>
    <div class="kpi">
      <span class="kpi-label">VRAM once uploaded</span>
      <span class="kpi-value">{vram?.value}<i>{vram?.unit}</i></span>
      <span class="kpi-note">
        Textures decompressed to RGBA8 (w × h × 4). The atlas scale chosen by the
        device swings {fmtBytes(swing)} on its own.
      </span>
    </div>
  </div>

  {#if r.warnings.length > 0}
    <div class="panel warn-panel">
      <h3>Worth a look</h3>
      {#each r.warnings as w (w.path)}
        <p class="warn-row">
          <span class="pill yellow">{fmtBytes(w.vram)} VRAM</span>
          <code>{w.path}</code>
          <span class="dim">{w.message}</span>
        </p>
      {/each}
    </div>
  {/if}

  <!-- ── Where the weight sits ────────────────────────────────────────────── -->
  <div class="panel">
    <h3>By category <span class="dim">— heaviest configuration</span></h3>
    <table class="fixed cats">
      <colgroup>
        <col class="c-cat" />
        <col class="c-size" />
        <col />
        <col class="c-size" />
        <col />
      </colgroup>
      <thead>
        <tr>
          <th>Category</th>
          <th class="num">Download</th>
          <th><span class="sr-only">Download share</span></th>
          <th class="num">VRAM</th>
          <th><span class="sr-only">VRAM share</span></th>
        </tr>
      </thead>
      <tbody>
        {#each r.categories as c (c.label)}
          <tr>
            <td class="cat">{c.label}</td>
            <td class="num">{fmtBytes(c.bytes)}</td>
            <td class="bar-cell">
              <span class="bar dl" style="width: {(c.bytes / maxCategoryBytes) * 100}%"
              ></span>
            </td>
            <td class="num">{c.vram > 0 ? fmtBytes(c.vram) : '—'}</td>
            <td class="bar-cell">
              <span class="bar vram" style="width: {(c.vram / maxCategoryVram) * 100}%"
              ></span>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  <!-- ── The choices the device makes ─────────────────────────────────────── -->
  <div class="panel">
    <h3>
      Variants <span class="dim">— one per group is loaded, picked from the device</span>
    </h3>
    {#each r.groups as g (g.kind + g.name)}
      <div class="group">
        <div class="group-head">
          <strong>{g.name}</strong>
          <span class="pill blue">
            {g.kind === 'atlasScale' ? 'atlas scale' : 'audio format'}
          </span>
        </div>
        <!-- Same columns in every group, so the numbers line up down the panel. -->
        <table class="fixed variants">
          <colgroup>
            <col class="c-variant" />
            <col class="c-size" />
            <col class="c-size" />
            <col />
          </colgroup>
          <thead>
            <tr>
              <th>Variant</th>
              <th class="num">Download</th>
              <th class="num">VRAM</th>
              <th class="pages-head">Pixels</th>
            </tr>
          </thead>
          <tbody>
            {#each g.variants as v (v.label)}
              <tr>
                <td class="variant" title={v.label}>{v.label}</td>
                <td class="num">{fmtBytes(v.bytes)}</td>
                <td class="num vram-col">{v.vram > 0 ? fmtBytes(v.vram) : '—'}</td>
                <td class="pages">
                  {#each v.pages as p (p.path)}
                    <span class="page" title="{p.path} — {fmtBytes(p.bytes)}">
                      {p.width}×{p.height}
                    </span>
                  {:else}
                    <span class="faint">—</span>
                  {/each}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/each}
  </div>

  <!-- ── Always loaded, no alternative ────────────────────────────────────── -->
  <div class="panel">
    <h3>Standalone files <span class="dim">— always downloaded, by VRAM</span></h3>
    <table class="fixed">
      <colgroup>
        <col />
        <col class="c-cat" />
        <col class="c-size" />
        <col class="c-px" />
        <col class="c-size" />
      </colgroup>
      <thead>
        <tr>
          <th>File</th>
          <th>Category</th>
          <th class="num">Size</th>
          <th class="num">Pixels</th>
          <th class="num">VRAM</th>
        </tr>
      </thead>
      <tbody id="asset-files">
        {#each files as i (i.path)}
          <tr>
            <td class="file" title={i.path}><code>{i.path}</code></td>
            <td class="cat">{i.category}</td>
            <td class="num">{fmtBytes(i.bytes)}</td>
            <td class="num dim">{i.width ? `${i.width}×${i.height}` : '—'}</td>
            <td class="num">{i.vram > 0 ? fmtBytes(i.vram) : '—'}</td>
          </tr>
        {/each}
      </tbody>
    </table>
    {#if r.items.length > FILES_SHOWN}
      <button
        class="ghost more"
        aria-expanded={showAllFiles}
        aria-controls="asset-files"
        onclick={() => (showAllFiles = !showAllFiles)}
      >
        {showAllFiles
          ? `Show the ${FILES_SHOWN} heaviest only`
          : `Show ${r.items.length - FILES_SHOWN} lighter files`}
      </button>
    {/if}
  </div>

  <!-- ── Look at every image once ─────────────────────────────────────────── -->
  <details class="panel sheet">
    <summary>
      Contact sheet <span class="dim">— {images.length} images, as they ship</span>
    </summary>
    <p class="dim">
      The one check no rule replaces: a placeholder, a stray gradient, an
      asset inherited from the scaffold or a symbol recycled as decoration is
      obvious here and invisible in every table above. Checkerboard = the
      transparent parts.
    </p>
    <div class="grid">
      {#each images as img (img.path)}
        <figure>
          <img
            src="/games/{app.selectedGame}/{img.path}"
            alt={img.path}
            loading="lazy"
          />
          <figcaption title={img.path}>
            {img.path.split('/').pop()}
            <span class="dim">{img.width ? `${img.width}×${img.height}` : ''}</span>
          </figcaption>
        </figure>
      {/each}
    </div>
  </details>
{/if}

<style>
  .sheet summary {
    cursor: pointer;
    font-weight: 640;
    margin-bottom: var(--sp-2);
  }

  .sheet .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(132px, 1fr));
    gap: var(--sp-3);
    margin-top: var(--sp-3);
  }

  .sheet figure {
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .sheet img {
    width: 100%;
    height: 108px;
    object-fit: contain;
    border: 1px solid var(--border);
    border-radius: var(--r-s);
    /* Checkerboard: transparency has to be visible, it is half the mistakes. */
    background-color: var(--border);
    background-image:
      linear-gradient(45deg, var(--bg-raised) 25%, transparent 25%),
      linear-gradient(-45deg, var(--bg-raised) 25%, transparent 25%),
      linear-gradient(45deg, transparent 75%, var(--bg-raised) 75%),
      linear-gradient(-45deg, transparent 75%, var(--bg-raised) 75%);
    background-size: 14px 14px;
    background-position: 0 0, 0 7px, 7px -7px, -7px 0;
  }

  .sheet figcaption {
    font-size: var(--fs-2xs);
    line-height: 1.35;
    overflow-wrap: anywhere;
    color: var(--text-dim);
  }

  .head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: var(--sp-4);
    margin-bottom: var(--sp-4);
  }

  .head-text {
    min-width: 0;
  }

  .path {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    margin: 2px 0 0;
    white-space: nowrap;
  }

  .kpis {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 14px;
    margin-bottom: var(--sp-4);
  }

  .kpi {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    padding: var(--sp-4) var(--sp-5);
  }

  .kpi-label {
    font-size: var(--fs-2xs);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--text-faint);
  }

  .kpi-value {
    font-family: var(--font-mono);
    font-size: var(--fs-xl);
    font-weight: 600;
    color: var(--gold);
    font-variant-numeric: tabular-nums;
  }

  .kpi-value i {
    font-size: var(--fs-base);
    font-style: normal;
    color: var(--text-faint);
    margin-left: 6px;
  }

  .kpi-note {
    font-size: var(--fs-xs);
    color: var(--text-dim);
    line-height: 1.45;
  }

  .panel {
    margin-bottom: 14px;
  }

  h3 {
    margin: 0 0 var(--sp-3);
    font-size: var(--fs-base);
    font-weight: 640;
  }

  h3 .dim {
    font-weight: 400;
  }

  .warn-panel {
    border-color: color-mix(in oklch, var(--warn) 35%, transparent);
  }

  .warn-row {
    display: flex;
    align-items: baseline;
    gap: 10px;
    flex-wrap: wrap;
    margin: 0 0 6px;
    font-size: var(--fs-sm);
  }

  .cat,
  .variant {
    color: var(--text-dim);
    font-size: var(--fs-sm);
  }

  .variant {
    font-family: var(--font-mono);
    color: var(--text);
  }

  /* Shared column widths (table.fixed): every table of the page lines up. */
  .c-cat {
    width: 140px;
  }

  .c-variant {
    width: 180px;
  }

  .c-size {
    width: 104px;
  }

  .c-px {
    width: 110px;
  }

  .variant,
  .file {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bar {
    display: block;
    height: 6px;
    border-radius: var(--r-pill);
    min-width: 1px;
  }

  .bar.dl {
    background: var(--info);
  }

  .bar.vram {
    background: var(--gold);
  }

  .group {
    margin-bottom: 14px;
  }

  .group-head {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-bottom: 2px;
    font-size: var(--fs-sm);
  }

  .vram-col {
    color: var(--gold);
  }

  .pages,
  .pages-head {
    padding-left: 14px;
  }

  .page {
    display: inline-block;
    font-family: var(--font-mono);
    font-size: var(--fs-2xs);
    color: var(--text-faint);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--r-s);
    padding: 0 5px;
    margin-right: var(--sp-1);
  }

  .file code {
    font-size: var(--fs-xs);
  }

  .more {
    margin-top: var(--sp-2);
    font-size: var(--fs-sm);
  }
</style>
