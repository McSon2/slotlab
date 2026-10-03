<script lang="ts">
  import PopulationDetail from './PopulationDetail.svelte';
  import { fx, oneIn, tree, refId, type ModePopulations, type PopRef } from '../../lib/populations';

  /**
   * One mode, every population of it: a tree on the left (mode, bonuses by
   * tier and variant, outcome types), the full picture of the selected one on
   * the right.
   */
  interface Props {
    all: ModePopulations[];
    mode: string;
    selectedKey: string;
    pinned: Set<string>;
    multiTrigger?: { share: number; books: number } | null;
    customThresholds: number[];
    onSelect: (ref: PopRef) => void;
    onTogglePin: (ref: PopRef) => void;
    onPinMany: (refs: PopRef[]) => void;
    onPlay: (mode: string, simId: number) => void | Promise<void>;
    onBookmark: (mode: string, simId: number, payout: number) => void | Promise<void>;
    onAddThreshold: (x: number) => void;
    onRemoveThreshold: (x: number) => void;
  }

  const props: Props = $props();

  const mp = $derived(props.all.find((m) => m.mode === props.mode));
  const groups = $derived(mp ? tree(mp) : []);
  const pop = $derived(
    mp?.populations.find((p) => p.key === props.selectedKey) ?? mp?.populations[0],
  );
</script>

{#if !mp || !pop}
  <p class="dim">No data for this mode.</p>
{:else}
  <div class="explore">
    <aside class="tree" aria-label="Populations of {mp.mode}">
      <p class="tree-legend" aria-hidden="true"><span>How often</span><span>median · average</span></p>
      {#each groups as g (g.title)}
        <p class="overline">{g.title}</p>
        <ul>
          {#each g.items as it (it.pop.key)}
            <li>
              <button
                class="node"
                class:active={it.pop.key === pop.key}
                aria-current={it.pop.key === pop.key ? 'true' : undefined}
                style="padding-left: {8 + it.depth * 14}px"
                onclick={() => props.onSelect({ mode: mp.mode, key: it.pop.key })}
              >
                <span class="n-label">
                  {#if props.pinned.has(refId({ mode: mp.mode, key: it.pop.key }))}<i class="pin" title="In Compare" aria-hidden="true">●</i><span class="sr-only">(in Compare)</span>{/if}
                  {it.pop.label}
                </span>
                <span class="n-meta">
                  <span><span class="sr-only">, </span>{it.pop.kind === 'all' ? 'every spin' : oneIn(it.pop.oneIn)}</span>
                  <span title="median · average"
                    ><span class="sr-only">, median </span>{fx(it.pop.median)}<span aria-hidden="true"> · </span><span class="sr-only">, average </span>{fx(it.pop.mean)}</span
                  >
                </span>
              </button>
            </li>
          {/each}
        </ul>
      {/each}
      {#if !mp.imported}
        <p class="note">
          Only the whole mode is available: bonuses, tiers and criteria come from
          the books, which are not imported for {mp.mode} yet (progress in Play).
        </p>
      {/if}
    </aside>

    <PopulationDetail
      {mp}
      {pop}
      all={props.all}
      pinned={props.pinned}
      multiTrigger={props.multiTrigger}
      customThresholds={props.customThresholds}
      onSelect={props.onSelect}
      onTogglePin={props.onTogglePin}
      onPinMany={props.onPinMany}
      onPlay={props.onPlay}
      onBookmark={props.onBookmark}
      onAddThreshold={props.onAddThreshold}
      onRemoveThreshold={props.onRemoveThreshold}
    />
  </div>
{/if}

<style>
  .explore {
    display: grid;
    grid-template-columns: 250px minmax(0, 1fr);
    gap: 26px;
    align-items: start;
  }

  @media (max-width: 900px) {
    .explore {
      grid-template-columns: 1fr;
    }
  }

  .tree {
    position: sticky;
    top: 12px;
    max-height: calc(100vh - 40px);
    overflow-y: auto;
    padding-right: 4px;
  }

  .tree .overline {
    margin: 12px 0 4px;
  }

  .tree-legend {
    display: flex;
    justify-content: space-between;
    gap: 6px;
    margin: 0 0 6px;
    padding: 0 8px 4px;
    border-bottom: 1px solid var(--border);
    font-size: var(--fs-2xs);
    color: var(--text-faint);
  }

  .tree-legend + .overline {
    margin-top: 8px;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .node {
    display: flex;
    flex-direction: column;
    gap: 1px;
    width: 100%;
    text-align: left;
    border: 1px solid transparent;
    border-radius: var(--r-s);
    background: none;
    padding: 5px 8px;
    margin-bottom: 1px;
  }

  .node:hover {
    background: var(--bg-raised);
  }

  .node.active {
    border-color: var(--gold);
    background: color-mix(in oklch, var(--gold) var(--tint-weak), transparent);
  }

  .n-label {
    font-size: var(--fs-sm);
    font-weight: 560;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .n-meta {
    display: flex;
    justify-content: space-between;
    gap: 6px;
    font-family: var(--font-mono);
    font-size: var(--fs-2xs);
    color: var(--text-faint);
  }

  .pin {
    font-style: normal;
    color: var(--gold);
    font-size: var(--fs-2xs);
    margin-right: 3px;
  }

  .note {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    line-height: 1.5;
  }
</style>
