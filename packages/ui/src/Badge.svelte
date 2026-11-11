<script lang="ts">
  import type { Snippet } from "svelte";

  type Props = { tone?: "neutral" | "warning" | "danger" | "success"; children: Snippet };
  let { tone = "neutral", children }: Props = $props();

  const looks = {
    neutral: "bg-surface-sunken text-text",
    warning: "bg-warning-subtle text-warning",
    danger: "bg-danger-subtle text-danger",
    success: "bg-success-subtle text-success",
  };
</script>

<span
  class="inline-flex h-6 items-center gap-1 rounded-(--radius-xs) px-2 text-(length:--text-xs) font-medium whitespace-nowrap {looks[
    tone
  ]}"
>
  {@render children()}
</span>
