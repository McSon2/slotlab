<script lang="ts">
  /**
   * Two-step button for destructive actions: the first click arms it (the
   * label turns into a question), the second one — within a few seconds —
   * runs the action. No native confirm(): it stays in the design system and
   * works the same in the browser and in the desktop app.
   */
  import type { Snippet } from 'svelte';

  let {
    onconfirm,
    children,
    confirmLabel = 'Click again to confirm',
    variant = 'ghost',
    disabled = false,
    title,
    ariaLabel,
  }: {
    onconfirm: () => void | Promise<void>;
    children: Snippet;
    confirmLabel?: string;
    variant?: 'ghost' | 'plain' | 'primary';
    disabled?: boolean;
    title?: string;
    ariaLabel?: string;
  } = $props();

  let armed = $state(false);
  let busy = $state(false);
  let timer: ReturnType<typeof setTimeout> | null = null;

  function disarm() {
    armed = false;
    if (timer) clearTimeout(timer);
    timer = null;
  }

  async function click() {
    if (!armed) {
      armed = true;
      timer = setTimeout(disarm, 4000);
      return;
    }
    disarm();
    busy = true;
    try {
      await onconfirm();
    } finally {
      busy = false;
    }
  }

  $effect(() => () => {
    if (timer) clearTimeout(timer);
  });
</script>

<button
  class="confirm-btn {variant === 'plain' ? '' : variant}"
  class:armed
  disabled={disabled || busy}
  {title}
  aria-label={armed ? confirmLabel : ariaLabel}
  onclick={click}
  onblur={disarm}
  onkeydown={(e) => e.key === 'Escape' && disarm()}
>
  {#if armed}{confirmLabel}{:else}{@render children()}{/if}
</button>

<style>
  .confirm-btn.armed,
  .confirm-btn.armed:hover:not(:disabled) {
    color: var(--fail);
    border-color: color-mix(in oklch, var(--fail) 60%, transparent);
    background: color-mix(in oklch, var(--fail) var(--tint), transparent);
  }
</style>
