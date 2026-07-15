<script lang="ts">
  interface Props {
    url: string;
    width: number;
    height: number;
    label: string;
    /** Display factor — the iframe's LOGICAL dimensions stay
     * width×height (essential for faithful responsive QA). */
    scale?: number;
  }

  const { url, width, height, label, scale = 1 }: Props = $props();
</script>

<div class="frame-card">
  <div class="frame-head">
    <span>{label}</span>
    <span class="dim">{width}×{height}</span>
  </div>
  <div
    class="frame-clip"
    style="width: {width * scale}px; height: {height * scale}px;"
  >
    <iframe
      src={url}
      title={label}
      style="width: {width}px; height: {height}px; transform: scale({scale});"
      allow="autoplay; fullscreen"
    ></iframe>
  </div>
</div>

<style>
  .frame-card {
    display: inline-flex;
    flex-direction: column;
    gap: 4px;
  }

  .frame-head {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    padding: 0 2px;
  }

  .frame-clip {
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: #000;
  }

  iframe {
    border: 0;
    transform-origin: top left;
    display: block;
  }
</style>
