<script lang="ts">
  import { Alert } from "@figlock/ui";
  import { onMount } from "svelte";

  import SettingsRow from "./SettingsRow.svelte";
  import { errorOf, setAutolock, vaultState } from "./vault";

  /** The times Settings offers, in seconds. There is no "never" (FIG-133). */
  const CHOICES = [60, 120, 300, 600, 900, 1800, 3600];
  const label = (secs: number) =>
    secs === 3600 ? "1 hour" : `${secs / 60} ${secs === 60 ? "minute" : "minutes"}`;
  const options = CHOICES.map((s) => ({ value: String(s), label: label(s) }));

  let chosen = $state("300");
  /** What the app core has, so a change is only sent once. */
  let saved = "300";
  let error = $state("");

  onMount(async () => {
    try {
      saved = chosen = String((await vaultState()).autolockSecs);
    } catch (e) {
      error = errorOf(e).message;
    }
  });

  $effect(() => {
    const next = chosen;
    if (next === saved) return;
    error = "";
    setAutolock(Number(next)).then(
      (s) => (saved = String(s.autolockSecs)),
      (e) => {
        error = errorOf(e).message;
        chosen = saved;
      },
    );
  });
</script>

<SettingsRow
  icon="clock"
  title="Lock after"
  hint="Also locks when the Mac sleeps."
  testid="autolock-row"
>
  {#snippet control()}
    <select
      aria-label="Lock after"
      bind:value={chosen}
      class="h-(--control-height-md) rounded-(--radius-sm) border border-border-strong bg-surface px-3 text-text"
    >
      {#each options as o (o.value)}
        <option value={o.value}>{o.label}</option>
      {/each}
    </select>
  {/snippet}
</SettingsRow>
{#if error}<div class="p-3"><Alert>{error}</Alert></div>{/if}
