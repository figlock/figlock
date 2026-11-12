<script lang="ts">
  import { Alert, Button, Icon } from "@figlock/ui";
  import { onMount } from "svelte";

  import FolderField from "./FolderField.svelte";
  import type { StorageChoice, StorageKind } from "./storage.svelte";

  type Props = {
    storage: StorageChoice;
    /** Under the folder field. */
    folderHelp?: string;
    folderError?: string;
    folderLabel?: string;
  };
  let { storage, folderHelp, folderError, folderLabel }: Props = $props();

  const KINDS: { kind: StorageKind; title: string; text: string }[] = [
    {
      kind: "folder",
      title: "A folder",
      text: "On this Mac, or synced by iCloud Drive or Dropbox.",
    },
    {
      kind: "drive",
      title: "Google Drive",
      text: "Downloads a copy to this Mac and keeps it synced. Nothing else in your Drive is touched.",
    },
  ];

  onMount(() => void storage.refresh());
</script>

{#snippet drivePanel()}
  <div class="flex flex-col gap-2" data-testid="drive-panel">
    {#if storage.drive === null}
      <p class="text-text-secondary">Checking Google Drive...</p>
    {:else if storage.drive.signedIn}
      <div
        class="flex items-center gap-3 rounded-(--radius-md) border border-border bg-surface-sunken p-3"
      >
        <Icon name="shield-check" class="text-success" />
        <span class="flex-1">Signed in to Google Drive.</span>
        <Button onclick={() => storage.signOut()}>Sign out</Button>
      </div>
    {:else if !storage.drive.canSignIn}
      <Alert tone="warning">
        This build of FigLock cannot sign in to Google. Install a release build, or choose a folder.
      </Alert>
    {:else}
      <div class="flex items-center gap-3">
        <Button onclick={() => storage.signIn()} disabled={storage.signingIn}>
          {storage.signingIn ? "Waiting for Google..." : "Sign in with Google"}
        </Button>
        {#if storage.signingIn}
          <span class="text-(length:--text-sm) text-text-secondary">
            Finish signing in in your browser.
          </span>
        {/if}
      </div>
    {/if}
    {#if storage.signInError}<Alert>{storage.signInError}</Alert>{/if}
    {#if storage.notRevoked}
      <Alert tone="warning">
        Signed out on this Mac, but Google could not be reached. To be sure FigLock has no access,
        remove it under Google Account, Security, Third-party access.
      </Alert>
    {/if}
    <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
      FigLock can only see its own hidden app folder, never your other files. Everything in it is
      encrypted.
    </p>
  </div>
{/snippet}

<div class="flex flex-col gap-3">
  {#if storage.mode === "create"}
    <FolderField
      bind:value={storage.folder}
      help={folderHelp ?? "The vault is always kept in a folder on this Mac."}
      error={folderError}
      label={folderLabel}
    />
    <label
      class="flex cursor-pointer items-start gap-3 rounded-(--radius-md) border p-3 {storage.syncDrive
        ? 'border-primary bg-surface-sunken'
        : 'border-border-strong hover:bg-surface-sunken'}"
    >
      <input type="checkbox" bind:checked={storage.syncDrive} class="mt-1" />
      <span class="flex flex-col gap-1">
        <span class="font-medium">Sync with Google Drive</span>
        <span class="text-(length:--text-xs) leading-4 text-text-secondary">
          Keep an encrypted copy in FigLock's own hidden Drive folder, to open the vault on your
          other devices. You can change this later in Settings.
        </span>
      </span>
    </label>
    {#if storage.syncDrive}{@render drivePanel()}{/if}
  {:else}
    <div role="radiogroup" aria-label="Where the vault is kept" class="grid grid-cols-2 gap-2">
      {#each KINDS as k (k.kind)}
        <button
          type="button"
          role="radio"
          aria-checked={storage.kind === k.kind}
          onclick={() => (storage.kind = k.kind)}
          class="flex flex-col gap-1 rounded-(--radius-md) border p-3 text-left {storage.kind ===
          k.kind
            ? 'border-primary bg-surface-sunken'
            : 'border-border-strong hover:bg-surface-sunken'}"
        >
          <span class="font-medium">{k.title}</span>
          <span class="text-(length:--text-xs) leading-4 text-text-secondary">{k.text}</span>
        </button>
      {/each}
    </div>

    {#if storage.kind === "folder"}
      <FolderField
        bind:value={storage.folder}
        help={folderHelp}
        error={folderError}
        label={folderLabel}
      />
    {:else}
      {@render drivePanel()}
    {/if}
  {/if}
</div>
