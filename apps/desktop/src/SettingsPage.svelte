<script lang="ts">
  import type { Snippet } from "svelte";

  /** One screen below the Settings list; the toolbar's Back button returns to the list. */
  type Props = { title: string; children: Snippet };
  let { title, children }: Props = $props();
</script>

<div class="grid gap-6">
  <h2 id="page-title" tabindex="-1" class="text-(length:--text-md) leading-6 font-semibold">
    {title}
  </h2>
  {@render children()}
</div>
