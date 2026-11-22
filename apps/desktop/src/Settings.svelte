<script lang="ts">
  import { Alert, Button, ConfirmDialog, Icon } from "@figlock/ui";

  import AutoLockPanel from "./AutoLockPanel.svelte";
  import CommandLinePanel from "./CommandLinePanel.svelte";
  import CredentialsPanel from "./CredentialsPanel.svelte";
  import DeleteDriveDialog from "./DeleteDriveDialog.svelte";
  import Devices from "./Devices.svelte";
  import RecoveryPanel from "./RecoveryPanel.svelte";
  import TouchIdPanel from "./TouchIdPanel.svelte";
  import UpdatePanel from "./UpdatePanel.svelte";
  import SecretKeyPanel from "./SecretKeyPanel.svelte";
  import { ago, type SyncView } from "./sync";
  import { lock, type VaultState } from "./vault";
  import { getWorkspace } from "./workspace";

  const { sync, relock } = getWorkspace();
  const view = $derived(sync.view);
  /** Each place the vault syncs to is its own row with its own state (ADR-0043). */
  const drive = $derived(view?.remotes.find((r) => r.kind === "drive") ?? null);
  const icloudRows = $derived(view?.remotes.filter((r) => r.kind === "icloud") ?? []);
  const folders = $derived(view?.remotes.filter((r) => r.kind === "folder") ?? []);
  const driveOn = $derived(drive !== null);

  /** States where nothing happens until the person acts, so "last synced" would mislead. */
  const PAUSED = ["sign_in", "lost", "foreign"];
  /** Only a sync that is working gets the green shield; a paused, offline or failing one must not read as healthy.
   * Waiting for iCloud to deliver files is normal, not a problem. */
  const HEALTHY = ["idle", "syncing", "waiting"];
  /** What the row says it is doing. */
  function statusLine(r: { state: string; lastSyncSecsAgo: number | null }) {
    if (r.state === "syncing") return "Syncing now...";
    if (r.state === "waiting") return "Waiting for iCloud to finish downloading the vault...";
    if (r.state === "unavailable") return "Not available right now";
    if (PAUSED.includes(r.state)) return "Sync is paused";
    return `Last synced ${ago(r.lastSyncSecsAgo)}`;
  }

  /** The newest vault state a panel got back, handed to the others so none shows a stale line. */
  let latest = $state<VaultState | null>(null);
  let confirming = $state<
    "off" | "reupload" | "signout" | "switch" | "delete" | "icloud-off" | null
  >(null);
  /** The place whose "Upload from this Mac" or "Turn off" the person is confirming. */
  let target = $state<string | null>(null);

  async function lockToRefresh() {
    await lock();
    relock("Unlock to see the latest changes from your other devices.");
  }
</script>

{#snippet icloudSection(v: SyncView)}
  {@const row = icloudRows[0] ?? null}
  <section aria-labelledby="icloud-title" class="grid gap-3" data-testid="icloud-section">
    <h3 id="icloud-title" class="font-semibold">iCloud Drive sync</h3>
    <div
      class="flex flex-wrap items-center gap-3 rounded-(--radius-md) border border-border bg-surface p-3"
      data-testid="remote-row"
    >
      {#if row && !HEALTHY.includes(row.state)}
        <Icon name="shield-alert" class="text-warning" />
      {:else if row}
        <Icon name="shield-check" class="text-success" />
      {:else}
        <Icon name="cloud" class="text-text-tertiary" />
      {/if}
      <div class="min-w-0 flex-1">
        {#if row}
          <p class="font-medium">On</p>
          <p class="text-(length:--text-sm) text-text-secondary" data-testid="icloud-status">
            {statusLine(row)}
          </p>
        {:else}
          <p class="font-medium">Off</p>
          <p class="text-(length:--text-sm) text-text-secondary">
            Keep an encrypted copy in the FigLock folder of your iCloud Drive and open the vault on
            your other Apple devices. There is nothing to sign in to.
          </p>
        {/if}
      </div>
      {#if row}
        <Button
          onclick={() => sync.now(false, row.id)}
          disabled={sync.busy || row.state === "syncing"}
        >
          <Icon name="refresh" />Sync now
        </Button>
      {/if}
    </div>

    {#if !row && !v.icloud.ready && v.icloud.help}
      <Alert tone="warning">{v.icloud.help}</Alert>
    {:else if row && row.message && row.state !== "idle" && row.state !== "syncing"}
      <Alert tone={row.state === "waiting" || row.state === "offline" ? "info" : "warning"}>
        {row.message}
      </Alert>
      {#if row.state === "lost"}
        <div>
          <Button
            variant="primary"
            onclick={() => {
              target = row.id;
              confirming = "reupload";
            }}
            disabled={sync.busy}
          >
            <Icon name="upload" />Upload from this Mac
          </Button>
        </div>
      {/if}
    {/if}
    <div>
      {#if row}
        <Button
          onclick={() => {
            target = row.id;
            confirming = "icloud-off";
          }}
          disabled={sync.busy}
        >
          Turn off iCloud Drive sync
        </Button>
      {:else}
        <Button
          variant="primary"
          onclick={() => sync.enableIcloud()}
          disabled={sync.busy || !v.icloud.ready}
        >
          {sync.busy ? "Turning on..." : "Turn on iCloud Drive sync"}
        </Button>
      {/if}
    </div>
    {#if !row && v.icloud.ready && !v.icloud.folderExists}
      <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
        FigLock makes a folder called FigLock in your iCloud Drive. Leave its files alone: they are
        encrypted and iCloud keeps them in step.
      </p>
    {/if}
    {#if row || v.icloud.ready}
      <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
        With Optimize Mac Storage on, iCloud may keep the vault only in the cloud until it is
        needed; FigLock downloads it when it needs it.
      </p>
    {/if}
  </section>
{/snippet}

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
        {#if drive && !HEALTHY.includes(drive.state)}
          <Icon name="shield-alert" class="text-warning" />
        {:else if drive}
          <Icon name="shield-check" class="text-success" />
        {:else}
          <Icon name="lock" class="text-text-tertiary" />
        {/if}
        <div class="min-w-0 flex-1">
          {#if drive}
            <p class="font-medium">On</p>
            <p class="text-(length:--text-sm) text-text-secondary" data-testid="last-sync">
              {statusLine(drive)}
            </p>
          {:else}
            <p class="font-medium">Off</p>
            <p class="text-(length:--text-sm) text-text-secondary">
              This vault is only on this Mac. Turn sync on to keep an encrypted copy in your Google
              Drive and open the vault on your other devices.
            </p>
          {/if}
        </div>
        {#if drive}
          <Button
            onclick={() => sync.now(false, drive.id)}
            disabled={sync.busy || drive.state === "syncing"}
          >
            <Icon name="refresh" />Sync now
          </Button>
        {/if}
      </div>

      {#if view.signedIn}
        <div
          class="flex flex-wrap items-center gap-3 rounded-(--radius-md) border border-border bg-surface p-3"
        >
          <Icon name="globe" class="text-text-tertiary" />
          <div class="min-w-0 flex-1">
            <p class="font-medium">Signed in to Google</p>
            <p class="text-(length:--text-sm) text-text-secondary" data-testid="google-signin">
              {view.signedInSecsAgo === null
                ? "This Mac is linked to a Google account."
                : `Signed in ${ago(view.signedInSecsAgo)}.`}
              FigLock only sees its own hidden app folder, so it cannot show which account.
            </p>
          </div>
          <Button onclick={() => (confirming = "switch")} disabled={sync.busy}>
            Use a different Google account
          </Button>
          <Button onclick={() => (confirming = "signout")} disabled={sync.busy}>
            Sign out of Google
          </Button>
        </div>
      {/if}
      {#if view.signedIn}
        <div
          class="flex flex-wrap items-center gap-3 rounded-(--radius-md) border border-border bg-surface p-3"
        >
          <Icon name="trash" class="text-text-tertiary" />
          <div class="min-w-0 flex-1">
            <p class="font-medium">Delete vault from Google Drive</p>
            <p class="text-(length:--text-sm) text-text-secondary">
              Removes this vault's encrypted copy from your Google Drive. The vault on this Mac is
              kept.
            </p>
          </div>
          <Button onclick={() => (confirming = "delete")} disabled={sync.busy}>
            Delete from Google Drive
          </Button>
        </div>
      {/if}
      {#if sync.notice}<Alert tone="warning">{sync.notice}</Alert>{/if}

      {#if drive}
        {#if drive.state === "offline"}
          <Alert tone="info">{drive.message}</Alert>
        {:else if drive.state === "sign_in"}
          <Alert tone="warning">{drive.message}</Alert>
          <div class="flex flex-wrap items-center gap-3">
            <Button variant="primary" onclick={() => sync.signInAgain()} disabled={sync.busy}>
              {sync.busy ? "Waiting for Google..." : "Sign in with Google"}
            </Button>
            {#if sync.step === "google"}
              <Button onclick={() => sync.cancelSignIn()}>Cancel</Button>
            {/if}
          </div>
        {:else if drive.state === "lost"}
          <Alert tone="warning">{drive.message}</Alert>
          <div>
            <Button
              variant="primary"
              onclick={() => {
                target = drive.id;
                confirming = "reupload";
              }}
              disabled={sync.busy}
            >
              <Icon name="upload" />Upload from this Mac
            </Button>
          </div>
        {:else if drive.state === "foreign"}
          <Alert tone="warning">{drive.message}</Alert>
        {:else if drive.state === "failed"}
          <Alert>{drive.message}</Alert>
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
            everything. If it rotated the Secret Key, you will be asked for the new one from the new
            Emergency Kit.
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
          <Button variant="primary" onclick={() => sync.turnOn()} disabled={sync.busy}>
            {sync.step === "google"
              ? "Waiting for Google..."
              : sync.step === "enabling"
                ? "Turning on..."
                : "Turn on sync"}
          </Button>
          {#if sync.step === "google"}
            <Button onclick={() => sync.cancelSignIn()}>Cancel</Button>
          {/if}
        </div>
        <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
          FigLock can only see its own hidden app folder, never your other files. Everything in it
          is encrypted. This Mac uploads its vault to the Google account you sign in with. If that
          Drive already holds a different FigLock vault, sync is refused and nothing is changed. A
          copy left in an earlier Drive stays there until you remove it in that Google account.
        </p>
      {/if}

      {#if sync.error}<Alert>{sync.error}</Alert>{/if}
    {/if}
  </section>

  {#if view && view.icloud.available}
    {@render icloudSection(view)}
  {/if}

  {#if view && folders.length > 0}
    <section aria-labelledby="folders-title" class="grid gap-3">
      <h3 id="folders-title" class="font-semibold">Other sync folders</h3>
      {#each folders as f (f.id)}
        <div
          class="flex flex-wrap items-center gap-3 rounded-(--radius-md) border border-border bg-surface p-3"
          data-testid="remote-row"
        >
          <Icon name="folder" class="text-text-tertiary" />
          <div class="min-w-0 flex-1">
            <p class="font-medium">{f.name}</p>
            <p class="text-(length:--text-xs) break-all text-text-secondary">{f.path}</p>
            <p class="text-(length:--text-sm) text-text-secondary">{statusLine(f)}</p>
          </div>
          <Button onclick={() => sync.now(false, f.id)} disabled={sync.busy}>
            <Icon name="refresh" />Sync now
          </Button>
          <Button onclick={() => sync.disableRemote(f.id)} disabled={sync.busy}>Turn off</Button>
        </div>
      {/each}
    </section>
  {/if}

  <section aria-labelledby="where-title" class="grid gap-3">
    <h3 id="where-title" class="sr-only">This Mac</h3>
    {#if view}
      {#if view.localFolder}
        <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
          Vault folder on this Mac: <span class="font-mono break-all">{view.localFolder}</span>
        </p>
      {/if}
    {/if}
  </section>

  <TouchIdPanel {latest} onstate={(s) => (latest = s)} />

  <SecretKeyPanel {latest} onstate={(s) => (latest = s)} />

  <AutoLockPanel />

  <CommandLinePanel />

  <UpdatePanel />

  <CredentialsPanel />

  <RecoveryPanel />

  <Devices />
</div>

{#if confirming === "off"}
  <ConfirmDialog
    title="Turn off sync?"
    body="The vault stays on this Mac, and the copy in your Google Drive stays where it is. Changes here will no longer reach your other devices through Google Drive. This Mac stays signed in to Google; to unlink it, use Sign out of Google in Settings. To remove the copy from your Drive, use Delete from Google Drive on this page."
    confirm="Turn off sync"
    onconfirm={() => {
      confirming = null;
      void sync.disable();
    }}
    oncancel={() => (confirming = null)}
  />
{:else if confirming === "signout"}
  <ConfirmDialog
    title="Sign out of Google?"
    body="This Mac will stop syncing and FigLock's access to your Google account is removed. The vault stays on this Mac, and the encrypted copy in your Google Drive stays where it is. You can sign in again any time. To remove that copy, use Delete from Google Drive on this page before you sign out."
    confirm="Sign out of Google"
    danger
    onconfirm={() => {
      confirming = null;
      void sync.signOut();
    }}
    oncancel={() => (confirming = null)}
  />
{:else if confirming === "switch"}
  <ConfirmDialog
    title="Use a different Google account?"
    body="This Mac signs out of Google and sync stops. Then you choose the other account and sync turns on against its Drive: this Mac uploads its vault there. If that Drive already holds a different FigLock vault, sync is refused and nothing is changed. The copy in the current Drive is left alone. To remove it, cancel and use Delete from Google Drive on this page first."
    confirm="Sign out and choose another"
    danger
    onconfirm={() => {
      confirming = null;
      void sync.useDifferentAccount();
    }}
    oncancel={() => (confirming = null)}
  />
{:else if confirming === "delete"}
  <DeleteDriveDialog
    syncOn={view?.enabled ?? false}
    ondelete={async (phrase) => {
      await sync.deleteRemote(phrase);
      confirming = null;
    }}
    oncancel={() => (confirming = null)}
  />
{:else if confirming === "reupload"}
  <ConfirmDialog
    title="Upload this vault again?"
    body="FigLock will put an encrypted copy of this vault back where it was lost. Use this only if you know that copy is gone."
    confirm="Upload again"
    onconfirm={() => {
      confirming = null;
      void sync.now(true, target ?? undefined);
    }}
    oncancel={() => (confirming = null)}
  />
{:else if confirming === "icloud-off"}
  <ConfirmDialog
    title="Turn off iCloud Drive sync?"
    body="The vault stays on this Mac, and the encrypted copy in the FigLock folder of your iCloud Drive stays where it is. Changes here will no longer reach your other devices through iCloud. Google Drive sync, if it is on, is not affected."
    confirm="Turn off iCloud Drive sync"
    onconfirm={() => {
      confirming = null;
      if (target) void sync.disableRemote(target);
    }}
    oncancel={() => (confirming = null)}
  />
{/if}
