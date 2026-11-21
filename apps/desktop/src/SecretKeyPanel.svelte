<script lang="ts">
  import { Alert, Button, ConfirmDialog, Icon, SecretKeyField, TextField } from "@figlock/ui";
  import { onDestroy, onMount } from "svelte";

  import {
    errorOf,
    forgetSecretKey,
    keepSecretKey,
    saveEmergencyKitAgain,
    showSecretKey,
    vaultState,
    type VaultState,
  } from "./vault";
  import { failure, getWorkspace } from "./workspace";

  const w = getWorkspace();

  /** How long a shown key stays on screen. */
  const SHOWN_MS = 30_000;

  let info = $state<Pick<
    VaultState,
    | "secretKeyKept"
    | "secretKeyProtection"
    | "secretKeyHeld"
    | "secretKeyOptedOut"
    | "quickUnlock"
    | "touchAvailable"
  > | null>(null);
  let shown = $state("");
  let busy = $state(false);
  let error = $state("");
  let confirming = $state(false);
  /** The Emergency Kit was saved just now. */
  let kitSaved = $state(false);
  /** Asking for the master password and Secret Key: to make the kit when this Mac keeps none, or to keep it again. */
  let typing = $state(false);
  let typingFor = $state<"kit" | "keep">("kit");
  let typedPassword = $state("");
  let typedKey = $state("");
  let typedError = $state<{ password?: string; key?: string }>({});
  let hideTimer: ReturnType<typeof setTimeout> | undefined;

  /** Open a dialog element as a modal as soon as it is in the page. */
  function showModal(node: HTMLDialogElement) {
    node.showModal();
  }

  const where = $derived(
    info?.secretKeyProtection === "secure-enclave"
      ? "in this Mac's Secure Enclave"
      : "in this Mac's keychain",
  );

  /** Why this Mac does not keep the key: no secure store, or the person stopped it (ADR-0027, ADR-0034). */
  const notKeptWhy = $derived(
    info === null || info.secretKeyKept
      ? ""
      : !info.touchAvailable
        ? "This Mac has no secure place to keep it."
        : info.secretKeyOptedOut
          ? "You stopped keeping it here."
          : "It is not kept yet.",
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

  /**
   * Save the Emergency Kit again (FIG-134). With a kept key this asks for Touch ID or the device password and
   * makes the kit from it; without one, from the master password and Secret Key typed here. The person picks
   * where the file goes, and cancelling that writes nothing.
   */
  async function saveKit(typed?: { password: string; key: string }) {
    error = "";
    kitSaved = false;
    busy = true;
    try {
      kitSaved = await saveEmergencyKitAgain(typed?.password, typed?.key);
      if (kitSaved) closeTyping();
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "no_secret_key") {
        typingFor = "kit";
        typing = true;
      } else if (err.code === "bad_secret_key") typedError = { key: err.message };
      else if (typing) error = err.message;
      else error = failure(w, e);
    } finally {
      busy = false;
    }
  }

  function closeTyping() {
    typing = false;
    typedPassword = "";
    typedKey = "";
    typedError = {};
  }

  function submitTyped(e: Event) {
    e.preventDefault();
    typedError = {};
    error = "";
    if (!typedPassword) typedError.password = "Enter your master password.";
    if (typedKey.split("-").length !== 9 || typedKey.split("-").some((g) => g.length === 0)) {
      typedError.key = "This Secret Key looks incomplete. It has eight groups after FL1.";
    }
    if (typedError.password || typedError.key) return;
    if (typingFor === "keep") void keepAgain({ password: typedPassword, key: typedKey });
    else void saveKit({ password: typedPassword, key: typedKey });
  }

  /**
   * Keep the key on this Mac again (FIG-142). An unlocked window already holds the key it unlocked with, so then
   * nothing is typed at all; otherwise the dialog asks for the master password and Secret Key once.
   */
  async function keepAgain(typed?: { password: string; key: string }) {
    error = "";
    busy = true;
    try {
      info = await keepSecretKey(typed?.password ?? "", typed?.key ?? "");
      if (typed) closeTyping();
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "needs_secret_key") {
        typingFor = "keep";
        typing = true;
      } else if (err.code === "bad_secret_key") typedError = { key: err.message };
      else if (typing) error = err.message;
      else error = failure(w, e);
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
            Stored {where}. Showing it asks for Touch ID or your device password. You only type it
            again if another device changes it.
          </p>
        {:else}
          <p class="font-medium">Not kept on this Mac</p>
          <p class="text-(length:--text-sm) text-text-secondary" data-testid="key-why">
            {notKeptWhy}
            {#if info.secretKeyOptedOut || !info.touchAvailable}
              You type it from your Emergency Kit each time you unlock.
            {:else}
              Keep it here and the master password is all you type.
            {/if}
            {#if info.secretKeyHeld}This window holds it until you lock.{/if}
          </p>
        {/if}
      </div>
      {#if info.secretKeyKept}
        <Button onclick={show} disabled={busy}>{busy ? "Waiting..." : "Show Secret Key"}</Button>
      {:else if info.touchAvailable}
        <Button onclick={() => keepAgain()} disabled={busy}>
          {busy ? "Keeping..." : "Keep it on this Mac"}
        </Button>
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

    <div class="flex flex-wrap items-center gap-2">
      <Button
        onclick={() => {
          if (info?.secretKeyKept) void saveKit();
          else {
            typingFor = "kit";
            typing = true;
          }
        }}
        disabled={busy}
      >
        <Icon name="download" />Save Emergency Kit
      </Button>
      {#if info.secretKeyKept}
        <Button onclick={() => (confirming = true)}>Stop keeping it on this Mac</Button>
      {/if}
    </div>
    <p class="text-(length:--text-xs) leading-4 text-text-tertiary" aria-live="polite">
      {#if kitSaved}
        <span data-testid="kit-saved">
          Saved. It carries the same Secret Key as before and today's date.
        </span>
      {:else}
        Lost your Emergency Kit, or never printed it? Save a new copy, made now from this Mac's
        Secret Key.
      {/if}
    </p>
  {/if}

  {#if error && !typing}<Alert>{error}</Alert>{/if}
</section>

{#if typing}
  <dialog
    use:showModal
    aria-labelledby="kit-title"
    oncancel={(e) => {
      e.preventDefault();
      if (!busy) closeTyping();
    }}
    class="m-auto w-[calc(100vw-2rem)] max-w-120 rounded-(--radius-lg) border border-border bg-surface-raised p-0 text-text shadow-xl"
  >
    <form class="flex flex-col gap-4 p-6" onsubmit={submitTyped}>
      <div class="flex flex-col gap-2">
        <h2 id="kit-title" class="text-(length:--text-md) leading-6 font-semibold">
          {typingFor === "keep" ? "Keep your Secret Key on this Mac" : "Save your Emergency Kit"}
        </h2>
        <p class="text-text-secondary">
          {#if typingFor === "keep"}
            Type your Secret Key from your Emergency Kit once. Your master password confirms it is
            the right one. After that this Mac keeps it and the master password is all you type.
          {:else}
            This Mac is not keeping your Secret Key, so type it from your old kit. Your master
            password confirms it is the right one. The kit is made on this Mac and saved only where
            you choose.
          {/if}
        </p>
      </div>
      {#if error}<Alert>{error}</Alert>{/if}
      <TextField
        label="Master password"
        type="password"
        bind:value={typedPassword}
        error={typedError.password}
        disabled={busy}
        focus
      />
      <SecretKeyField bind:value={typedKey} error={typedError.key} disabled={busy} />
      <div class="grid grid-cols-2 gap-2">
        <Button onclick={closeTyping} disabled={busy}>Cancel</Button>
        <Button variant="primary" type="submit" disabled={busy}>
          {#if typingFor === "keep"}
            {busy ? "Keeping..." : "Keep it"}
          {:else}
            {busy ? "Saving..." : "Save kit"}
          {/if}
        </Button>
      </div>
    </form>
  </dialog>
{/if}

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
