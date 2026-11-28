<script lang="ts">
  import { Alert, Button, Icon, SecretKeyField, TextField } from "@figlock/ui";
  import { onMount } from "svelte";

  import { errorOf, exportVault, vaultState, type VaultState } from "./vault";
  import { failure, getWorkspace } from "./workspace";

  const w = getWorkspace();

  let info = $state<Pick<VaultState, "secretKeyKept" | "secretKeyHeld" | "quickUnlock"> | null>(
    null,
  );
  let includeSecrets = $state(false);
  let includeDeleted = $state(false);
  let busy = $state(false);
  let error = $state("");
  let saved = $state(false);
  /** Asking for the password (and the Secret Key when this Mac has none) before secrets are exported. */
  let asking = $state(false);
  let password = $state("");
  let secretKey = $state("");
  let askKey = $state(false);
  let fieldError = $state<{ password?: string; key?: string }>({});
  let dialog = $state<HTMLDialogElement>();

  $effect(() => {
    if (asking) dialog?.showModal();
  });

  function close() {
    asking = false;
    password = "";
    secretKey = "";
    fieldError = {};
    dialog?.close();
  }

  /** Export with what is typed, or with none: the backend then asks for Touch ID where this Mac can. */
  async function run(typed?: { password: string; key: string }) {
    error = "";
    saved = false;
    busy = true;
    try {
      saved = await exportVault(
        includeSecrets,
        includeDeleted,
        typed?.password,
        typed?.key || undefined,
      );
      if (typed) close();
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "needs_password" || err.code === "needs_secret_key") {
        askKey = err.code === "needs_secret_key" || !(info?.secretKeyKept || info?.secretKeyHeld);
        asking = true;
        if (typed) error = err.message;
      } else if (err.code === "bad_secret_key") fieldError = { key: err.message };
      else if (asking) error = err.message;
      else error = failure(w, e);
    } finally {
      busy = false;
    }
  }

  function submit(e: Event) {
    e.preventDefault();
    fieldError = {};
    error = "";
    if (!password) fieldError.password = "Enter your password.";
    if (askKey && !secretKey) fieldError.key = "Enter your Secret Key.";
    if (fieldError.password || fieldError.key) return;
    void run({ password, key: secretKey });
  }

  onMount(async () => {
    try {
      info = await vaultState();
    } catch (e) {
      error = failure(w, e);
    }
  });
</script>

<section aria-label="Export" class="grid gap-3">
  <div class="flex items-start gap-3 rounded-(--radius-md) border border-border bg-surface p-3">
    <Icon name="download" class="mt-0.5 text-text-tertiary" />
    <p class="text-text-secondary">
      Save your items as a JSON file you can keep or move to another program. The file is readable
      by anyone who gets it.
    </p>
  </div>

  <label class="flex items-center gap-3 py-1">
    <input type="checkbox" bind:checked={includeDeleted} class="size-4 accent-primary" />
    <span>Include recently deleted items</span>
  </label>
  <label class="flex items-start gap-3 py-1">
    <input
      type="checkbox"
      bind:checked={includeSecrets}
      class="mt-1 size-4 accent-primary"
      data-testid="include-secrets"
    />
    <span>
      Include passwords and other secrets
      <span class="block text-(length:--text-sm) text-text-secondary">
        They are written in plain text. {info?.quickUnlock
          ? "You confirm with Touch ID or your device password first."
          : "You type your password first."}
      </span>
    </span>
  </label>

  {#if includeSecrets}
    <Alert
      >This file will contain every secret in plain text. Keep it somewhere safe and delete it when
      you are done.</Alert
    >
  {/if}

  <div>
    <Button variant="primary" onclick={() => run()} disabled={busy}>
      {busy ? "Waiting..." : "Export..."}
    </Button>
  </div>
  <p class="text-(length:--text-xs) leading-4 text-text-tertiary" aria-live="polite">
    {#if saved}<span data-testid="export-saved">Saved.</span>{/if}
  </p>

  {#if error && !asking}<Alert>{error}</Alert>{/if}
</section>

{#if asking}
  <dialog
    bind:this={dialog}
    aria-labelledby="export-title"
    oncancel={(e) => {
      e.preventDefault();
      if (!busy) close();
    }}
    class="m-auto w-[calc(100vw-2rem)] max-w-120 rounded-(--radius-lg) border border-border bg-surface-raised p-0 text-text shadow-xl"
  >
    <form class="flex flex-col gap-4 p-6" onsubmit={submit}>
      <div class="flex flex-col gap-2">
        <h2 id="export-title" class="text-(length:--text-md) leading-6 font-semibold">
          Export with secrets
        </h2>
        <p class="text-text-secondary">
          Type your password to save the file with every secret in it.
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
          {busy ? "Exporting..." : "Export"}
        </Button>
      </div>
    </form>
  </dialog>
{/if}
