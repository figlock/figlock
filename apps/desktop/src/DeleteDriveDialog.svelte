<script lang="ts">
  import { Alert, Button, TextField } from "@figlock/ui";

  import { DELETE_DRIVE_PHRASE } from "./sync";
  import { errorOf } from "./vault";

  type Props = {
    /** Sync is on for this Mac, and will be turned off by the deletion. */
    syncOn: boolean;
    /** Resolves when the copy is deleted; a rejection is shown in the dialog. */
    ondelete: (confirmation: string) => Promise<void>;
    oncancel: () => void;
  };
  let { syncOn, ondelete, oncancel }: Props = $props();

  const titleId = $props.id();
  let dialog = $state<HTMLDialogElement>();
  let typed = $state("");
  let busy = $state(false);
  let error = $state("");

  /** The button stays off until the phrase is right, so a click is never the whole confirmation. */
  const ready = $derived(typed.trim().toLowerCase() === DELETE_DRIVE_PHRASE.toLowerCase());

  $effect(() => {
    dialog?.showModal();
  });

  async function submit(e: Event) {
    e.preventDefault();
    if (!ready || busy) return;
    error = "";
    busy = true;
    try {
      await ondelete(typed);
    } catch (e) {
      error = errorOf(e).message;
      busy = false;
    }
  }
</script>

<dialog
  bind:this={dialog}
  aria-labelledby={titleId}
  oncancel={(e) => {
    e.preventDefault();
    if (!busy) oncancel();
  }}
  class="m-auto w-[calc(100vw-2rem)] max-w-120 rounded-(--radius-lg) border border-border bg-surface-raised p-0 text-text shadow-xl"
>
  <form class="flex flex-col gap-4 p-6" onsubmit={submit}>
    <div class="flex flex-col gap-2">
      <h2 id={titleId} class="text-(length:--text-md) leading-6 font-semibold">
        Delete this vault from Google Drive?
      </h2>
      <p class="text-text-secondary">
        This permanently deletes every FigLock file of this vault from your Google Drive: the vault
        settings, device records, change history and snapshots. It cannot be undone.
      </p>
      <ul class="list-disc pl-5 text-text-secondary">
        <li>The vault on this Mac stays exactly as it is.</li>
        <li>
          {syncOn ? "Sync is turned off on this Mac." : "Sync stays off on this Mac."}
          Turn it on again to upload the vault anew.
        </li>
        <li>
          Your other devices that sync to Drive will find the vault missing and ask before they
          upload it again.
        </li>
        <li>Your Google account and its other files are not touched.</li>
      </ul>
    </div>
    {#if error}<Alert>{error}</Alert>{/if}
    <TextField
      label={`Type "${DELETE_DRIVE_PHRASE}" to confirm`}
      bind:value={typed}
      disabled={busy}
      focus
    />
    <div class="grid grid-cols-2 gap-2">
      <Button onclick={oncancel} disabled={busy}>Cancel</Button>
      <button
        type="submit"
        disabled={!ready || busy}
        class="inline-flex h-(--control-height-md) items-center justify-center rounded-(--radius-sm) bg-danger px-4 text-(length:--text-sm) font-medium text-text-on-primary transition-colors duration-(--duration-fast) hover:brightness-90 disabled:opacity-60"
      >
        {busy ? "Deleting..." : "Delete from Google Drive"}
      </button>
    </div>
  </form>
</dialog>
