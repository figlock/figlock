<script lang="ts">
  import { Alert, Icon, Logo } from "@figlock/ui";
  import { onMount } from "svelte";

  import { ABOUT_LINKS, openAboutPage } from "./about";
  import { errorOf, vaultState } from "./vault";

  let version = $state("");
  let error = $state("");

  onMount(async () => {
    try {
      version = (await vaultState()).version;
    } catch {
      // The version line stays empty rather than showing a wrong one.
    }
  });

  async function open(id: string) {
    error = "";
    try {
      await openAboutPage(id);
    } catch (e) {
      error = errorOf(e).message;
    }
  }
</script>

<div class="grid gap-6">
  <div class="flex flex-col items-center gap-2 pt-4 text-center">
    <Logo size={88} />
    <h2 id="page-title" tabindex="-1" class="text-(length:--text-xl) leading-7 font-semibold">
      FigLock
    </h2>
    {#if version}
      <p class="text-(length:--text-sm) text-text-secondary" data-testid="about-version">
        Version {version}
      </p>
    {/if}
  </div>

  <section aria-labelledby="learn-more" class="grid gap-1.5">
    <h3
      id="learn-more"
      class="px-1 text-(length:--text-xs) leading-4 font-medium text-text-tertiary"
    >
      Learn more
    </h3>
    <div
      class="divide-y divide-border overflow-hidden rounded-(--radius-md) border border-border bg-surface"
    >
      {#each ABOUT_LINKS as link (link.id)}
        <button
          type="button"
          onclick={() => open(link.id)}
          data-testid="about-{link.id}"
          class="flex min-h-11 w-full items-center gap-3 px-3 py-2 text-left font-medium hover:bg-border/50"
        >
          <Icon name={link.icon} class="flex-none text-text-tertiary" />
          <span class="min-w-0 flex-1">{link.title}</span>
          <Icon name="chevron-right" class="flex-none text-text-tertiary" />
        </button>
      {/each}
    </div>
  </section>
  {#if error}<Alert>{error}</Alert>{/if}
</div>
