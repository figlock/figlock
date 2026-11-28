<script lang="ts">
  import { Alert, Button, ConfirmDialog, Icon } from "@figlock/ui";

  import DeleteDriveDialog from "./DeleteDriveDialog.svelte";
  import { ago, type SyncView } from "./sync";
  import { lock } from "./vault";
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
    if (r.state === "syncing") return "Syncing...";
    if (r.state === "waiting") return "Waiting for iCloud...";
    if (r.state === "unavailable") return "Not available";
    if (r.state === "offline") return "Offline";
    if (PAUSED.includes(r.state)) return "Paused";
    return `Synced ${ago(r.lastSyncSecsAgo)}`;
  }

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
    <h3 id="icloud-title" class="font-semibold">iCloud Drive</h3>
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
            Use your vault on your other Apple devices. Nothing to sign in to.
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
  </section>
{/snippet}

<div class="grid gap-6">
  <section aria-labelledby="sync-title" class="grid gap-3">
    <h3 id="sync-title" class="font-semibold">Google Drive</h3>

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
              Only on this Mac. Turn on to use your vault on other devices.
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
                ? "Linked to a Google account."
                : `Signed in ${ago(view.signedInSecsAgo)}.`}
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
              Removes the copy in your Drive. The vault on this Mac stays.
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
            Two devices changed the vault's settings at once, and one change was kept. Check your
            devices and repeat any that is missing.
          </Alert>
        {/if}
        {#if view.needsUnlock}
          <Alert tone="info">
            Another device changed this vault. Lock and unlock to see everything. A new Secret Key
            comes from the new Emergency Kit.
          </Alert>
          <div><Button onclick={lockToRefresh}><Icon name="lock" />Lock and unlock</Button></div>
        {/if}
        <div>
          <Button onclick={() => (confirming = "off")} disabled={sync.busy}>Turn off sync</Button>
        </div>
      {:else if !view.canSignIn}
        <Alert tone="warning">This build cannot sign in to Google. Use a release build.</Alert>
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
          FigLock only sees its own hidden folder in your Drive. Everything in it is encrypted.
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
      <h3 id="folders-title" class="font-semibold">Folders</h3>
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
          Vault folder: <span class="font-mono break-all">{view.localFolder}</span>
        </p>
      {/if}
    {/if}
  </section>
</div>

{#if confirming === "off"}
  <ConfirmDialog
    title="Turn off sync?"
    body="The vault stays on this Mac and the copy in your Drive stays where it is. Changes here will stop reaching your other devices. This Mac stays signed in to Google. To remove the copy, use Delete from Google Drive on this page."
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
    body="Sync stops and FigLock loses access to your Google account. The vault stays on this Mac and the copy in your Drive stays where it is. To remove it, use Delete from Google Drive on this page first."
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
    body="Sync stops, then you choose another account and this Mac uploads its vault there. If that Drive already holds a different FigLock vault, nothing changes. The copy in the current Drive stays; to remove it, cancel and use Delete from Google Drive on this page first."
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
    body="This puts an encrypted copy of the vault back. Use it only if you know the copy is gone."
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
    body="The vault stays on this Mac and the copy in iCloud Drive stays where it is. Changes here will stop reaching your other devices through iCloud."
    confirm="Turn off iCloud Drive sync"
    onconfirm={() => {
      confirming = null;
      if (target) void sync.disableRemote(target);
    }}
    oncancel={() => (confirming = null)}
  />
{/if}
