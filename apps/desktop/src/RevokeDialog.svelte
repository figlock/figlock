<script lang="ts">
  import { Alert, Button, SecretKeyField, TextField } from "@figlock/ui";

  import type { Device } from "./devices";
  import { errorOf } from "./vault";

  type Props = {
    device: Device;
    /** Resolves when the device is revoked; a rejection is shown in the dialog. */
    onrevoke: (password: string, secretKey: string, newPassword: string | null) => Promise<void>;
    oncancel: () => void;
  };

  let { device, onrevoke, oncancel }: Props = $props();

  const titleId = $props.id();
  let dialog = $state<HTMLDialogElement>();
  let password = $state("");
  let secretKey = $state("");
  let compromised = $state(false);
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
    if (!password) fieldError.password = "Enter your master password.";
    if (secretKey.split("-").length !== 9 || secretKey.split("-").some((g) => g.length === 0)) {
      fieldError.key = "This Secret Key looks incomplete. It has eight groups after FL1.";
    }
    if (compromised) {
      if (newPassword.length < 8) fieldError.new = "Use at least 8 characters.";
      else if (newPassword !== repeat) fieldError.repeat = "The two passwords do not match.";
    }
    if (Object.keys(fieldError).length > 0) return;
    busy = true;
    try {
      await onrevoke(password, secretKey, compromised ? newPassword : null);
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "bad_secret_key") fieldError.key = err.message;
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
        Revoke {device.name ?? "this device"}?
      </h2>
      <p class="text-text-secondary">
        It can no longer open anything written from now on. A copy of the vault it already has stays
        readable, and so does anything it copied out. Enter your master password and Secret Key to
        confirm.
      </p>
    </div>
    {#if error}<Alert>{error}</Alert>{/if}
    <TextField
      label="Master password"
      type="password"
      bind:value={password}
      error={fieldError.password}
      disabled={busy}
      focus
    />
    <SecretKeyField bind:value={secretKey} error={fieldError.key} disabled={busy} />
    <label class="flex items-start gap-2">
      <input type="checkbox" bind:checked={compromised} disabled={busy} class="mt-1" />
      <span>
        This device may have been lost or compromised. Also set a new master password.
        <span class="block text-(length:--text-xs) leading-4 text-text-tertiary">
          The revoked device may still know your current password and Secret Key.
        </span>
      </span>
    </label>
    {#if compromised}
      <TextField
        label="New master password"
        type="password"
        autocomplete="new-password"
        bind:value={newPassword}
        error={fieldError.new}
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
      <button
        type="submit"
        disabled={busy}
        class="inline-flex h-(--control-height-md) items-center justify-center rounded-(--radius-sm) bg-danger px-4 text-(length:--text-sm) font-medium text-text-on-primary transition-colors duration-(--duration-fast) hover:brightness-90 disabled:opacity-60"
      >
        {busy ? "Revoking..." : "Revoke device"}
      </button>
    </div>
  </form>
</dialog>
