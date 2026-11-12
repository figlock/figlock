<script lang="ts">
  import { Alert, Button, SecretKeyField, TextField } from "@figlock/ui";

  import type { Changed } from "./credentials";
  import { errorOf } from "./vault";

  type Props = {
    /** What this dialog changes: the master password, or the Secret Key. */
    mode: "password" | "key";
    /** This Mac keeps the Secret Key, so only the master password is asked for. */
    keyKept: boolean;
    /** Resolves when the change is stored; a rejection is shown in the dialog. */
    onchange: (password: string, secretKey: string, newPassword: string | null) => Promise<Changed>;
    oncancel: () => void;
  };

  let { mode, keyKept, onchange, oncancel }: Props = $props();

  const titleId = $props.id();
  let dialog = $state<HTMLDialogElement>();
  let password = $state("");
  let secretKey = $state("");
  let keyRefused = $state(false);
  /** The key field shows when none is kept, or when the kept one was not enough. */
  const askKey = $derived(keyRefused || !keyKept);
  let newPassword = $state("");
  let repeat = $state("");
  let busy = $state(false);
  let error = $state("");
  let fieldError = $state<{ password?: string; key?: string; new?: string; repeat?: string }>({});

  $effect(() => {
    dialog?.showModal();
  });

  async function submit(e: Event) {
    e.preventDefault();
    error = "";
    fieldError = {};
    if (!password) fieldError.password = "Enter your current master password.";
    if (
      askKey &&
      (secretKey.split("-").length !== 9 || secretKey.split("-").some((g) => g.length === 0))
    ) {
      fieldError.key = "This Secret Key looks incomplete. It has eight groups after FL1.";
    }
    if (mode === "password") {
      if (newPassword.length < 8) fieldError.new = "Use at least 8 characters.";
      else if (newPassword !== repeat) fieldError.repeat = "The two passwords do not match.";
    }
    if (Object.keys(fieldError).length > 0) return;
    busy = true;
    try {
      await onchange(password, askKey ? secretKey : "", mode === "password" ? newPassword : null);
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "needs_secret_key") {
        keyRefused = true;
        error = err.message;
      } else if (err.code === "bad_secret_key") fieldError.key = err.message;
      else if (err.code === "weak_password") fieldError.new = err.message;
      else error = err.message;
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
  class="m-auto w-[calc(100vw-2rem)] max-w-120 rounded-(--radius-lg) border border-border bg-surface-raised p-0 text-text shadow-xl backdrop:bg-black/40"
>
  <form class="flex flex-col gap-4 p-6" onsubmit={submit}>
    <div class="flex flex-col gap-2">
      <h2 id={titleId} class="text-(length:--text-md) leading-6 font-semibold">
        {mode === "password" ? "Change master password" : "Rotate Secret Key"}
      </h2>
      <p class="text-text-secondary">
        {#if mode === "password"}
          Your other devices will ask for the new password the next time they unlock. Your Secret
          Key stays the same.
        {:else}
          FigLock makes a new Secret Key and the old one stops working. You then save a new
          Emergency Kit, and your other devices ask for the new key the next time they unlock.
        {/if}
        Enter your current master password{askKey ? " and Secret Key" : ""} to confirm.
      </p>
    </div>
    {#if error}<Alert>{error}</Alert>{/if}
    <TextField
      label="Current master password"
      type="password"
      bind:value={password}
      error={fieldError.password}
      disabled={busy}
      focus
    />
    {#if askKey}
      <SecretKeyField bind:value={secretKey} error={fieldError.key} disabled={busy} />
    {/if}
    {#if mode === "password"}
      <TextField
        label="New master password"
        type="password"
        autocomplete="new-password"
        bind:value={newPassword}
        error={fieldError.new}
        help="Pick something you can remember. FigLock cannot reset it."
        disabled={busy}
      />
      <TextField
        label="Repeat new master password"
        type="password"
        autocomplete="new-password"
        bind:value={repeat}
        error={fieldError.repeat}
        disabled={busy}
      />
    {/if}
    <div class="grid grid-cols-2 gap-2">
      <Button onclick={oncancel} disabled={busy}>Cancel</Button>
      <Button variant="primary" type="submit" disabled={busy}>
        {#if busy}
          Changing...
        {:else}
          {mode === "password" ? "Change password" : "Rotate key"}
        {/if}
      </Button>
    </div>
  </form>
</dialog>
