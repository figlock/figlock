<script lang="ts">
  import { Alert, Button, ConfirmDialog } from "@figlock/ui";

  import Shell from "./Shell.svelte";
  import { fmtDay } from "./util";
  import { errorOf, leaveVault } from "./vault";

  type Props = {
    /** When the vault removed this Mac, in Unix milliseconds. */
    removedAtMs: number;
    /** Keep the unlocked copy open to read; every change stays refused. */
    onread: () => void;
    /** This Mac no longer uses the vault: read the state again. */
    onleft: () => void;
    /** This Mac no longer uses the vault and the person wants to join again with an invite code. */
    onjoin: () => void;
  };
  let { removedAtMs, onread, onleft, onjoin }: Props = $props();

  let confirming = $state<"" | "remove" | "join">("");
  let busy = $state(false);
  let error = $state("");

  const date = $derived(fmtDay(removedAtMs));

  async function leave(next: "remove" | "join") {
    confirming = "";
    error = "";
    busy = true;
    try {
      await leaveVault();
      if (next === "join") onjoin();
      else onleft();
    } catch (e) {
      error = errorOf(e).message;
    } finally {
      busy = false;
    }
  }
</script>

<Shell
  title="This Mac was removed from the vault"
  subtitle={`Removed on ${date}. Changes made here are no longer accepted.`}
>
  <div class="flex flex-col gap-4">
    {#if error}<Alert>{error}</Alert>{/if}
    <Alert tone="warning">
      Anything you add or change on this Mac is refused, so nothing is lost silently. Your copy
      stays readable.
    </Alert>
    <p class="text-(length:--text-sm) text-text-secondary">
      Another of your devices removed this Mac. If that was a mistake, add this Mac again with a new
      invite code from a device that still has the vault. Otherwise you can remove the vault from
      this Mac.
    </p>
    <div class="flex flex-col gap-2">
      <Button variant="primary" block onclick={() => (confirming = "join")} disabled={busy}>
        Add this Mac again with an invite code
      </Button>
      <Button block onclick={onread} disabled={busy}>Keep reading my copy</Button>
    </div>
    <div class="text-center">
      <button
        type="button"
        onclick={() => (confirming = "remove")}
        disabled={busy}
        class="text-(length:--text-sm) text-primary underline-offset-2 hover:underline disabled:opacity-50"
      >
        Remove this vault from this Mac
      </button>
    </div>
  </div>
</Shell>

{#if confirming === "remove"}
  <ConfirmDialog
    title="Remove this vault from this Mac?"
    body="This Mac forgets the vault, its keys and its Secret Key. Nothing is deleted from the vault folder or Google Drive. To open the vault here again, add this Mac with a new invite code."
    confirm="Remove from this Mac"
    danger
    onconfirm={() => leave("remove")}
    oncancel={() => (confirming = "")}
  />
{:else if confirming === "join"}
  <ConfirmDialog
    title="Add this Mac again?"
    body="This Mac first forgets the vault it has, then asks for a new invite code from a device that still has the vault. Nothing is deleted from the vault folder or Google Drive."
    confirm="Continue"
    onconfirm={() => leave("join")}
    oncancel={() => (confirming = "")}
  />
{/if}
