<script lang="ts">
  import { mutes } from '../lib/muted.svelte';

  /**
   * « Mute » on one finding: in this mode, or in every mode of the game.
   * The page only shows it for a finding that may be muted (never a blocking one).
   */
  interface Props {
    rule: string;
    /** The mode the finding is read in; `null`: a whole-game finding (every mode only). */
    mode: string | null;
  }

  const { rule, mode }: Props = $props();
  let asking = $state(false);

  async function go(scope: string | null) {
    asking = false;
    await mutes.mute(rule, scope);
  }
</script>

<span class="mute">
  {#if !asking}
    <button
      class="ghost tiny"
      title="Stop counting this finding for this game — it stays listed under « muted » and can be brought back"
      disabled={mutes.busy}
      onclick={() => (mode === null ? go(null) : (asking = true))}
    >
      Mute
    </button>
  {:else}
    <span class="faint">Mute</span>
    <button class="ghost tiny" onclick={() => go(mode)}>in {mode}</button>
    <button class="ghost tiny" onclick={() => go(null)}>in every mode</button>
    <button class="ghost tiny" aria-label="Cancel" onclick={() => (asking = false)}>✕</button>
  {/if}
</span>

<style>
  .mute {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
    font-size: var(--fs-xs);
  }

  button.tiny {
    padding: 1px 8px;
    font-size: var(--fs-xs);
    color: var(--text-faint);
  }

  button.tiny:hover:not(:disabled) {
    color: var(--text);
  }
</style>
