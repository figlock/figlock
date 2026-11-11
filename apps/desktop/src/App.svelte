<script lang="ts">
  import { Button } from "@figlock/ui";

  import { vaultState, type VaultState } from "./vault";

  let vault = $state<VaultState | null>(null);
  let failed = $state(false);

  $effect(() => {
    vaultState().then(
      (s) => (vault = s),
      () => (failed = true),
    );
  });
</script>

<main class="flex min-h-screen items-center justify-center p-6">
  <section class="w-full max-w-sm rounded-(--radius-md) border border-border bg-surface p-6">
    <h1 class="text-(length:--text-2xl) font-semibold tracking-(--tracking-tight)">FigLock</h1>
    {#if failed}
      <p class="mt-2 text-text-secondary">
        Can't reach the app core. Restart FigLock and try again.
      </p>
    {:else if vault === null}
      <p class="mt-2 text-text-secondary">Starting...</p>
    {:else}
      <p class="mt-2 text-text-secondary">
        {vault.initialised ? "Your vault is locked." : "No vault on this device yet."}
      </p>
      <div class="mt-4">
        <Button variant="primary" disabled>{vault.initialised ? "Unlock" : "Create a vault"}</Button
        >
      </div>
      <p class="mt-6 text-(length:--text-xs) text-text-tertiary">Version {vault.version}</p>
    {/if}
  </section>
</main>
