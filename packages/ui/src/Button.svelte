<script lang="ts">
  import type { Snippet } from "svelte";

  type Props = {
    variant?: "primary" | "secondary";
    type?: "button" | "submit";
    disabled?: boolean;
    onclick?: (event: MouseEvent) => void;
    children: Snippet;
  };

  let {
    variant = "secondary",
    type = "button",
    disabled = false,
    onclick,
    children,
  }: Props = $props();
</script>

<button
  {type}
  {disabled}
  {onclick}
  class="inline-flex h-(--control-height-md) items-center justify-center rounded-(--radius-sm) px-4 text-(length:--text-sm) font-medium transition-colors duration-(--duration-fast) disabled:opacity-50 {variant ===
  'primary'
    ? 'bg-primary text-text-on-primary hover:bg-primary-hover'
    : 'border border-border-strong bg-surface text-text hover:bg-surface-sunken'}"
>
  {@render children()}
</button>
