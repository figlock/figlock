<script lang="ts">
  import { Alert, Button, SecretKeyField, TextField } from "@figlock/ui";

  import Shell from "./Shell.svelte";
  import { errorOf, quickUnlock, unlock, type VaultState } from "./vault";

  type Props = { vault: VaultState; notice?: string; ondone: (s: VaultState) => void };
  let { vault, notice = "", ondone }: Props = $props();

  let password = $state("");
  let secretKey = $state("");
  let shown = $state(false);
  let busy = $state<"" | "unlocking" | "touch">("");
  let error = $state("");
  let fieldError = $state<{ password?: string; key?: string }>({});

  const minutes = $derived(Math.round(vault.autolockSecs / 60));
  const timeout = $derived(
    minutes === 60 ? "1 hour" : `${minutes} ${minutes === 1 ? "minute" : "minutes"}`,
  );

  async function submit(e: Event) {
    e.preventDefault();
    error = "";
    fieldError = {};
    if (!password) fieldError.password = "Enter your master password.";
    if (secretKey.split("-").length !== 9 || secretKey.split("-").some((g) => g.length === 0)) {
      fieldError.key = "This Secret Key looks incomplete. It has eight groups after FL1.";
    }
    if (fieldError.password || fieldError.key) return;
    busy = "unlocking";
    try {
      const next = await unlock(password, secretKey);
      password = "";
      secretKey = "";
      ondone(next);
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "bad_secret_key") fieldError.key = err.message;
      else error = err.message;
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
    <TextField
      label="Master password"
      type={shown ? "text" : "password"}
      bind:value={password}
      error={fieldError.password}
      disabled={busy !== ""}
      focus
    >
      {#snippet trailing()}
        <Button onclick={() => (shown = !shown)}>{shown ? "Hide" : "Show"}</Button>
      {/snippet}
    </TextField>
    <SecretKeyField
      bind:value={secretKey}
      error={fieldError.key}
      disabled={busy !== ""}
      help="It is on your Emergency Kit. You can paste the whole key into any box."
    />
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
  </form>
  <p class="text-(length:--text-xs) leading-4 text-text-tertiary">
    FigLock locks itself after {timeout} without activity.
  </p>
</Shell>
