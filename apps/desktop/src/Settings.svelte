<script lang="ts">
  import { Alert, Button, ConfirmDialog, Icon } from "@figlock/ui";

  import { ago } from "./sync";
  import { lock } from "./vault";
  import { getWorkspace } from "./workspace";

  const { sync, relock } = getWorkspace();
  const view = $derived(sync.view);

  let confirming = $state<"off" | "reupload" | null>(null);

  async function lockToRefresh() {
    await lock();
    relock("Unlock to see the latest changes from your other devices.");
  }
</script>

<div class="grid gap-6">
  <div>
    <h2 id="page-title" tabindex="-1" class="text-(length:--text-md) leading-6 font-semibold">
      Settings
    </h2>
    <p class="text-text-secondary">How this vault is kept and synced.</p>
  </div>

  <section aria-labelledby="sync-title" class="grid gap-3">
    <h3 id="sync-title" class="font-semibold">Google Drive sync</h3>

    {#if view === null}
      <p class="text-text-secondary">Checking...</p>
    {:else}
      <div
        class="flex items-center gap-3 rounded-(--radius-md) border border-border bg-surface p-3"
      >
        <Icon
          name={view.enabled ? "shield-check" : "lock"}
          class={view.enabled ? "text-success" : "text-text-tertiary"}
        />
        <div class="min-w-0 flex-1">
          {#if view.enabled}
            <p class="font-medium">On</p>
            <p class="text-(length:--text-sm) text-text-secondary" data-testid="last-sync">
              {view.state === "syncing"
                ? "Syncing now..."
                : `Last synced ${ago(view.lastSyncSecsAgo)}`}
            </p>
          {:else}
            <p class="font-medium">Off</p>
            <p class="text-(length:--text-sm) text-text-secondary">
              This vault is only on this Mac. Turn sync on to keep an encrypted copy in your Google
              Drive and open the vault on your other devices.
            </p>
          {/if}
        </div>
        {#if view.enabled}
          <Button onclick={() => sync.now()} disabled={sync.busy || view.state === "syncing"}>
            <Icon name="refresh" />Sync now
          </Button>
        {/if}
      </div>

      {#if view.enabled}
        {#if view.state === "offline"}
          <Alert tone="info">{view.message}</Alert>
        {:else if view.state === "sign_in"}
          <Alert tone="warning">{view.message}</Alert>
          <div>
            <Button variant="primary" onclick={() => sync.signInAgain()} disabled={sync.busy}>
              {sync.busy ? "Waiting for Google..." : "Sign in with Google"}
            </Button>
          </div>
        {:else if view.state === "lost"}
          <Alert tone="warning">{view.message}</Alert>
          <div>
            <Button
              variant="primary"
              onclick={() => (confirming = "reupload")}
              disabled={sync.busy}
            >
              <Icon name="upload" />Upload from this Mac
            </Button>
          </div>
        {:else if view.state === "foreign"}
          <Alert tone="warning">{view.message}</Alert>
        {:else if view.state === "failed"}
          <Alert>{view.message}</Alert>
        {/if}
        {#if view.fork}
          <Alert tone="warning">
            Two of your devices changed the vault's settings (password, devices or keys) at the same
            time, and FigLock kept one of them. Check your devices list and repeat any change that
            is missing.
          </Alert>
        {/if}
        {#if view.needsUnlock}
          <Alert tone="info">
            Another device changed this vault's password, keys or devices. Lock and unlock to see
            everything.
          </Alert>
          <div><Button onclick={lockToRefresh}><Icon name="lock" />Lock and unlock</Button></div>
        {/if}
        <div>
          <Button onclick={() => (confirming = "off")} disabled={sync.busy}>Turn off sync</Button>
        </div>
      {:else if !view.canSignIn}
        <Alert tone="warning">
          This build of FigLock cannot sign in to Google. Install a release build to use Google
          Drive.
        </Alert>
      {:else}
        <div class="flex flex-wrap items-center gap-3">
          {#if !view.signedIn}
            <Button variant="primary" onclick={() => sync.signInAgain()} disabled={sync.busy}>
              {sync.busy ? "Waiting for Google..." : "Sign in with Google"}
            </Button>
          {:else}
            <Button variant="primary" onclick={() => sync.enable()} disabled={sync.busy}>
              {sync.busy ? "Turning on..." : "Turn on sync"}
            </Button>
          {/if}
        </div>
        <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
          FigLock can only see its own hidden app folder, never your other files. Everything in it
          is encrypted.
        </p>
      {/if}

      {#if sync.error}<Alert>{sync.error}</Alert>{/if}

      {#if view.localFolder}
        <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
          Vault folder on this Mac: <span class="font-mono break-all">{view.localFolder}</span>
        </p>
      {/if}
    {/if}
  </section>
</div>

{#if confirming === "off"}
  <ConfirmDialog
    title="Turn off sync?"
    body="The vault stays on this Mac, and the copy in your Google Drive stays where it is. Changes here will no longer reach your other devices."
    confirm="Turn off sync"
    onconfirm={() => {
      confirming = null;
      void sync.disable();
    }}
    oncancel={() => (confirming = null)}
  />
{:else if confirming === "reupload"}
  <ConfirmDialog
    title="Upload this vault again?"
    body="FigLock will put an encrypted copy of this vault back in your Google Drive. Use this only if you know the Drive copy is gone."
    confirm="Upload again"
    onconfirm={() => {
      confirming = null;
      void sync.now(true);
    }}
    oncancel={() => (confirming = null)}
  />
{/if}
