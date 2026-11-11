<script lang="ts">
  type Props = {
    label: string;
    value: string;
    help?: string;
    error?: string;
    placeholder?: string;
    rows?: number;
    maxlength?: number;
    mono?: boolean;
    /** Show dots instead of the text, for a secret such as a private key. */
    masked?: boolean;
    required?: boolean;
    oninput?: () => void;
  };

  let {
    label,
    value = $bindable(),
    help,
    error,
    placeholder,
    rows = 4,
    maxlength,
    mono = false,
    masked = false,
    required = false,
    oninput,
  }: Props = $props();

  const id = $props.id();
</script>

<div>
  <label for={id} class="mb-1 block text-(length:--text-sm) leading-5 font-medium">
    {label}{#if required}<span class="text-danger" aria-hidden="true"> *</span>{/if}
  </label>
  <textarea
    {id}
    bind:value
    {rows}
    {maxlength}
    {placeholder}
    {oninput}
    spellcheck="false"
    autocomplete="off"
    aria-invalid={error ? "true" : undefined}
    aria-required={required ? "true" : undefined}
    aria-describedby={error ? `${id}-err` : help ? `${id}-help` : undefined}
    class="min-h-24 w-full resize-y rounded-(--radius-sm) border bg-surface px-3 py-2 text-text {error
      ? 'border-danger'
      : 'border-border-strong'} {mono ? 'font-mono' : ''}"
    style:-webkit-text-security={masked ? "disc" : undefined}
  ></textarea>
  {#if error}
    <p id="{id}-err" class="mt-1 text-(length:--text-xs) leading-4 text-danger">{error}</p>
  {:else if help}
    <p id="{id}-help" class="mt-1 text-(length:--text-xs) leading-4 text-text-tertiary">{help}</p>
  {/if}
</div>
