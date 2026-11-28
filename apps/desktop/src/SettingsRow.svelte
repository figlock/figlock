<script lang="ts">
  import { Icon, type IconName } from "@figlock/ui";
  import type { Snippet } from "svelte";

  type Props = {
    icon: IconName;
    title: string;
    /** One short line under the title. */
    hint?: string;
    /** Plain text at the right, before the chevron. */
    value?: string;
    /** Makes the whole row a button that opens a screen. Without it, `control` sits at the right. */
    onclick?: () => void;
    control?: Snippet;
    testid?: string;
  };
  let { icon, title, hint, value, onclick, control, testid }: Props = $props();
</script>

{#snippet body()}
  <Icon name={icon} class="flex-none text-text-tertiary" />
  <span class="min-w-0 flex-1">
    <span class="block font-medium">{title}</span>
    {#if hint}<span class="block text-(length:--text-sm) text-text-secondary">{hint}</span>{/if}
  </span>
{/snippet}

{#if onclick}
  <button
    type="button"
    {onclick}
    data-testid={testid}
    class="flex min-h-11 w-full items-center gap-3 px-3 py-2 text-left hover:bg-border/50"
  >
    {@render body()}
    {#if value}<span class="text-(length:--text-sm) text-text-secondary">{value}</span>{/if}
    <Icon name="chevron-right" class="flex-none text-text-tertiary" />
  </button>
{:else}
  <div class="flex min-h-11 items-center gap-3 px-3 py-2" data-testid={testid}>
    {@render body()}
    {@render control?.()}
  </div>
{/if}
