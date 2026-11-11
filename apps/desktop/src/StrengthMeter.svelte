<script lang="ts">
  import { Icon, type IconName } from "@figlock/ui";

  import type { Strength } from "./generator";

  type Props = { strength: Strength | null };
  let { strength }: Props = $props();

  const LOOK: Record<string, { text: string; icon: IconName; bars: number; tone: string }> = {
    weak: { text: "Weak", icon: "shield-alert", bars: 1, tone: "text-danger" },
    fair: { text: "Fair", icon: "circle-alert", bars: 2, tone: "text-warning" },
    strong: { text: "Strong", icon: "shield-check", bars: 3, tone: "text-success" },
  };
  const look = $derived(strength ? LOOK[strength.level] : null);
</script>

<div class="flex items-center gap-3 text-(length:--text-sm)" data-testid="strength">
  {#if strength && look}
    <span class="flex gap-1 {look.tone}" aria-hidden="true">
      {#each [1, 2, 3] as n (n)}
        <i class="h-1.5 w-8 rounded-full {n <= look.bars ? 'bg-current' : 'bg-border'}"></i>
      {/each}
    </span>
    <span class="inline-flex items-center gap-1 font-medium {look.tone}">
      <Icon name={look.icon} />{look.text}
    </span>
    <span class="text-text-tertiary">about {strength.bits} bits</span>
  {:else}
    <span class="text-text-tertiary">Add a password to see its strength</span>
  {/if}
</div>
