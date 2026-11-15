<script lang="ts">
  import { Alert, Icon, SelectField } from "@figlock/ui";
  import { onMount } from "svelte";

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

<section aria-labelledby="autolock-title" class="grid gap-3">
  <h3 id="autolock-title" class="font-semibold">Auto-lock</h3>
  <div
    class="flex flex-wrap items-center gap-3 rounded-(--radius-md) border border-border bg-surface p-3"
  >
    <Icon name="lock" class="text-text-tertiary" />
    <div class="min-w-0 flex-1">
      <p class="text-(length:--text-sm) text-text-secondary">
        FigLock locks when you have not used it for this long. It also locks when the Mac sleeps or
        the screen locks, whatever you choose here.
      </p>
    </div>
    <SelectField label="Lock after" bind:value={chosen} {options} />
  </div>
  {#if error}<Alert>{error}</Alert>{/if}
</section>
