<script lang="ts">
  /** Publish: the two ways to put a game in front of someone — a temporary
   * Cloudflare link (nothing to set up) or the permanent homelab. */
  import { app } from '../lib/stores.svelte';
  import Share from './Share.svelte';
  import Deploy from './Deploy.svelte';

  const views = [
    { id: 'link', label: 'Temporary link', hint: 'Cloudflare quick tunnel · dies with SlotLab' },
    { id: 'homelab', label: 'Homelab', hint: 'Permanent subdomain · survives restarts' },
  ] as const;
</script>

<nav class="view-tabs" aria-label="Publish target">
  {#each views as v (v.id)}
    <button
      class:active={app.publishView === v.id}
      aria-current={app.publishView === v.id ? 'page' : undefined}
      onclick={() => (app.publishView = v.id)}
    >
      {v.label}
    </button>
  {/each}
  <span class="view-context">{views.find((v) => v.id === app.publishView)?.hint}</span>
</nav>

{#if app.publishView === 'link'}
  <Share />
{:else}
  <Deploy />
{/if}
