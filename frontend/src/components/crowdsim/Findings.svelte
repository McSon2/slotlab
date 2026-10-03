<script lang="ts">
  import type { SimFinding, SimSeverity } from '../../lib/crowdsim';

  /** Findings of a cohort: one line each, the detail on click. */
  interface Props {
    lines: SimFinding[];
    limit?: number;
  }

  const { lines, limit }: Props = $props();
  let open = $state<number | null>(null);
  const shown = $derived(limit ? lines.slice(0, limit) : lines);

  /** Spoken severity (the dot's color and shape carry it visually). */
  const SEV_LABEL: Record<SimSeverity, string> = {
    serious: 'Serious',
    minor: 'Minor',
    lived: 'Context',
  };
</script>

<ul class="findings">
  {#each shown as l, i (i)}
    <li class={l.severity}>
      <button class="line" aria-expanded={open === i} onclick={() => (open = open === i ? null : i)}>
        <i class="sev" aria-hidden="true" title={SEV_LABEL[l.severity]}></i>
        <span class="title"><span class="sr-only">{SEV_LABEL[l.severity]}: </span>{l.title}</span>
        <span class="big">{l.big}</span>
        <span class="ref">{l.ref}</span>
      </button>
      {#if open === i}
        <div class="more">
          <p>{l.text}</p>
          {#if l.why}<p class="why">{l.why}</p>{/if}
        </div>
      {/if}
    </li>
  {/each}
</ul>

<style>
  .findings {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    border-top: 1px solid var(--border);
  }

  li:first-child {
    border-top: none;
  }

  .line {
    display: grid;
    grid-template-columns: 10px minmax(0, 1fr) auto;
    grid-template-areas: 'sev title big' '. title ref';
    column-gap: 10px;
    row-gap: 1px;
    align-items: baseline;
    width: 100%;
    padding: 8px 14px;
    text-align: left;
    background: none;
    border: none;
    border-radius: 0;
  }

  .line:hover {
    background: var(--bg-raised);
  }

  /* Severity marker — shape as well as color: serious = filled dot,
     minor = ring, context = small dot. */
  .sev {
    grid-area: sev;
    width: 9px;
    height: 9px;
    border-radius: 50%;
    align-self: center;
    justify-self: center;
    box-sizing: border-box;
  }

  li.serious .sev {
    background: var(--sv-serious);
  }

  li.minor .sev {
    border: 2px solid var(--sv-minor);
  }

  li.lived .sev {
    width: 5px;
    height: 5px;
    background: var(--sv-context);
  }

  .title {
    grid-area: title;
    font-size: var(--fs-base);
    line-height: 1.4;
  }

  li.lived .title {
    color: var(--text-dim);
  }

  .big {
    grid-area: big;
    justify-self: end;
    font-family: var(--font-mono);
    font-size: var(--fs-base);
    font-weight: 650;
    white-space: nowrap;
  }

  li.serious .big { color: var(--sv-serious); }
  li.minor .big { color: var(--sv-minor); }

  .ref {
    grid-area: ref;
    justify-self: end;
    font-size: var(--fs-2xs);
    color: var(--text-faint);
    white-space: nowrap;
  }

  .more {
    padding: 0 14px 10px 34px;
    font-size: var(--fs-sm);
  }

  .more p {
    margin: 0 0 6px;
    line-height: 1.5;
  }

  .why {
    color: var(--text-faint);
  }
</style>
