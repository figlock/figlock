<script lang="ts">
  import type { Snippet } from "svelte";

  type Props = { title: string; children: Snippet };
  let { title, children }: Props = $props();
  const id = $props.id();
</script>

<section aria-labelledby={id} class="grid gap-1.5">
  <h3 {id} class="px-1 text-(length:--text-xs) leading-4 font-medium text-text-tertiary">
    {title}
  </h3>
  <div
    class="divide-y divide-border overflow-hidden rounded-(--radius-md) border border-border bg-surface"
  >
    {@render children()}
  </div>
</section>
