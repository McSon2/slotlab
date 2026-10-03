<script lang="ts" module>
  export type IconName =
    | 'games'
    | 'play'
    | 'board'
    | 'analyze'
    | 'balance'
    | 'challenge'
    | 'crowdsim'
    | 'scan'
    | 'preflight'
    | 'assets'
    | 'review'
    | 'publish'
    | 'help'
    | 'folder'
    | 'close'
    | 'refresh'
    | 'star'
    | 'link'
    | 'chevron-left'
    | 'chevron-right';
</script>

<script lang="ts">
  /** Stroke icons (Lucide geometry, ISC) — one consistent size/weight
   * instead of Unicode glyphs that each fall back to a different font. */
  let { name, size = 16 }: { name: IconName; size?: number } = $props();
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
>
  {#if name === 'games'}
    <path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" />
  {:else if name === 'play'}
    <polygon points="6 3 20 12 6 21 6 3" />
  {:else if name === 'board'}
    <rect width="18" height="18" x="3" y="3" rx="2" />
    <path d="M3 9h18M3 15h18M9 3v18M15 3v18" />
  {:else if name === 'analyze'}
    <path d="M18 7V4H6l6 8-6 8h12v-3" />
  {:else if name === 'balance'}
    <path d="m16 16 3-8 3 8c-.87.65-1.92 1-3 1s-2.13-.35-3-1Z" />
    <path d="m2 16 3-8 3 8c-.87.65-1.92 1-3 1s-2.13-.35-3-1Z" />
    <path d="M7 21h10M12 3v18M3 7h2c2 0 5-1 7-2 2 1 5 2 7 2h2" />
  {:else if name === 'challenge'}
    <circle cx="12" cy="12" r="10" />
    <circle cx="12" cy="12" r="6" />
    <circle cx="12" cy="12" r="2" />
  {:else if name === 'crowdsim'}
    <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" />
    <circle cx="9" cy="7" r="4" />
    <path d="M22 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75" />
  {:else if name === 'scan'}
    <circle cx="11" cy="11" r="8" />
    <path d="m21 21-4.3-4.3" />
  {:else if name === 'preflight'}
    <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
    <path d="m9 11 3 3L22 4" />
  {:else if name === 'assets'}
    <rect width="18" height="18" x="3" y="3" rx="2" />
    <circle cx="9" cy="9" r="2" />
    <path d="m21 15-3.09-3.09a2 2 0 0 0-2.82 0L6 21" />
  {:else if name === 'review' || name === 'star'}
    <polygon
      points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"
    />
  {:else if name === 'publish'}
    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
    <path d="m17 8-5-5-5 5M12 3v12" />
  {:else if name === 'help'}
    <circle cx="12" cy="12" r="10" />
    <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3M12 17h.01" />
  {:else if name === 'folder'}
    <path
      d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"
    />
  {:else if name === 'close'}
    <path d="M18 6 6 18M6 6l12 12" />
  {:else if name === 'refresh'}
    <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
    <path d="M21 3v5h-5M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
    <path d="M8 16H3v5" />
  {:else if name === 'chevron-left'}
    <path d="m15 18-6-6 6-6" />
  {:else if name === 'chevron-right'}
    <path d="m9 18 6-6-6-6" />
  {:else if name === 'link'}
    <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" />
    <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />
  {/if}
</svg>

<style>
  .icon {
    flex-shrink: 0;
    vertical-align: -0.15em;
  }
</style>
