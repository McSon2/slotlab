<script lang="ts">
  import type { Check, GameCheckReport } from '../../lib/api';

  interface Props {
    checks: GameCheckReport;
    selectedMode: string;
  }

  const { checks, selectedMode }: Props = $props();
  const currentChecks = $derived(checks.modes.find((m) => m.mode === selectedMode));
  /** 3★ reached while some constraint misses its 2★ limit. */
  const only3Star = $derived(checks.starTier === 3 && checks.betLevelConstraints.some((c) => !c.pass2star && c.pass3star));

  function statusPill(status: Check['status']): string {
    return status === 'pass' ? 'green' : status === 'warn' ? 'yellow' : status === 'fail' ? 'red' : 'blue';
  }
</script>

{#snippet mark(pass: boolean)}
  <span class="pill {pass ? 'green' : 'red'}">
    <span aria-hidden="true">{pass ? '✓' : '✕'}</span><span class="sr-only">{pass ? 'pass' : 'fail'}</span>
  </span>
{/snippet}

<section class="page">
  <p class="overline">
    Compliance per mode — {selectedMode}
    {#if currentChecks}<span class="faint">({currentChecks.passed}/{currentChecks.total})</span>{/if}
  </p>
  {#if currentChecks}
    <table>
      <thead>
        <tr>
          <th>Check</th>
          <th class="num">Needs</th>
          <th class="num">Value</th>
          <th>Status</th>
        </tr>
      </thead>
      <tbody>
        {#each currentChecks.checks as c (c.id)}
          <tr>
            <td>{c.label}</td>
            <td class="num faint">{c.threshold}</td>
            <td class="num">{c.value}</td>
            <td><span class="pill {statusPill(c.status)}">{c.status}</span></td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}

  <p class="overline section">Overall Bet Level Compliance</p>
  <table class="tier-table">
    <thead>
      <tr>
        <th>Constraint</th>
        <th class="num">Value</th>
        <th class="num">2★ limit</th>
        <th><span class="sr-only">2★ result</span></th>
        <th class="num">3★ limit</th>
        <th><span class="sr-only">3★ result</span></th>
      </tr>
    </thead>
    <tbody>
      {#each checks.betLevelConstraints as c (c.label)}
        <tr>
          <td>{c.label}</td>
          <td class="num">{c.value}</td>
          <td class="num faint">{c.limit2star}</td>
          <td>{@render mark(c.pass2star)}</td>
          <td class="num faint">{c.limit3star}</td>
          <td>{@render mark(c.pass3star)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if only3Star}
    <p class="note">
      The game reaches 3★: a ✕ under 2★ only means it cannot be listed as a 2★
      game — it is not counted as a fail.
    </p>
  {/if}
  <p class="note">
    Tail Probability, CVaR and ETL are a proprietary Stake aggregate not
    replicated by SlotLab — see <em>Tail risk</em> in Overview (indicative) and
    confirm on the dashboard.
  </p>

  {#if checks.publishChecks.length > 0}
    <p class="overline section">Publish checks (RGS)</p>
    <table>
      <thead>
        <tr>
          <th>Check</th>
          <th class="num">Value</th>
          <th>Status</th>
        </tr>
      </thead>
      <tbody>
        {#each checks.publishChecks as c (c.id)}
          <tr>
            <td>{c.label}</td>
            <td class="num">{c.value}</td>
            <td><span class="pill {statusPill(c.status)}">{c.status}</span></td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .tier-table td:first-child {
    color: var(--text);
  }

  .section {
    margin-top: var(--sp-6);
  }

  .note {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    max-width: 780px;
    margin: var(--sp-3) 0 0;
    line-height: 1.55;
  }
</style>
