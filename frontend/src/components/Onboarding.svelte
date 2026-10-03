<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { api } from '../lib/api';
  import { GameAdder } from '../lib/addGame.svelte';
  import { storageSet } from '../lib/storage';
  import { app, errorText } from '../lib/stores.svelte';
  import Icon from './Icon.svelte';

  let {
    trusted = false,
    onclose,
  }: { trusted?: boolean; onclose: () => void } = $props();

  // 0 welcome · 1 certificate · 2 first game · 3 ready
  let step = $state(0);
  const steps = ['Welcome', 'Certificate', 'First game', 'Ready'];
  const titleId = 'onboarding-title';

  let dialogEl = $state<HTMLDialogElement>();

  // The prop is only a hint (App may still be fetching when we mount) —
  // ask the server ourselves so step 1 never wrongly shows "installed".
  let tlsTrusted = $state(untrack(() => trusted));
  let tlsBusy = $state(false);
  let tlsError = $state<string | null>(null);

  const adder = new GameAdder();

  /** Games present before this guide added any (reopened via "?"). */
  const hasGames = $derived(app.games.length > 0);

  onMount(() => {
    // Native modal: focus trap, Escape (→ `cancel`) and the backdrop for free.
    if (dialogEl && !dialogEl.open) dialogEl.showModal();
    api.tlsStatus().then((s) => (tlsTrusted = s.trusted)).catch(() => {});
  });

  /** Each step lands on its primary action. */
  async function focusPrimary() {
    await tick();
    dialogEl?.querySelector<HTMLElement>('[data-primary]:not(:disabled)')?.focus();
  }

  $effect(() => {
    void step;
    void focusPrimary();
  });

  let finished = false;

  function finish(openPlay: boolean) {
    if (finished) return;
    finished = true;
    storageSet('slotlab.onboarded', '1');
    if (openPlay) app.tab = app.games.length > 0 ? 'play' : 'games';
    if (dialogEl?.open) dialogEl.close();
    onclose();
  }

  function onCancel(e: Event) {
    // Escape = "Skip setup".
    e.preventDefault();
    finish(false);
  }

  async function installCa() {
    tlsBusy = true;
    tlsError = null;
    try {
      const r = await api.tlsInstall();
      tlsTrusted = r.trusted;
      if (!r.trusted) tlsError = 'The certificate was not marked as trusted. Try again.';
    } catch (e) {
      tlsError = errorText(e);
    } finally {
      tlsBusy = false;
    }
    // The install button is gone once trusted: land on "Continue".
    if (tlsTrusted) void focusPrimary();
  }
</script>

<dialog
  bind:this={dialogEl}
  aria-labelledby={titleId}
  oncancel={onCancel}
  onclose={() => finish(false)}
>
  <div class="card">
    <header>
      <div class="progress">
        <div class="dots" aria-hidden="true">
          {#each steps as label, i (label)}
            <span class="dot" class:on={i === step} class:done={i < step}></span>
          {/each}
        </div>
        <span class="step-count" aria-live="polite">
          Step {step + 1} of {steps.length} · {steps[step]}
        </span>
      </div>
      <button class="ghost skip" onclick={() => finish(false)}>Skip setup</button>
    </header>

    {#if step === 0}
      <div class="body center">
        <div class="chip" aria-hidden="true">SL</div>
        <h1 id={titleId}>Welcome to SlotLab</h1>
        <p class="lead">
          Your dev &amp; QA bench for Stake Engine slots — play any game against
          a local RGS, audit its math, and publish a playable build in one click.
        </p>
        <ul class="features">
          <li>
            <span class="glyph"><Icon name="games" size={15} /></span>
            <div>
              <strong>Setup</strong>
              <span>Add games from their math folder — books are imported for you.</span>
            </div>
          </li>
          <li>
            <span class="glyph"><Icon name="play" size={15} /></span>
            <div>
              <strong>Play</strong>
              <span>Run games against a local RGS: viewports, forced outcomes, replays, entry board.</span>
            </div>
          </li>
          <li>
            <span class="glyph"><Icon name="analyze" size={15} /></span>
            <div>
              <strong>Math</strong>
              <span>RTP, volatility, distribution and compliance checks per mode; balance and crowd simulations.</span>
            </div>
          </li>
          <li>
            <span class="glyph"><Icon name="preflight" size={15} /></span>
            <div>
              <strong>QA</strong>
              <span>Scan the books, preflight the build, audit the assets, review before submission.</span>
            </div>
          </li>
          <li>
            <span class="glyph"><Icon name="publish" size={15} /></span>
            <div>
              <strong>Ship</strong>
              <span>Publish a playable build — a temporary public link or your homelab.</span>
            </div>
          </li>
        </ul>
      </div>
      <footer>
        <span></span>
        <button class="primary big" data-primary onclick={() => (step = 1)}>Get started</button>
      </footer>
    {:else if step === 1}
      <div class="body">
        <p class="overline">Local HTTPS certificate</p>
        <h2 id={titleId}>Let your browser trust the local casino server</h2>
        <p>
          SlotLab emulates Stake's game server (the RGS) on your machine.
          Games only talk to it over <code>https</code>, so SlotLab needs its
          local certificate — <em>SlotLab Local CA</em> — added to your macOS
          keychain.
        </p>
        <ul class="reassure">
          <li>
            <Icon name="preflight" size={13} />
            <span>macOS will ask for <strong>your session password</strong> — that's the standard system dialog, nothing else is changed.</span>
          </li>
          <li>
            <Icon name="preflight" size={13} />
            <span>The certificate only vouches for <code>localhost</code>, is stored in <strong>your</strong> login keychain, and never leaves this Mac.</span>
          </li>
          <li>
            <Icon name="preflight" size={13} />
            <span>You can remove it anytime in Keychain Access (search “SlotLab”).</span>
          </li>
        </ul>
        {#if tlsTrusted}
          <p class="status ok" role="status">
            <Icon name="preflight" size={14} /> Certificate installed — you're good.
          </p>
        {:else}
          <button class="primary big install" data-primary onclick={installCa} disabled={tlsBusy}>
            {tlsBusy ? 'Waiting for macOS…' : 'Install the certificate'}
          </button>
          {#if tlsError}
            <p class="status err" role="alert">{tlsError}</p>
          {/if}
        {/if}
      </div>
      <footer>
        <button class="ghost" onclick={() => (step = 0)}>Back</button>
        {#if tlsTrusted}
          <button class="primary big" data-primary onclick={() => (step = 2)}>Continue</button>
        {:else}
          <button class="ghost" onclick={() => (step = 2)}>Do this later</button>
        {/if}
      </footer>
    {:else if step === 2}
      <div class="body">
        <p class="overline">Add your first game</p>
        <h2 id={titleId}>Point SlotLab at a game's math</h2>
        <p>
          Pick a game folder, a repo, or any parent folder — SlotLab digs out
          every <code>publish_files</code> inside, derives the names and
          imports the books on its own. Several games found = add them all in
          one click.
        </p>
        {#if hasGames && adder.added.length === 0}
          <p class="faint small">
            {app.games.length} game{app.games.length > 1 ? 's' : ''} already in SlotLab —
            add another one, or continue.
          </p>
        {/if}
        <form
          class="add-row"
          onsubmit={(e) => {
            e.preventDefault();
            void adder.addTyped();
          }}
        >
          <button type="button" data-primary onclick={adder.browse} disabled={adder.busy}>
            <Icon name="folder" size={14} /> Browse…
          </button>
          <input
            bind:value={adder.path}
            aria-label="Game folder path"
            placeholder="…or paste a path: a game folder or its publish_files"
          />
          <button type="submit" class="primary" disabled={adder.busy || !adder.path.trim()}>
            {adder.busy ? 'Adding…' : adder.derivedName ? `Add ${adder.derivedName}` : 'Add'}
          </button>
        </form>
        {#if adder.candidates.length > 0}
          <p class="faint found">
            {adder.candidates.length} games found:
            <button class="primary" onclick={adder.addAll} disabled={adder.busy}>
              {adder.busy ? 'Adding…' : `Add all ${adder.candidates.length}`}
            </button>
            <span>or pick them one by one —</span>
          </p>
          <div class="cands">
            {#each adder.candidates as c (c.path)}
              <button
                class="cand"
                title={c.path}
                onclick={() => adder.addOne(c.name, c.path)}
                disabled={adder.busy}
              >
                + {c.name}
              </button>
            {/each}
          </div>
        {/if}
        {#if adder.added.length > 0}
          <p class="status ok" role="status">
            <Icon name="preflight" size={14} />
            Added: {adder.added.join(', ')} — books are importing in the background.
          </p>
        {/if}
        {#if adder.info}
          <p class="status" role="status">{adder.info}</p>
        {/if}
        {#if adder.error}
          <p class="status err" role="alert">{adder.error}</p>
        {/if}
      </div>
      <footer>
        <button class="ghost" onclick={() => (step = 1)}>Back</button>
        {#if adder.added.length > 0 || hasGames}
          <button class="primary big" onclick={() => (step = 3)}>Continue</button>
        {:else}
          <button class="ghost" onclick={() => (step = 3)}>Do this later</button>
        {/if}
      </footer>
    {:else}
      <div class="body center">
        <div class="chip ok-chip" aria-hidden="true"><Icon name="preflight" size={26} /></div>
        <h1 id={titleId}>You're all set</h1>
        <p class="lead">
          {#if adder.added.length > 0}
            <strong>{adder.added.join(', ')}</strong>
            {adder.added.length > 1 ? 'are' : 'is'} importing books in the
            background (a few minutes for a big game, one time only). The
            <strong>Games</strong> tab shows the progress; everything else is
            ready as soon as it turns green.
          {:else if hasGames}
            Your games are ready. Add more anytime from the
            <strong>Games</strong> tab.
          {:else}
            Add a game anytime from the <strong>Games</strong> tab — everything
            else lights up from there.
          {/if}
        </p>
        <p class="faint">
          Tip: reopen this guide anytime via the “?” button, bottom-left.
        </p>
      </div>
      <footer>
        <button class="ghost" onclick={() => (step = 2)}>Back</button>
        <button class="primary big" data-primary onclick={() => finish(true)}>Open SlotLab</button>
      </footer>
    {/if}
  </div>
</dialog>

<style>
  dialog {
    width: min(680px, calc(100% - 48px));
    max-width: none;
    max-height: calc(100% - 48px);
    padding: 0;
    color: var(--text);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    box-shadow: 0 30px 80px color-mix(in oklch, black 45%, transparent);
  }

  /* Same opaque veil as before: the app behind is inert anyway. */
  dialog::backdrop {
    background:
      radial-gradient(
        1200px 500px at 75% -10%,
        color-mix(in oklch, var(--bg-panel, oklch(0.19 0.014 165)) 85%, transparent),
        transparent 70%
      ),
      var(--bg, oklch(0.16 0.012 165));
  }

  .card {
    min-height: 540px;
    display: flex;
    flex-direction: column;
    padding: var(--sp-5) 34px 26px;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 18px;
  }

  .progress {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
  }

  .dots {
    display: flex;
    gap: 7px;
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--border-strong);
    transition: background 0.2s var(--ease-out), transform 0.2s var(--ease-out);
  }

  .dot.on {
    background: var(--gold);
    transform: scale(1.25);
  }

  .dot.done {
    background: color-mix(in oklch, var(--gold) 45%, transparent);
  }

  .step-count {
    font-size: var(--fs-xs);
    color: var(--text-faint);
  }

  .skip {
    font-size: var(--fs-sm);
    color: var(--text-faint);
  }

  .body {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }

  .body.center {
    align-items: center;
    text-align: center;
  }

  .chip {
    display: grid;
    place-items: center;
    width: 58px;
    height: 58px;
    border-radius: 50%;
    background: var(--gold);
    color: var(--gold-ink);
    font-size: var(--fs-xl);
    font-weight: 800;
    margin-bottom: 18px;
    box-shadow:
      0 0 0 5px var(--bg-panel),
      0 0 0 6.5px color-mix(in oklch, var(--gold) 40%, transparent);
  }

  .ok-chip {
    background: var(--pass);
    color: var(--bg-deep);
    box-shadow:
      0 0 0 5px var(--bg-panel),
      0 0 0 6.5px color-mix(in oklch, var(--pass) 40%, transparent);
  }

  h1 {
    font-size: var(--fs-xl);
    margin-bottom: var(--sp-2);
  }

  h2 {
    font-size: var(--fs-lg);
    margin-bottom: 10px;
  }

  .lead {
    font-size: var(--fs-md);
    color: var(--text-dim);
    max-width: 46ch;
    margin: 0 0 22px;
  }

  .features {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    text-align: left;
  }

  .features li {
    display: flex;
    gap: 14px;
    align-items: baseline;
  }

  .features .glyph {
    color: var(--gold);
    width: 18px;
    text-align: center;
    flex-shrink: 0;
  }

  .features strong {
    display: block;
    font-weight: 660;
  }

  .features span:not(.glyph) {
    color: var(--text-dim);
    font-size: var(--fs-sm);
  }

  .reassure {
    list-style: none;
    padding: 14px var(--sp-4);
    margin: 14px 0 var(--sp-5);
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--r-s);
    font-size: var(--fs-sm);
    color: var(--text-dim);
  }

  .reassure li {
    display: flex;
    gap: var(--sp-2);
    align-items: baseline;
  }

  .reassure li > :global(svg) {
    color: var(--pass);
    position: relative;
    top: 2px;
  }

  .install {
    align-self: flex-start;
  }

  .add-row {
    display: flex;
    gap: 10px;
    margin-top: 14px;
  }

  .add-row button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .add-row input {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-sm);
  }

  .found {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    margin: 14px 0 var(--sp-2);
    font-size: var(--fs-sm);
  }

  .cands {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .cand {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
  }

  .small {
    font-size: var(--fs-sm);
    margin: 0;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 14px 0 0;
    font-size: var(--fs-base);
    color: var(--text-dim);
  }

  .status.ok {
    color: var(--pass);
  }

  .status.err {
    color: var(--fail);
  }

  .primary.big {
    padding: 9px 22px;
    font-size: var(--fs-base);
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 22px;
  }
</style>
