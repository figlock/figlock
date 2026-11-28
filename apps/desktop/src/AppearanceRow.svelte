<script lang="ts">
  import { APPEARANCES, savedAppearance, setAppearance, type Appearance } from "./appearance";
  import SettingsRow from "./SettingsRow.svelte";

  let chosen = $state<Appearance>(savedAppearance());

  $effect(() => setAppearance(chosen));
</script>

<SettingsRow icon="sun" title="Appearance" testid="appearance-row">
  {#snippet control()}
    <select
      aria-label="Appearance"
      bind:value={chosen}
      class="h-(--control-height-md) rounded-(--radius-sm) border border-border-strong bg-surface px-3 text-text"
    >
      {#each APPEARANCES as a (a.value)}
        <option value={a.value}>{a.label}</option>
      {/each}
    </select>
  {/snippet}
</SettingsRow>
