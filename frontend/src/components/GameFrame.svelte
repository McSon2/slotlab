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
  }

  const { url, width, height, label, game, scale = 1 }: Props = $props();

  const title = $derived(
    game ? `${game} — ${label} (${width}×${height})` : `${label} (${width}×${height})`,
  );
</script>

<div class="frame-card">
  <div class="frame-head">
    <span>{label}</span>
    <span class="dim num">{width}×{height}</span>
  </div>
  <div
    class="frame-clip"
    style="width: {width * scale}px; height: {height * scale}px;"
  >
    <iframe
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
