<script lang="ts">
  import { Alert, Button, SecretKeyField, TextField } from "@figlock/ui";
  import { onMount } from "svelte";

  import { onSynced } from "./sync";
  import { errorOf, updateSecretKey, vaultState } from "./vault";

  /** Shown while the vault is open when another device made a new Secret Key (ADR-0034). */
  let stale = $state(false);
  let asking = $state(false);
  let dialog = $state<HTMLDialogElement>();
  let password = $state("");
  let secretKey = $state("");
  let busy = $state(false);
  let error = $state("");
  let fieldError = $state<{ password?: string; key?: string }>({});
  const titleId = $props.id();

  async function check() {
    try {
      stale = (await vaultState()).secretKeyStale;
    } catch {
      // The banner is an extra: a window that cannot read its state shows none.
    }
  }

  onMount(() => {
    void check();
    const timer = setInterval(() => void check(), 30_000);
    const off = onSynced(() => void check()).catch(() => () => {});
    return () => {
      clearInterval(timer);
      void off.then((f) => f());
    };
  });

  $effect(() => {
    if (asking) dialog?.showModal();
  });

  function close() {
    asking = false;
    password = "";
    secretKey = "";
    error = "";
    fieldError = {};
  }

  async function submit(e: Event) {
    e.preventDefault();
    error = "";
    fieldError = {};
    if (!password) fieldError.password = "Enter your password.";
    if (secretKey.split("-").length !== 9 || secretKey.split("-").some((g) => g.length === 0)) {
      fieldError.key = "This Secret Key looks incomplete. It has eight groups after FL1.";
    }
    if (fieldError.password || fieldError.key) return;
    busy = true;
    try {
      stale = (await updateSecretKey(password, secretKey)).secretKeyStale;
      close();
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "bad_secret_key") fieldError.key = err.message;
      else error = err.message;
    } finally {
      busy = false;
    }
  }
</script>

{#if stale}
  <div class="border-b border-border bg-surface px-4 py-2" data-testid="secret-key-banner">
    <div class="mx-auto flex max-w-190 flex-wrap items-center gap-3">
      <div class="min-w-0 flex-1">
        <Alert tone="warning">
          Your Secret Key was changed on another device. This window keeps working. Update it here
          to unlock this Mac again.
        </Alert>
      </div>
      <Button onclick={() => (asking = true)}>Update now</Button>
    </div>
  </div>
{/if}

{#if asking}
  <dialog
    bind:this={dialog}
    aria-labelledby={titleId}
    oncancel={(e) => {
      e.preventDefault();
      if (!busy) close();
    }}
    class="m-auto w-[calc(100vw-2rem)] max-w-120 rounded-(--radius-lg) border border-border bg-surface-raised p-0 text-text shadow-xl"
  >
    <form class="flex flex-col gap-4 p-6" onsubmit={submit}>
      <div class="flex flex-col gap-2">
        <h2 id={titleId} class="text-(length:--text-md) leading-6 font-semibold">
          Update your Secret Key
        </h2>
        <p class="text-text-secondary">
          Type the new Secret Key from the Emergency Kit of the device that changed it, or from its
          Show Secret Key. Your password confirms it is the right one.
        </p>
      </div>
      {#if error}<Alert>{error}</Alert>{/if}
      <TextField
        label="Password"
        type="password"
        bind:value={password}
        error={fieldError.password}
        disabled={busy}
        focus
      />
      <SecretKeyField bind:value={secretKey} error={fieldError.key} disabled={busy} />
      <div class="grid grid-cols-2 gap-2">
        <Button onclick={close} disabled={busy}>Cancel</Button>
        <Button variant="primary" type="submit" disabled={busy}>
          {busy ? "Updating..." : "Update"}
        </Button>
      </div>
    </form>
  </dialog>
{/if}
