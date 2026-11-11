<script lang="ts">
  import { Button, Icon } from "@figlock/ui";

  import type { Pair } from "./draft";

  type Props = {
    label: string;
    noun: string;
    pairs: Pair[];
    /** The names are variable names: monospace, and uppercased as they are typed. */
    env?: boolean;
    help?: string;
    error?: string;
    onadd: () => void;
    /** A button beside Add, such as Paste .env. */
    onpaste?: () => void;
  };
  let {
    label,
    noun,
    pairs = $bindable(),
    env = false,
    help,
    error,
    onadd,
    onpaste,
  }: Props = $props();

  const labelId = $props.id();
  let shown = $state(new Set<string>());

  const toggle = (id: string) => {
    const next = new Set(shown);
    if (!next.delete(id)) next.add(id);
    shown = next;
  };
</script>

<div>
  <span id={labelId} class="mb-1 block text-(length:--text-sm) leading-5 font-medium">{label}</span>
  <div role="group" aria-labelledby={labelId} class="flex flex-col gap-2">
    {#each pairs as pair, i (pair.id)}
      <div class="grid grid-cols-[minmax(0,1fr)_minmax(0,1.5fr)_auto_auto] items-start gap-2">
        <input
          bind:value={pair.name}
          oninput={() => env && (pair.name = pair.name.toUpperCase())}
          placeholder={env ? "NAME" : "Name"}
          maxlength={env ? 60 : 128}
          aria-label="{noun} {i + 1} name"
          aria-invalid={pair.error ? "true" : undefined}
          autocomplete="off"
          spellcheck="false"
          class="h-(--control-height-md) min-w-0 rounded-(--radius-sm) border bg-surface px-3 text-text placeholder:text-text-tertiary {pair.error
            ? 'border-danger'
            : 'border-border-strong'} {env ? 'font-mono' : ''}"
        />
        <input
          bind:value={pair.value}
          type={shown.has(pair.id) ? "text" : "password"}
          maxlength="65536"
          aria-label="{noun} {i + 1} value"
          autocomplete="new-password"
          spellcheck="false"
          class="h-(--control-height-md) min-w-0 rounded-(--radius-sm) border border-border-strong bg-surface px-3 font-mono text-text"
        />
        <Button onclick={() => toggle(pair.id)}>
          <Icon name={shown.has(pair.id) ? "eye-off" : "eye"} />
          <span class="sr-only"
            >{shown.has(pair.id) ? "Hide" : "Show"} value of {noun.toLowerCase()} {i + 1}</span
          >
        </Button>
        <Button onclick={() => (pairs = pairs.filter((p) => p.id !== pair.id))}>
          <Icon name="x" /><span class="sr-only">Remove {noun.toLowerCase()} {i + 1}</span>
        </Button>
        {#if pair.error}
          <p class="col-span-full text-(length:--text-xs) leading-4 text-danger">{pair.error}</p>
        {/if}
      </div>
    {/each}
  </div>
  <div class="mt-2 flex gap-2">
    <Button onclick={onadd}><Icon name="plus" />Add {noun.toLowerCase()}</Button>
    {#if onpaste}<Button onclick={onpaste}><Icon name="copy" />Paste .env</Button>{/if}
  </div>
  {#if error}
    <p role="alert" class="mt-1 text-(length:--text-xs) leading-4 text-danger">{error}</p>
  {:else if help}
    <p class="mt-1 text-(length:--text-xs) leading-4 text-text-tertiary">{help}</p>
  {/if}
</div>
