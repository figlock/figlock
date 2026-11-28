<script lang="ts">
  import { Alert, Button, ConfirmDialog, Icon } from "@figlock/ui";

  import { updates } from "./updates.svelte";

  const view = $derived(updates.view);
  const percent = $derived(
    updates.progress?.total
      ? Math.min(100, Math.round((updates.progress.downloaded / updates.progress.total) * 100))
      : null,
  );

  let confirming = $state(false);
</script>

<section aria-label="Updates" class="grid gap-3">
  {#if view?.state === "unavailable"}
    <p class="text-text-secondary" data-testid="update-unavailable">
      This build does not update itself. Use a release build.
    </p>
  {:else}
    <div class="flex items-center gap-3 rounded-(--radius-md) border border-border bg-surface p-3">
      <Icon
        name={view?.state === "available" || view?.state === "ready" ? "download" : "shield-check"}
        class={view?.state === "available" || view?.state === "ready"
          ? "text-primary"
          : "text-text-tertiary"}
      />
      <div class="min-w-0 flex-1">
        {#if updates.installing}
          <p class="font-medium">Downloading FigLock {view?.version}</p>
          <p
            class="text-(length:--text-sm) text-text-secondary"
            role="status"
            data-testid="update-progress"
          >
            {percent === null ? "Downloading..." : `${percent}% downloaded`}
          </p>
        {:else if view?.state === "ready"}
          <p class="font-medium">FigLock {view.version} is installed</p>
          <p class="text-(length:--text-sm) text-text-secondary">
            Restart to use it. Your vault locks on restart.
          </p>
        {:else if view?.state === "available"}
          <p class="font-medium" data-testid="update-available">
            FigLock {view.version} is available
          </p>
          <p class="text-(length:--text-sm) text-text-secondary">
            You have {view.current}.
          </p>
        {:else if view?.state === "up_to_date"}
          <p class="font-medium">FigLock is up to date</p>
          <p class="text-(length:--text-sm) text-text-secondary" data-testid="update-current">
            Version {view.current}.
          </p>
        {:else}
          <p class="font-medium">{updates.checking ? "Checking..." : "Updates"}</p>
          <p class="text-(length:--text-sm) text-text-secondary">Check for a newer version.</p>
        {/if}
      </div>
      {#if view?.state === "ready"}
        <Button variant="primary" onclick={() => (confirming = true)}>Restart FigLock</Button>
      {:else if view?.state === "available"}
        <Button variant="primary" onclick={() => updates.install()} disabled={updates.installing}>
          {updates.installing ? "Installing..." : "Install update"}
        </Button>
      {:else}
        <Button onclick={() => updates.check()} disabled={updates.checking}>
          <Icon name="refresh" />{updates.checking ? "Checking..." : "Check for updates"}
        </Button>
      {/if}
    </div>

    {#if view?.state === "available" && view.notes}
      <div class="grid gap-1 rounded-(--radius-md) border border-border bg-surface-sunken p-3">
        <p class="text-(length:--text-xs) font-medium text-text-secondary">What is new</p>
        <p class="text-(length:--text-sm) whitespace-pre-line" data-testid="update-notes">
          {view.notes}
        </p>
      </div>
    {/if}

    {#if updates.error}<Alert>{updates.error}</Alert>{/if}

    <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
      Updates are signed and checked before they install. Nothing from your vault is sent.
    </p>
  {/if}
</section>

{#if confirming}
  <ConfirmDialog
    title="Restart FigLock?"
    body="FigLock restarts and your vault locks."
    confirm="Restart"
    onconfirm={() => {
      confirming = false;
      void updates.restart();
    }}
    oncancel={() => (confirming = false)}
  />
{/if}
