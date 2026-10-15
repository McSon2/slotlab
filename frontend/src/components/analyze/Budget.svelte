<script lang="ts">
  import { fx, pct, oneIn, type ModePopulations, type Population } from '../../lib/populations';
  import { fmtPct } from '../../lib/api';

  /**
   * The RTP budget of a mode: who carries the points — plain spins (by
   * family), each bonus tier — and what moving them means. The questions
   * of every tuning session (« si je réduis le RTP du hidden de 17 à 12 % en
   * touchant juste sa fréquence, ça donne quoi ? », TOXIC SPINS 2026-10-03)
   * are arithmetic: points = frequency × average ÷ cost. Type a target and
   * read the frequency at the same average, or the average at the same
   * frequency.
   */
  interface Props {
    mp: ModePopulations | undefined;
    rtp: number;
  }

  const { mp, rtp }: Props = $props();

  interface Row {
    pop: Population;
    label: string;
    depth: number;
  }

  const rows = $derived.by((): Row[] => {
    if (!mp) return [];
    const pops = mp.populations;
    const out: Row[] = [];
    const nobonus = pops.find((p) => p.kind === 'nobonus');
    const fams = pops.filter((p) => p.kind === 'criteria');
    if (nobonus) {
      out.push({ pop: nobonus, label: 'Plain spins', depth: 0 });
      for (const f of fams) out.push({ pop: f, label: f.label, depth: 1 });
    } else if (fams.length) {
      for (const f of fams) out.push({ pop: f, label: f.label, depth: 0 });
    }
    const bonus = pops.find((p) => p.kind === 'bonus');
    const tiers = pops.filter((p) => p.kind === 'tier');
    if (bonus) out.push({ pop: bonus, label: 'Bonuses', depth: 0 });
    for (const t of tiers) out.push({ pop: t, label: t.label, depth: bonus ? 1 : 0 });
    return out;
  });

  /** Target points per population key (typed by the reader), in RTP points. */
  let targets = $state<Record<string, string>>({});

  const pts = (p: Population) => p.rtpContribution * 100;

  function implied(p: Population): { freq: number; mean: number } | null {
    const raw = targets[p.key];
    const t = Number(raw);
    if (!raw || !Number.isFinite(t) || t <= 0 || p.mean <= 0 || p.probability <= 0) return null;
    const ratio = t / pts(p);
    return { freq: p.oneIn / ratio, mean: p.mean * ratio };
  }

  const cost = $derived(mp && mp.cost > 0 ? mp.cost : 1);
  const total = $derived(rows.filter((r) => r.depth === 0).reduce((a, r) => a + pts(r.pop), 0));
</script>

{#if mp && rows.length > 1}
  <section class="budget">
    <p class="overline">RTP budget <span class="faint">— {mp.mode}: points = frequency × average ÷ cost</span></p>
    <div class="wrap">
      <table>
        <thead>
          <tr>
            <th>Carried by</th>
            <th class="num">Frequency</th>
            <th class="num">Average</th>
            <th class="num">Median</th>
            <th class="num">RTP points</th>
            <th class="num">Share</th>
            <th class="num what">Target points</th>
            <th class="num what">→ frequency, same average</th>
            <th class="num what">→ average, same frequency</th>
          </tr>
        </thead>
        <tbody>
          {#each rows as r (r.pop.key)}
            {@const w = implied(r.pop)}
            <tr class:sub={r.depth > 0}>
              <td style="padding-left: {10 + r.depth * 16}px">{r.label}</td>
              <td class="num">{r.pop.kind === 'all' ? 'every spin' : oneIn(r.pop.oneIn)}</td>
              <td class="num">{fx(r.pop.mean)}</td>
              <td class="num faint">{fx(r.pop.median)}</td>
              <td class="num"><b>{pts(r.pop).toFixed(2)}</b></td>
              <td class="num faint">{pct(r.pop.rtpShare)}</td>
              <td class="num what">
                <input
                  type="number"
                  step="0.1"
                  min="0"
                  placeholder={pts(r.pop).toFixed(1)}
                  aria-label="Target RTP points for {r.label}"
                  bind:value={targets[r.pop.key]}
                />
              </td>
              <td class="num what">{w ? oneIn(w.freq) : '—'}</td>
              <td class="num what">{w ? fx(w.mean) : '—'}</td>
            </tr>
          {/each}
        </tbody>
        <tfoot>
          <tr>
            <td>Mode</td>
            <td class="num faint">cost {fx(cost)}</td>
            <td></td>
            <td></td>
            <td class="num"><b>{total.toFixed(2)}</b> <span class="faint">of {fmtPct(rtp, 2)}</span></td>
            <td class="num faint">100 %</td>
            <td colspan="3"></td>
          </tr>
        </tfoot>
      </table>
    </div>
    <p class="note">
      Points taken from one row must land on another: the mode stays at {fmtPct(rtp, 2)}. A frequency moves the
      base game only by its own books; an average moves with the lookup table alone.
    </p>
  </section>
{/if}

<style>
  .budget {
    margin-top: 24px;
  }

  .wrap {
    overflow-x: auto;
  }

  tr.sub td {
    color: var(--text-dim);
    font-size: var(--fs-xs);
  }

  td.what,
  th.what {
    background: color-mix(in oklch, var(--gold) var(--tint-weak), transparent);
  }

  input {
    width: 72px;
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    text-align: right;
  }

  tfoot td {
    border-top: 1px solid var(--border-strong);
    font-weight: 600;
  }

  .note {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    max-width: 820px;
    margin: 8px 0 0;
    line-height: 1.55;
  }
</style>
