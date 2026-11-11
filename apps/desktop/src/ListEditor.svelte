<script lang="ts">
  import { Button, Icon } from "@figlock/ui";

  import type { Entry } from "./draft";

  type Props = {
    label: string;
    /** What one entry is called, for the buttons: "Website". */
    noun: string;
    entries: Entry[];
    placeholder: string;
    mono?: boolean;
    help?: string;
    maxlength?: number;
    onadd: () => void;
  };
  let {
    label,
    noun,
    entries = $bindable(),
    placeholder,
    mono = false,
    help,
    maxlength,
    onadd,
  }: Props = $props();

  const labelId = $props.id();
</script>

<div>
  <span id={labelId} class="mb-1 block text-(length:--text-sm) leading-5 font-medium">{label}</span>
  <div role="group" aria-labelledby={labelId} class="flex flex-col gap-2">
    {#each entries as entry, i (entry.id)}
      <div class="flex items-center gap-2">
        <input
          bind:value={entry.value}
          {placeholder}
          {maxlength}
          aria-label="{noun} {i + 1}"
          autocomplete="off"
          spellcheck="false"
          class="h-(--control-height-md) min-w-0 flex-1 rounded-(--radius-sm) border border-border-strong bg-surface px-3 text-text placeholder:text-text-tertiary {mono
            ? 'font-mono'
            : ''}"
        />
        <Button onclick={() => (entries = entries.filter((e) => e.id !== entry.id))}>
          <Icon name="x" /><span class="sr-only">Remove {noun.toLowerCase()} {i + 1}</span>
        </Button>
      </div>
    {/each}
  </div>
  <div class="mt-2">
    <Button onclick={onadd}><Icon name="plus" />Add {noun.toLowerCase()}</Button>
  </div>
  {#if help}<p class="mt-1 text-(length:--text-xs) leading-4 text-text-tertiary">{help}</p>{/if}
</div>
