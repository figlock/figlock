<script lang="ts">
  import { Alert, Button, ConfirmDialog } from "@figlock/ui";

  import Shell from "./Shell.svelte";
  import {
    chooseFolder,
    driveSignIn,
    driveStatus,
    errorOf,
    forgetVault,
    locateFolder,
    restoreFromDrive,
    type MissingFolder,
  } from "./vault";

  type Props = {
    missing: MissingFolder;
    /** The folder is back, or this Mac no longer uses the vault: read the state again. */
    onchanged: () => void;
    /** This Mac no longer uses the vault and the person wants a new one. */
    oncreate: () => void;
  };
  let { missing, onchanged, oncreate }: Props = $props();

  let busy = $state<"" | "locate" | "restore" | "forget">("");
  let error = $state("");
  /** Which step needs the person's yes before this Mac stops using the vault. */
  let confirming = $state<"" | "create" | "remove">("");

  async function run(kind: "locate" | "restore", step: () => Promise<boolean>) {
    error = "";
    busy = kind;
    try {
      if (await step()) onchanged();
    } catch (e) {
      error = errorOf(e).message;
    } finally {
      busy = "";
    }
  }

  const locate = () =>
    run("locate", async () => {
      const folder = await chooseFolder();
      if (!folder) return false;
      await locateFolder(folder);
      return true;
    });

  const restore = () =>
    run("restore", async () => {
      if (!(await driveStatus()).signedIn) await driveSignIn();
      await restoreFromDrive();
      return true;
    });

  async function forget(next: "create" | "remove") {
    confirming = "";
    error = "";
    busy = "forget";
    try {
      await forgetVault(true);
      if (next === "create") oncreate();
      else onchanged();
    } catch (e) {
      error = errorOf(e).message;
    } finally {
      busy = "";
    }
  }
</script>

<Shell
  title="Your vault folder is missing"
  subtitle="FigLock can't find the folder this Mac keeps your vault in."
>
  <div class="flex flex-col gap-4">
    {#if error}<Alert>{error}</Alert>{/if}
    <p
      class="rounded-(--radius-sm) border border-border bg-surface-raised px-3 py-2 font-mono text-(length:--text-xs) break-all text-text-secondary"
    >
      {missing.path}
    </p>
    <p class="text-(length:--text-sm) text-text-secondary">
      It may have been moved, deleted, or sit on a drive that is not connected. Nothing has been
      deleted: your master password is not needed until the vault is found again.
    </p>
    <div class="flex flex-col gap-2">
      <Button variant="primary" block onclick={locate} disabled={busy !== ""}>
        {busy === "locate" ? "Checking the folder..." : "Locate the folder"}
      </Button>
      {#if missing.canRestore}
        <Button block onclick={restore} disabled={busy !== ""}>
          {busy === "restore" ? "Restoring from Google Drive..." : "Restore from Google Drive"}
        </Button>
      {/if}
      <Button block onclick={() => (confirming = "create")} disabled={busy !== ""}>
        Create a new vault
      </Button>
    </div>
    <div class="text-center">
      <button
        type="button"
        onclick={() => (confirming = "remove")}
        disabled={busy !== ""}
        class="text-(length:--text-sm) text-primary underline-offset-2 hover:underline disabled:opacity-50"
      >
        Remove this vault from this Mac
      </button>
    </div>
    <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
      Keep your Emergency Kit safe: its Secret Key, with your master password, is what opens the
      vault again.
    </p>
  </div>
</Shell>

{#if confirming === "create"}
  <ConfirmDialog
    title="Create a new vault?"
    body="This Mac will stop using the old vault. The old vault's data is not deleted: it stays in the folder or Google Drive where it is, and you can open it again with your Emergency Kit."
    confirm="Stop using the old vault"
    danger
    onconfirm={() => forget("create")}
    oncancel={() => (confirming = "")}
  />
{:else if confirming === "remove"}
  <ConfirmDialog
    title="Remove this vault from this Mac?"
    body="This Mac forgets the vault. Nothing is deleted from the folder or Google Drive, and you can open the vault again with your Emergency Kit."
    confirm="Remove from this Mac"
    danger
    onconfirm={() => forget("remove")}
    oncancel={() => (confirming = "")}
  />
{/if}
