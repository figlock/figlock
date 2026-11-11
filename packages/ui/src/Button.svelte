<script lang="ts">
  import type { Snippet } from "svelte";

  type Props = {
    variant?: "primary" | "secondary" | "ghost";
    type?: "button" | "submit";
    disabled?: boolean;
    /** Fill the width of the container. */
    block?: boolean;
    /** The name a screen reader announces, when the visible text alone is not enough. */
    label?: string;
    onclick?: (event: MouseEvent) => void;
    children: Snippet;
  };

  let {
    variant = "secondary",
    type = "button",
    disabled = false,
    block = false,
    label,
    onclick,
    children,
  }: Props = $props();

  const looks = {
    primary: "bg-primary text-text-on-primary hover:bg-primary-hover",
    secondary: "border border-border-strong bg-surface text-text hover:bg-surface-sunken",
    ghost: "text-text hover:bg-surface-sunken",
  };
</script>

<button
  {type}
  {disabled}
  aria-label={label}
  {onclick}
  class="inline-flex h-(--control-height-md) items-center justify-center gap-2 rounded-(--radius-sm) px-4 text-(length:--text-sm) font-medium transition-colors duration-(--duration-fast) disabled:opacity-50 {block
    ? 'w-full'
    : ''} {looks[variant]}"
>
  {@render children()}
</button>
