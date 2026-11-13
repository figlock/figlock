<script lang="ts">
  import { Alert, Badge, Button, Icon } from "@figlock/ui";
  import { onMount } from "svelte";

  import RecoveryDialog from "./RecoveryDialog.svelte";
  import { errorOf, lock, vaultState } from "./vault";
  import { getWorkspace } from "./workspace";

  const w = getWorkspace();

  let enabled = $state<boolean | null>(null);
  let keyKept = $state(false);
  let dialog = $state<"setup" | "off" | null>(null);
  let done = $state("");
  let error = $state("");

  async function load() {
    try {
      const s = await vaultState();
      enabled = s.recoveryEnabled;
      keyKept = s.secretKeyKept;
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "locked") w.relock();
      else error = err.message;
    }
  }

  onMount(() => void load());

  async function start(mode: "setup" | "off") {
    done = "";
    error = "";
    await load();
    dialog = mode;
  }

  async function failed(code: string, message: string) {
    dialog = null;
    if (code === "locked") w.relock();
    else if (code === "needs_unlock" || code === "change_uncertain") {
      await lock();
      w.relock(message);
    } else error = message;
  }

  async function finished() {
    const was = dialog;
    dialog = null;
    await load();
    done =
      was === "off"
        ? "The Recovery Key is turned off."
        : "The Recovery Key is set up. Keep it apart from your master password.";
  }
</script>

<section aria-labelledby="recovery-title" class="grid gap-3">
  <h3 id="recovery-title" class="font-semibold">Recovery Key</h3>
  <p class="text-text-secondary">
    A code, or cards for people you trust, that lets you set a new master password if you forget it.
  </p>
  {#if error}<Alert>{error}</Alert>{/if}
  {#if done}<Alert tone="info">{done}</Alert>{/if}
  <div class="flex items-center gap-3 rounded-(--radius-md) border border-border bg-surface p-3">
    <Icon
      name={enabled ? "shield-check" : "shield-alert"}
      class={enabled ? "text-success" : "text-text-tertiary"}
    />
    <div class="min-w-0 flex-1">
      {#if enabled === null}
        <p class="text-text-secondary">Checking...</p>
      {:else if enabled}
        <p class="font-medium">On <Badge tone="success">Protected</Badge></p>
        <p class="text-(length:--text-sm) text-text-secondary">
          Setting up a new one makes the old code or cards stop working.
        </p>
      {:else}
        <p class="font-medium">Off</p>
        <p class="text-(length:--text-sm) text-text-secondary">
          If you forget your master password, your data cannot be recovered.
        </p>
      {/if}
    </div>
    {#if enabled === true}
      <Button onclick={() => void start("setup")}>Replace</Button>
      <Button onclick={() => void start("off")}>Turn off</Button>
    {:else if enabled === false}
      <Button variant="primary" onclick={() => void start("setup")}>Set up</Button>
    {/if}
  </div>
</section>

{#if dialog}
  <RecoveryDialog
    mode={dialog}
    {keyKept}
    onerror={failed}
    ondone={finished}
    oncancel={() => (dialog = null)}
  />
{/if}
