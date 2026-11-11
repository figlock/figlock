<script lang="ts">
  import { Alert, Button, TextField } from "@figlock/ui";
  import { onDestroy } from "svelte";

  import FolderField from "./FolderField.svelte";
  import Shell from "./Shell.svelte";
  import TouchIdOption from "./TouchIdOption.svelte";
  import {
    errorOf,
    joinAnswered,
    joinCancel,
    joinFinish,
    joinStart,
    type VaultState,
  } from "./vault";

  type Props = { touchAvailable: boolean; onback: () => void; ondone: (s: VaultState) => void };
  let { touchAvailable, onback, ondone }: Props = $props();

  const POLL_MS = 2000;

  let step = $state<"code" | "waiting" | "password">("code");
  let folder = $state("");
  let code = $state("");
  let fingerprint = $state("");
  let password = $state("");
  let touchId = $state(true);
  let busy = $state(false);
  let error = $state("");
  let fieldError = $state<{ folder?: string; code?: string; password?: string }>({});
  let timer: ReturnType<typeof setInterval> | undefined;

  const stop = () => {
    clearInterval(timer);
    timer = undefined;
  };
  onDestroy(stop);

  async function request(e: Event) {
    e.preventDefault();
    error = "";
    fieldError = {};
    if (!folder) fieldError.folder = "Choose the folder that holds the vault.";
    if (!code.trim()) fieldError.code = "Enter the invite code shown on your other device.";
    if (fieldError.folder || fieldError.code) return;
    busy = true;
    try {
      fingerprint = await joinStart(folder, code);
      step = "waiting";
      timer = setInterval(async () => {
        try {
          if (await joinAnswered()) {
            stop();
            step = "password";
          }
        } catch (e) {
          stop();
          error = errorOf(e).message;
          step = "code";
        }
      }, POLL_MS);
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "invite") fieldError.code = err.message;
      else error = err.message;
    } finally {
      busy = false;
    }
  }

  async function cancel() {
    stop();
    await joinCancel().catch(() => {});
    step = "code";
  }

  async function finish(e: Event) {
    e.preventDefault();
    error = "";
    fieldError = {};
    if (!password) {
      fieldError.password = "Enter your master password.";
      return;
    }
    busy = true;
    try {
      ondone(await joinFinish(password, touchAvailable && touchId));
    } catch (e) {
      error = errorOf(e).message;
    } finally {
      busy = false;
    }
  }
</script>

{#if step === "code"}
  <Shell
    title="Add this Mac"
    subtitle="On a device that already has your vault, choose to add a device and enter the code it shows."
  >
    <form class="flex flex-col gap-4" onsubmit={request}>
      {#if error}<Alert>{error}</Alert>{/if}
      <FolderField
        bind:value={folder}
        error={fieldError.folder}
        help="The same storage your vault uses."
      />
      <TextField
        label="Invite code"
        bind:value={code}
        error={fieldError.code}
        placeholder="XXXX-XXXX-XXXX-XXXX-XXXX"
        mono
        focus
      />
      <div class="flex gap-2">
        <Button onclick={onback}>Back</Button>
        <div class="flex-1"></div>
        <Button variant="primary" type="submit" disabled={busy}>
          {busy ? "Sending..." : "Request to join"}
        </Button>
      </div>
    </form>
  </Shell>
{:else if step === "waiting"}
  <Shell title="Approve on your other device" subtitle="Waiting for it to answer.">
    <div class="flex flex-col gap-4">
      {#if error}<Alert>{error}</Alert>{/if}
      <div class="rounded-(--radius-sm) bg-surface-sunken p-4 text-center">
        <p class="font-mono text-(length:--text-lg) tracking-wider" data-testid="fingerprint">
          {fingerprint}
        </p>
        <p class="mt-2 text-(length:--text-xs) text-text-tertiary">
          Check that your other device shows the same code before you approve.
        </p>
      </div>
      <Button onclick={cancel}>Cancel</Button>
    </div>
  </Shell>
{:else}
  <Shell title="Approved" subtitle="Enter your master password to finish adding this Mac.">
    <form class="flex flex-col gap-4" onsubmit={finish}>
      {#if error}<Alert>{error}</Alert>{/if}
      <TextField
        label="Master password"
        type="password"
        bind:value={password}
        error={fieldError.password}
        focus
      />
      {#if touchAvailable}<TouchIdOption bind:checked={touchId} />{/if}
      <Button variant="primary" type="submit" block disabled={busy}>
        {busy ? "Joining..." : "Join vault"}
      </Button>
    </form>
  </Shell>
{/if}
