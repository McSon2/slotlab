<script lang="ts">
  import { mutes } from '../lib/muted.svelte';
  import type { MutedGroup } from '../lib/muted';

  /** The findings muted for this game, folded; each can be brought back. */
  interface Props {
    groups: MutedGroup[];
    /** Findings hidden right now (the summary's number). */
    count: number;
  }

  const { groups, count }: Props = $props();
</script>

{#if groups.length}
  <details class="muted-list">
    <summary>
      {count} muted finding{count === 1 ? '' : 's'}
      {#if groups.length !== count}<span class="faint"> · {groups.length} mute{groups.length === 1 ? '' : 's'}</span>{/if}
    </summary>
    <ul>
      {#each groups as g (`${g.mute.rule}|${g.mute.mode}`)}
        <li>
          <span class="what">
            {#if g.titles.length}{g.titles.join(' · ')}{:else}<span class="faint">not raised right now</span>{/if}
            <code>{g.mute.rule}</code>
          </span>
          <span class="scope">{g.mute.mode ?? 'every mode'}</span>
          <button class="ghost tiny" disabled={mutes.busy} onclick={() => mutes.unmute(g.mute.rule, g.mute.mode)}>Unmute</button>
        </li>
      {/each}
    </ul>
    {#if mutes.error}<p class="err" role="alert">{mutes.error}</p>{/if}
  </details>
{:else if mutes.error}
  <p class="err" role="alert">Muted findings: {mutes.error}</p>
{/if}

<style>
  .muted-list {
    margin-top: 12px;
    font-size: var(--fs-sm);
  }

  summary {
    cursor: pointer;
    color: var(--text-faint);
    font-size: var(--fs-xs);
  }

  ul {
    list-style: none;
    margin: 6px 0 0;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--r-m);
  }

  li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: 10px;
    align-items: baseline;
    padding: 6px 12px;
    color: var(--text-dim);
  }

  li + li {
    border-top: 1px solid var(--border);
  }

  code {
    margin-left: 6px;
    font-size: var(--fs-2xs);
    color: var(--text-faint);
  }

  .scope {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    color: var(--text-faint);
  }

  button.tiny {
    padding: 1px 8px;
    font-size: var(--fs-xs);
  }

  .err {
    color: var(--fail);
    font-size: var(--fs-xs);
  }
</style>
