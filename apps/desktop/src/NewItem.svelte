<script lang="ts">
  import { Icon } from "@figlock/ui";

  import { KINDS } from "./items";
  import type { Filter } from "./nav.svelte";
  import { getWorkspace } from "./workspace";

  type Props = { from: Filter };
  let { from }: Props = $props();

  const { nav } = getWorkspace();
</script>

<div class="flex flex-col gap-1">
  <h1 id="page-title" tabindex="-1" class="text-(length:--text-2xl) leading-7 font-semibold">
    New item
  </h1>
  <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
    Choose what you want to store. You cannot change the type later.
  </p>
</div>
<ul
  class="divide-y divide-border overflow-hidden rounded-(--radius-md) border border-border bg-surface"
>
  {#each KINDS as k (k.kind)}
    <li>
      <button
        type="button"
        onclick={() => nav.go({ p: "edit", kind: k.kind, from })}
        aria-label="{k.label}. {k.desc}"
        class="grid min-h-18 w-full grid-cols-[2.75rem_minmax(0,1fr)_auto] items-center gap-3 py-2 pr-3 pl-4 text-left hover:bg-surface-sunken focus-visible:outline-offset-[-2px]"
      >
        <span
          class="grid size-11 place-items-center rounded-(--radius-md) bg-surface-sunken text-text-secondary"
        >
          <Icon name={k.icon} size={20} />
        </span>
        <span class="grid min-w-0">
          <span class="font-semibold">{k.label}</span>
          <span class="text-(length:--text-sm) text-text-secondary">{k.desc}</span>
        </span>
        <Icon name="chevron-right" class="text-text-tertiary" />
      </button>
    </li>
  {/each}
</ul>
