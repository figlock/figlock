<script lang="ts">
  import { Button, Icon, TextField } from "@figlock/ui";
  import type { Snippet } from "svelte";

  import Generator from "./Generator.svelte";
  import { passwordStrength, type Strength } from "./generator";
  import StrengthMeter from "./StrengthMeter.svelte";

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
    /** More controls beside Show. */
    extra?: Snippet;
    /** Offer the generator under the field, and show how strong the typed value is. */
    generate?: boolean;
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
    generate = false,
  }: Props = $props();

  let shown = $state(false);
  let generating = $state(false);
  let strength = $state<Strength | null>(null);

  // Rate what is typed, once typing pauses. The rating comes from the app core, same as the generator's.
  $effect(() => {
    if (!generate) return;
    const v = value;
    if (!v) {
      strength = null;
      return;
    }
    const t = setTimeout(() => {
      passwordStrength(v).then(
        (s) => {
          if (v === value) strength = s;
        },
        () => (strength = null),
      );
    }, 150);
    return () => clearTimeout(t);
  });

  function use(v: string) {
    value = v;
    generating = false;
    shown = true;
  }
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
    {#if generate}
      <Button onclick={() => (generating = !generating)} label="Generate a password">
        <Icon name="dices" />Generate
      </Button>
    {/if}
    {@render extra?.()}
  {/snippet}
</TextField>
{#if generate}
  {#if strength}<div class="mt-2"><StrengthMeter {strength} /></div>{/if}
  {#if generating}
    <div class="mt-2 rounded-(--radius-md) border border-border p-4">
      <Generator compact onuse={use} />
    </div>
  {/if}
{/if}
