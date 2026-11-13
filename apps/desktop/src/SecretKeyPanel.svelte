<script lang="ts">
  import { Alert, Button, ConfirmDialog, Icon } from "@figlock/ui";
  import { onDestroy, onMount } from "svelte";

  import { errorOf, forgetSecretKey, showSecretKey, vaultState, type VaultState } from "./vault";
  import { failure, getWorkspace } from "./workspace";

  const w = getWorkspace();

  /** How long a shown key stays on screen. */
  const SHOWN_MS = 30_000;

  let info = $state<Pick<
    VaultState,
    "secretKeyKept" | "secretKeyProtection" | "secretKeyHeld" | "quickUnlock" | "touchAvailable"
  > | null>(null);
  let shown = $state("");
  let busy = $state(false);
  let error = $state("");
  let confirming = $state(false);
  let hideTimer: ReturnType<typeof setTimeout> | undefined;

  const where = $derived(
    info?.secretKeyProtection === "secure-enclave"
      ? "in this Mac's Secure Enclave"
      : "in this Mac's keychain",
  );

  /** Why this Mac does not keep the key: no secure store, Touch ID off, or the person stopped it (ADR-0027). */
  const notKeptWhy = $derived(
    info === null || info.secretKeyKept
      ? ""
      : !info.touchAvailable
        ? "This Mac has no secure place to keep it."
        : info.quickUnlock
          ? "You stopped keeping it here."
          : "Touch ID is off, and the key is only kept with it.",
  );

  async function load() {
    try {
      info = await vaultState();
    } catch (e) {
      error = failure(w, e);
    }
  }

  function hide() {
    clearTimeout(hideTimer);
    shown = "";
  }

  async function show() {
    error = "";
    busy = true;
    try {
      shown = await showSecretKey();
      clearTimeout(hideTimer);
      hideTimer = setTimeout(hide, SHOWN_MS);
    } catch (e) {
      error = failure(w, e);
    } finally {
      busy = false;
    }
  }

  async function forget() {
    confirming = false;
    error = "";
    hide();
    try {
      info = await forgetSecretKey();
    } catch (e) {
      error = errorOf(e).message;
    }
  }

  onMount(load);
  onDestroy(hide);
</script>

<section aria-labelledby="secret-key-title" class="grid gap-3">
  <h3 id="secret-key-title" class="font-semibold">Secret Key</h3>

  {#if info === null}
    {#if !error}<p class="text-text-secondary">Checking...</p>{/if}
  {:else}
    <div class="flex items-center gap-3 rounded-(--radius-md) border border-border bg-surface p-3">
      <Icon
        name={info.secretKeyKept ? "shield-check" : "key"}
        class={info.secretKeyKept ? "text-success" : "text-text-tertiary"}
      />
      <div class="min-w-0 flex-1">
        {#if info.secretKeyKept}
          <p class="font-medium">Kept on this Mac</p>
          <p class="text-(length:--text-sm) text-text-secondary" data-testid="key-where">
            Stored {where}. Showing it asks for Touch ID or your device password.
          </p>
        {:else}
          <p class="font-medium">Not kept on this Mac</p>
          <p class="text-(length:--text-sm) text-text-secondary" data-testid="key-why">
            {notKeptWhy} You type it from your Emergency Kit each time you unlock.
            {#if info.secretKeyHeld}This window holds it until you lock.{/if}
          </p>
        {/if}
      </div>
      {#if info.secretKeyKept}
        <Button onclick={show} disabled={busy}>{busy ? "Waiting..." : "Show Secret Key"}</Button>
      {/if}
    </div>

    {#if shown}
      <div class="grid gap-2 rounded-(--radius-md) border border-border bg-surface-sunken p-3">
        <p class="font-mono break-all select-all" data-testid="shown-key">{shown}</p>
        <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
          Anyone with this key and your master password can open your vault. It hides itself in 30
          seconds.
        </p>
        <div><Button onclick={hide}>Hide</Button></div>
      </div>
    {/if}

    {#if info.secretKeyKept}
      <div>
        <Button onclick={() => (confirming = true)}>Stop keeping it on this Mac</Button>
      </div>
    {/if}
  {/if}

  {#if error}<Alert>{error}</Alert>{/if}
</section>

{#if confirming}
  <ConfirmDialog
    title="Stop keeping the Secret Key on this Mac?"
    body="From now on you type the Secret Key from your Emergency Kit whenever you unlock with your master password, and this Mac can no longer hand the key to a new device."
    confirm="Stop keeping it"
    danger
    onconfirm={forget}
    oncancel={() => (confirming = false)}
  />
{/if}
