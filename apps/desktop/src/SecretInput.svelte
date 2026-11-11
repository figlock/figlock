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
    extra,
  }: Props = $props();

  let shown = $state(false);
</script>

<TextField
  {label}
  bind:value
  {help}
  {error}
  {maxlength}
  {required}
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
    {@render extra?.()}
  {/snippet}
</TextField>
