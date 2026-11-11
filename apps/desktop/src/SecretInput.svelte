<script lang="ts">
  import { Button, Icon, TextField } from "@figlock/ui";
  import type { Snippet } from "svelte";

  type Props = {
    label: string;
    value: string;
    help?: string;
    error?: string;
    maxlength?: number;
    required?: boolean;
    mono?: boolean;
    /** The item already holds a value for this field. Left blank, it is kept. */
    held?: boolean;
    /** The held value will be removed on save. */
    cleared?: boolean;
    /** Remove the held value, or take that back. Leave it out for a field that must have a value. */
    ontoggleclear?: () => void;
    /** More controls beside Show, such as the generator. */
    extra?: Snippet;
  };
  let {
    label,
    value = $bindable(),
    help,
    error,
    maxlength,
    required,
    mono = true,
    held = false,
    cleared = false,
    ontoggleclear,
    extra,
  }: Props = $props();

  let shown = $state(false);
</script>

<TextField
  {label}
  bind:value
  help={cleared ? "Removed when you save." : help}
  {error}
  {maxlength}
  required={required && !held}
  placeholder={held && !cleared ? "Unchanged" : undefined}
  {mono}
  type={shown ? "text" : "password"}
  autocomplete="new-password"
>
  {#snippet trailing()}
    <Button
      onclick={() => (shown = !shown)}
      label="{shown ? 'Hide' : 'Show'} {label.toLowerCase()}"
    >
      <Icon name={shown ? "eye-off" : "eye"} />
      {shown ? "Hide" : "Show"}
    </Button>
    {#if ontoggleclear && (held || cleared) && !value}
      <Button onclick={ontoggleclear}>{cleared ? "Keep" : "Remove"}</Button>
    {/if}
    {@render extra?.()}
  {/snippet}
</TextField>
