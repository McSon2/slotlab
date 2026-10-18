<script lang="ts">
  interface Props {
    url: string;
    width: number;
    height: number;
    label: string;
    /** Game name, for the frame's accessible title. */
    game?: string;
    /** Display factor — the iframe's LOGICAL dimensions stay
     * width×height (essential for faithful responsive QA). */
    scale?: number;
    /** Game clock rate (1 = real time, 0 = frozen), applied by the time
     * warp SlotLab injects into the builds it serves. */
    speed?: number;
    /** Silences the game (Web Audio and media elements), same channel. */
    muted?: boolean;
  }

  const { url, width, height, label, game, scale = 1, speed = 1, muted = false }: Props = $props();

  let frame = $state<HTMLIFrameElement>();

  function sendSpeed() {
    frame?.contentWindow?.postMessage({ slotlab: 'speed', value: speed }, '*');
  }

  function sendMuted() {
    frame?.contentWindow?.postMessage({ slotlab: 'mute', value: muted }, '*');
  }

  $effect(() => {
    void speed;
    sendSpeed();
  });

  $effect(() => {
    void muted;
    sendMuted();
  });

  // The injected script asks for the rate as soon as it runs, before the
  // game boots: a reloaded frame starts at the chosen speed (and mute).
  $effect(() => {
    const onMessage = (e: MessageEvent) => {
      if (e.source === frame?.contentWindow && e.data?.slotlab === 'speed?') {
        sendSpeed();
        sendMuted();
      }
    };
    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  });

  const title = $derived(
    game ? `${game} — ${label} (${width}×${height})` : `${label} (${width}×${height})`,
  );
</script>

<div class="frame-card">
  <div class="frame-head">
    <span>{label}</span>
    <span class="dim num">
      {#if muted}<span class="speed">muted</span>{/if}
      {#if speed !== 1}<span class="speed">{speed === 0 ? 'paused' : `×${speed}`}</span>{/if}
      {width}×{height}
    </span>
  </div>
  <div
    class="frame-clip"
    style="width: {width * scale}px; height: {height * scale}px;"
  >
    <iframe
      bind:this={frame}
      src={url}
      {title}
      style="width: {width}px; height: {height}px; transform: scale({scale});"
      allow="autoplay; fullscreen"
    ></iframe>
  </div>
</div>

<style>
  .frame-card {
    display: inline-flex;
    flex-direction: column;
    gap: var(--sp-1);
  }

  .frame-head {
    display: flex;
    justify-content: space-between;
    gap: var(--sp-2);
    font-size: var(--fs-xs);
    padding: 0 2px;
  }

  .frame-head .num {
    font-size: var(--fs-xs);
  }

  .speed {
    color: var(--warn);
    margin-right: var(--sp-1);
  }

  .frame-clip {
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: var(--r-m);
    /* Deliberately pure black, not a theme token: it is the letterbox the
       game sits in on the real page, whatever SlotLab's theme. */
    background: #000;
  }

  iframe {
    border: 0;
    transform-origin: top left;
    display: block;
  }
</style>
