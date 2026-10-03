<script lang="ts">
  import { onMount } from 'svelte';
  import {
    Chart,
    BarController,
    BarElement,
    LineController,
    LineElement,
    PointElement,
    CategoryScale,
    LinearScale,
    LogarithmicScale,
    Tooltip,
    Legend,
    Filler,
    type ChartConfiguration,
  } from 'chart.js';
  import { chartColors } from '../lib/populations';

  Chart.register(
    BarController,
    BarElement,
    LineController,
    LineElement,
    PointElement,
    CategoryScale,
    LinearScale,
    LogarithmicScale,
    Tooltip,
    Legend,
    Filler,
  );

  Chart.defaults.font.family = "'IBM Plex Mono', ui-monospace, monospace";

  interface Props {
    config: ChartConfiguration;
    height?: number;
    /**
     * What the chart shows, for screen readers (the canvas is an image to
     * them). Defaults to the names of the plotted series.
     */
    label?: string;
  }

  const { config, height = 260, label }: Props = $props();

  const ariaLabel = $derived.by(() => {
    if (label) return label;
    const names = (config.data?.datasets ?? [])
      .map((d) => d.label)
      .filter((l): l is string => !!l);
    return names.length ? `Chart: ${names.join(', ')}` : 'Chart';
  });

  let canvas: HTMLCanvasElement;
  let chart: Chart | null = null;

  /**
   * Chart.js runs `Object.defineProperty` on the data object (tag
   * `_chartjs`). On a Svelte 5 reactive proxy, this throws
   * `state_descriptors_fixed` → empty canvas. So we pass it a plain
   * clone via $state.snapshot (the configs contain no functions).
   */
  function plainConfig(): ChartConfiguration {
    return $state.snapshot(config) as ChartConfiguration;
  }

  onMount(() => {
    // Tokens are resolved once the stylesheet is applied (not at import).
    const c = chartColors();
    Chart.defaults.color = c.text;
    Chart.defaults.borderColor = c.grid;
    chart = new Chart(canvas, plainConfig());
    return () => chart?.destroy();
  });

  $effect(() => {
    const snap = plainConfig();
    if (chart) {
      chart.data = snap.data;
      chart.options = snap.options ?? {};
      chart.update('none');
    }
  });
</script>

<div style="height: {height}px; position: relative; width: 100%;" role="img" aria-label={ariaLabel}>
  <canvas bind:this={canvas}></canvas>
</div>
