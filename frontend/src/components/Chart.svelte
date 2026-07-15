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

  Chart.defaults.color = '#8b9b90';
  Chart.defaults.borderColor = 'rgba(120, 140, 128, 0.15)';
  Chart.defaults.font.family = "'IBM Plex Mono', ui-monospace, monospace";

  interface Props {
    config: ChartConfiguration;
    height?: number;
  }

  const { config, height = 260 }: Props = $props();

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

<div style="height: {height}px; position: relative; width: 100%;">
  <canvas bind:this={canvas}></canvas>
</div>
