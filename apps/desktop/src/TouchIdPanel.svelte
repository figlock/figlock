<script lang="ts">
  import { Alert, Button, ConfirmDialog, Icon, SecretKeyField, TextField } from "@figlock/ui";
  import { onMount } from "svelte";

  import { errorOf, turnTouchIdOff, turnTouchIdOn, vaultState, type VaultState } from "./vault";
  import { failure, getWorkspace } from "./workspace";

  const w = getWorkspace();

  type Props = {
    /** The newest vault state any Settings panel got back, so a change here shows in its sibling (FIG-143). */
    latest?: VaultState | null;
    onstate?: (s: VaultState) => void;
  };
  let { latest = null, onstate }: Props = $props();

  let info = $state<Pick<
    VaultState,
    "quickUnlock" | "touchAvailable" | "secretKeyKept" | "secretKeyHeld"
  > | null>(null);
  let busy = $state(false);
  let error = $state("");
  let confirmingOff = $state(false);
  /** Asking for the password to turn Touch ID on. */
  let asking = $state(false);
  let password = $state("");
  let secretKey = $state("");
  /** The Secret Key field shows only when this Mac neither keeps nor holds one. */
  let askKey = $state(false);
  let fieldError = $state<{ password?: string; key?: string }>({});
  let dialog = $state<HTMLDialogElement>();

  $effect(() => {
    if (asking) dialog?.showModal();
  });

  async function load() {
    try {
      info = await vaultState();
    } catch (e) {
      error = failure(w, e);
    }
  }

  async function turnOff() {
    confirmingOff = false;
    error = "";
    try {
      const next = await turnTouchIdOff();
      info = next;
      onstate?.(next);
    } catch (e) {
      error = failure(w, e);
    }
  }

  function start() {
    error = "";
    fieldError = {};
    askKey = !(info?.secretKeyKept || info?.secretKeyHeld);
    asking = true;
  }

  function close() {
    asking = false;
    password = "";
    secretKey = "";
    fieldError = {};
  }

  async function submit(e: Event) {
    e.preventDefault();
    fieldError = {};
    error = "";
    if (!password) fieldError.password = "Enter your password.";
    if (askKey && (secretKey.split("-").length !== 9 || secretKey.split("-").some((g) => !g))) {
      fieldError.key = "This Secret Key looks incomplete. It has eight groups after FL1.";
    }
    if (fieldError.password || fieldError.key) return;
    busy = true;
    try {
      const next = await turnTouchIdOn(password, askKey ? secretKey : "");
      info = next;
      onstate?.(next);
      close();
    } catch (err) {
      const e2 = errorOf(err);
      if (e2.code === "bad_secret_key") fieldError.key = e2.message;
      else if (e2.code === "needs_secret_key") {
        askKey = true;
        error = e2.message;
      } else error = e2.message;
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    if (latest) info = latest;
  });

  onMount(load);
</script>

{#if info?.touchAvailable}
  <section aria-labelledby="touch-id-title" class="grid gap-3">
    <h3 id="touch-id-title" class="font-semibold">Touch ID</h3>
    <div class="flex items-center gap-3 rounded-(--radius-md) border border-border bg-surface p-3">
      <Icon name="fingerprint" class={info.quickUnlock ? "text-success" : "text-text-tertiary"} />
      <div class="min-w-0 flex-1">
        {#if info.quickUnlock}
          <p class="font-medium">On</p>
          <p class="text-(length:--text-sm) text-text-secondary" data-testid="touch-id-state">
            Open FigLock with your fingerprint.
          </p>
        {:else}
          <p class="font-medium">Off</p>
          <p class="text-(length:--text-sm) text-text-secondary" data-testid="touch-id-state">
            Unlock with your password.
          </p>
        {/if}
      </div>
      {#if info.quickUnlock}
        <Button onclick={() => (confirmingOff = true)}>Turn off Touch ID</Button>
      {:else}
        <Button onclick={start}>Turn on Touch ID</Button>
      {/if}
    </div>
    {#if error && !asking}<Alert>{error}</Alert>{/if}
  </section>
{/if}

{#if asking}
  <dialog
    bind:this={dialog}
    aria-labelledby="touch-id-on-title"
    oncancel={(e) => {
      e.preventDefault();
      if (!busy) close();
    }}
    class="m-auto w-[calc(100vw-2rem)] max-w-120 rounded-(--radius-lg) border border-border bg-surface-raised p-0 text-text shadow-xl"
  >
    <form class="flex flex-col gap-4 p-6" onsubmit={submit}>
      <div class="flex flex-col gap-2">
        <h2 id="touch-id-on-title" class="text-(length:--text-md) leading-6 font-semibold">
          Turn on Touch ID
        </h2>
        <p class="text-text-secondary">
          Enter your password{askKey ? " and Secret Key" : ""} to confirm.
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
      {#if askKey}
        <SecretKeyField bind:value={secretKey} error={fieldError.key} disabled={busy} />
      {/if}
      <div class="grid grid-cols-2 gap-2">
        <Button onclick={close} disabled={busy}>Cancel</Button>
        <Button variant="primary" type="submit" disabled={busy}>
          {busy ? "Turning on..." : "Turn on"}
        </Button>
      </div>
    </form>
  </dialog>
{/if}

{#if confirmingOff}
  <ConfirmDialog
    title="Turn off Touch ID?"
    body="You will unlock with your password only. You can turn Touch ID on again here."
    confirm="Turn off Touch ID"
    onconfirm={turnOff}
    oncancel={() => (confirmingOff = false)}
  />
{/if}
