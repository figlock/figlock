<script lang="ts">
  import type { Snippet } from "svelte";

  type Props = {
    label: string;
    value: string;
    type?: "text" | "password" | "date";
    help?: string;
    error?: string;
    placeholder?: string;
    maxlength?: number;
    required?: boolean;
    autocomplete?: "off" | "new-password";
    disabled?: boolean;
    readonly?: boolean;
    mono?: boolean;
    /** Focus the field when it appears. */
    focus?: boolean;
    /** A control that sits to the right of the input, such as Show or Choose. */
    trailing?: Snippet;
    oninput?: () => void;
  };

  let {
    label,
    value = $bindable(),
    type = "text",
    help,
    error,
    placeholder,
    maxlength,
    required = false,
    autocomplete = "off",
    disabled = false,
    readonly = false,
    mono = false,
    focus = false,
    trailing,
    oninput,
  }: Props = $props();

  const id = $props.id();
  let input = $state<HTMLInputElement>();

  $effect(() => {
    if (focus) input?.focus();
  });
</script>

<div>
  <label for={id} class="mb-1 block text-(length:--text-sm) leading-5 font-medium">
    {label}{#if required}<span class="text-danger" aria-hidden="true"> *</span>{/if}
  </label>
  <div class="flex items-center gap-2">
    <input
      {id}
      bind:this={input}
      bind:value
      {type}
      {placeholder}
      {maxlength}
      {autocomplete}
      {disabled}
      {readonly}
      {oninput}
      spellcheck="false"
      aria-required={required ? "true" : undefined}
      aria-invalid={error ? "true" : undefined}
      aria-describedby={error ? `${id}-err` : help ? `${id}-help` : undefined}
      class="h-(--control-height-md) w-full min-w-0 rounded-(--radius-sm) border bg-surface px-3 text-text placeholder:text-text-tertiary disabled:opacity-50 {error
        ? 'border-danger'
        : 'border-border-strong'} {mono ? 'font-mono' : ''}"
    />
    {@render trailing?.()}
  </div>
  {#if error}
    <p id="{id}-err" class="mt-1 text-(length:--text-xs) leading-4 text-danger">{error}</p>
  {:else if help}
    <p id="{id}-help" class="mt-1 text-(length:--text-xs) leading-4 text-text-tertiary">{help}</p>
  {/if}
</div>
