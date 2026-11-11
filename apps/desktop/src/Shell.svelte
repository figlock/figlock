<script lang="ts">
  import { Logo } from "@figlock/ui";
  import type { Snippet } from "svelte";

  type Props = { title: string; subtitle?: string; children: Snippet };
  let { title, subtitle, children }: Props = $props();
</script>

<main class="grid min-h-screen place-items-center overflow-auto p-6">
  <section
    class="flex w-full max-w-[480px] flex-col gap-4 rounded-(--radius-md) border border-border bg-surface p-8"
  >
    <header class="flex flex-col items-center gap-3 text-center">
      <Logo />
      <div class="flex flex-col gap-1">
        <h1 class="text-(length:--text-xl) leading-7 font-semibold tracking-(--tracking-tight)">
          {title}
        </h1>
        {#if subtitle}
          <p class="text-(length:--text-sm) text-text-secondary">{subtitle}</p>
        {/if}
      </div>
    </header>
    {@render children()}
  </section>
</main>
