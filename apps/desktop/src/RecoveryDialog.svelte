<script lang="ts">
  import { Alert, Button, SecretKeyField, TextField } from "@figlock/ui";

  import RecoverySetup from "./RecoverySetup.svelte";
  import { errorOf, recoveryCancel, recoveryDisable, recoveryEnable } from "./vault";

  type Props = {
    /** Set up (or replace) a Recovery Key, or turn it off. */
    mode: "setup" | "off";
    /** This Mac keeps the Secret Key, so only the master password is asked for. */
    keyKept: boolean;
    /** The vault locked or another device changed it. */
    onerror: (code: string, message: string) => void;
    /** The change is stored. */
    ondone: () => void;
    oncancel: () => void;
  };
  let { mode, keyKept, onerror, ondone, oncancel }: Props = $props();

  const titleId = $props.id();
  let dialog = $state<HTMLDialogElement>();
  /** In setup, the key is shown and confirmed first; then the credentials are asked for. */
  let shown = $state(false);
  const confirmed = $derived(mode === "off" || shown);
  let password = $state("");
  let secretKey = $state("");
  let keyRefused = $state(false);
  const askKey = $derived(keyRefused || !keyKept);
  let busy = $state(false);
  let error = $state("");
  let fieldError = $state<{ password?: string; key?: string }>({});

  $effect(() => {
    dialog?.showModal();
  });

  async function close() {
    if (mode === "setup") await recoveryCancel().catch(() => {});
    oncancel();
  }

  async function submit(e: Event) {
    e.preventDefault();
    error = "";
    fieldError = {};
    if (!password) fieldError.password = "Enter your master password.";
    if (
      askKey &&
      (secretKey.split("-").length !== 9 || secretKey.split("-").some((g) => g === ""))
    ) {
      fieldError.key = "This Secret Key looks incomplete. It has eight groups after FL1.";
    }
    if (Object.keys(fieldError).length > 0) return;
    busy = true;
    try {
      const key = askKey ? secretKey : "";
      if (mode === "setup") await recoveryEnable(password, key);
      else await recoveryDisable(password, key);
      ondone();
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "needs_secret_key") {
        keyRefused = true;
        error = err.message;
      } else if (err.code === "bad_secret_key") fieldError.key = err.message;
      else if (["locked", "needs_unlock", "change_uncertain"].includes(err.code)) {
        onerror(err.code, err.message);
        return;
      } else error = err.message;
      busy = false;
    }
  }
</script>

<dialog
  bind:this={dialog}
  aria-labelledby={titleId}
  oncancel={(e) => {
    e.preventDefault();
    if (!busy) void close();
  }}
  class="m-auto w-[calc(100vw-2rem)] max-w-140 rounded-(--radius-lg) border border-border bg-surface-raised p-0 text-text shadow-xl backdrop:bg-black/40"
>
  <div class="flex flex-col gap-4 p-6">
    <h2 id={titleId} class="text-(length:--text-md) leading-6 font-semibold">
      {mode === "off" ? "Turn off the Recovery Key?" : "Set up a Recovery Key"}
    </h2>
    {#if !confirmed}
      <RecoverySetup
        onconfirmed={() => (shown = true)}
        onback={() => void close()}
        backLabel="Cancel"
        {onerror}
      />
    {:else}
      <form class="flex flex-col gap-4" onsubmit={submit}>
        <p class="text-text-secondary">
          {#if mode === "off"}
            Any code or cards you made stop working. If you forget your master password after this,
            your data cannot be recovered. Enter your master password to confirm{askKey
              ? ", and your Secret Key"
              : ""}.
          {:else}
            Enter your master password{askKey ? " and your Secret Key" : ""} to add it to the vault. A
            Recovery Key you made before stops working.
          {/if}
        </p>
        {#if error}<Alert>{error}</Alert>{/if}
        <TextField
          label="Master password"
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
          <Button onclick={() => void close()} disabled={busy}>Cancel</Button>
          {#if mode === "off"}
            <button
              type="submit"
              disabled={busy}
              class="inline-flex h-(--control-height-md) items-center justify-center rounded-(--radius-sm) bg-danger px-4 text-(length:--text-sm) font-medium text-text-on-primary transition-colors duration-(--duration-fast) hover:brightness-90 disabled:opacity-60"
            >
              {busy ? "Turning off..." : "Turn off"}
            </button>
          {:else}
            <Button variant="primary" type="submit" disabled={busy}>
              {busy ? "Saving..." : "Save Recovery Key"}
            </Button>
          {/if}
        </div>
      </form>
    {/if}
  </div>
</dialog>
