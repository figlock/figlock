<script lang="ts">
  import { Alert, Button, SecretKeyField, TextField } from "@figlock/ui";
  import { tick } from "svelte";

  import Shell from "./Shell.svelte";
  import { errorOf, quickUnlock, unlock, type VaultState } from "./vault";

  type Props = {
    vault: VaultState;
    notice?: string;
    ondone: (s: VaultState) => void;
    /** Can't unlock: show the ways back in (FIG-139). */
    onforgot: () => void;
  };
  let { vault, notice = "", ondone, onforgot }: Props = $props();

  let password = $state("");
  let secretKey = $state("");
  let shown = $state(false);
  /** The person said their Secret Key changed ("My Secret Key changed"), or the app core asked for it. */
  let keyRefused = $state(false);
  /**
   * The Secret Key field shows when this Mac keeps none, when another device changed it, or when the person
   * asks. A wrong password never shows it (ADR-0034).
   */
  const askKey = $derived(keyRefused || vault.secretKeyStale || !vault.secretKeyKept);
  let busy = $state<"" | "unlocking" | "touch">("");
  let error = $state("");
  let fieldError = $state<{ password?: string; key?: string }>({});
  /** Turned off and on again to put the cursor back in the password box after a refusal. */
  let focusPassword = $state(true);
  async function refocus() {
    focusPassword = false;
    await tick();
    focusPassword = true;
  }

  const minutes = $derived(Math.round(vault.autolockSecs / 60));
  const timeout = $derived(
    minutes === 60 ? "1 hour" : `${minutes} ${minutes === 1 ? "minute" : "minutes"}`,
  );

  async function submit(e: Event) {
    e.preventDefault();
    error = "";
    fieldError = {};
    if (!password) fieldError.password = "Enter your password.";
    if (
      askKey &&
      (secretKey.split("-").length !== 9 || secretKey.split("-").some((g) => g.length === 0))
    ) {
      fieldError.key = "This Secret Key looks incomplete. It has eight groups after FL1.";
    }
    if (fieldError.password || fieldError.key) return;
    busy = "unlocking";
    try {
      const next = await unlock(password, askKey ? secretKey : undefined);
      password = "";
      secretKey = "";
      ondone(next);
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "bad_secret_key") fieldError.key = err.message;
      else if (err.code === "needs_secret_key") {
        keyRefused = true;
        error = err.message;
      } else error = err.message;
      if (err.code !== "bad_secret_key") void refocus();
    } finally {
      busy = "";
    }
  }

  async function touch() {
    error = "";
    fieldError = {};
    busy = "touch";
    try {
      ondone(await quickUnlock());
    } catch (e) {
      error = errorOf(e).message;
    } finally {
      busy = "";
    }
  }
</script>

<Shell title="Unlock your vault" subtitle="Your vault is locked.">
  <form class="flex flex-col gap-4" onsubmit={submit}>
    {#if error}<Alert>{error}</Alert>{:else if notice}<Alert tone="warning">{notice}</Alert>{/if}
    {#if vault.secretKeyStale && vault.secretKeyKept}
      <Alert tone="info">
        Your Secret Key was changed on another device. Type the new one from that device's Emergency
        Kit or Show Secret Key.
      </Alert>
    {/if}
    <TextField
      label="Password"
      type={shown ? "text" : "password"}
      bind:value={password}
      error={fieldError.password}
      disabled={busy !== ""}
      focus={focusPassword}
    >
      {#snippet trailing()}
        <Button onclick={() => (shown = !shown)}>{shown ? "Hide" : "Show"}</Button>
      {/snippet}
    </TextField>
    {#if askKey}
      <SecretKeyField
        bind:value={secretKey}
        error={fieldError.key}
        disabled={busy !== ""}
        help={vault.secretKeyOptedOut
          ? "You stopped keeping it on this Mac, so it is asked for each time. Turn it back on in Settings after you unlock."
          : "It is on your Emergency Kit. You can paste the whole key into any box."}
      />
    {:else}
      <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
        This Mac keeps your Secret Key, so you only need your password.
        <button
          type="button"
          class="underline underline-offset-2 hover:text-text"
          disabled={busy !== ""}
          onclick={() => (keyRefused = true)}
        >
          My Secret Key changed
        </button>
      </p>
    {/if}
    <Button variant="primary" type="submit" block disabled={busy !== ""}>
      {busy === "unlocking" ? "Unlocking..." : "Unlock"}
    </Button>
    {#if vault.quickUnlock}
      <div
        class="flex items-center gap-3 text-(length:--text-xs) text-text-tertiary before:h-px before:flex-1 before:bg-border after:h-px after:flex-1 after:bg-border"
      >
        or
      </div>
      <Button block onclick={touch} disabled={busy !== ""}>
        {busy === "touch" ? "Waiting for Touch ID..." : "Use Touch ID"}
      </Button>
    {/if}
    <button
      type="button"
      class="self-center text-(length:--text-sm) text-text-secondary underline underline-offset-2 hover:text-text"
      disabled={busy !== ""}
      onclick={onforgot}
    >
      I forgot my password or Secret Key
    </button>
  </form>
  <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
    FigLock locks itself after {timeout} without activity.
  </p>
</Shell>
