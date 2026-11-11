<script lang="ts">
  import type { Snippet } from "svelte";

  type Props = {
    tone?: "danger" | "warning" | "info";
    children: Snippet;
  };

  let { tone = "danger", children }: Props = $props();

  const looks = {
    danger: "bg-danger-subtle text-text",
    warning: "bg-warning-subtle text-text",
    info: "bg-primary-subtle text-text",
  };
  const mark = { danger: "text-danger", warning: "text-warning", info: "text-primary" };
</script>

<div
  role={tone === "info" ? "status" : "alert"}
  class="flex items-start gap-3 rounded-(--radius-sm) p-3 text-(length:--text-sm) leading-5 {looks[
    tone
  ]}"
>
  <svg
    class="mt-0.5 size-[18px] shrink-0 {mark[tone]}"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <circle cx="12" cy="12" r="10" />
    <path d="M12 8v4M12 16h.01" />
  </svg>
  <span>{@render children()}</span>
</div>
