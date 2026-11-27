<script lang="ts">
  import { Alert, Button, TextField } from "@figlock/ui";
  import { onDestroy } from "svelte";

  import {
    errorOf,
    recoveryCancel,
    recoveryCheck,
    recoveryPrepare,
    saveRecoveryKit,
    type RecoveryMaterial,
  } from "./vault";

  type Props = {
    /** The key was shown and the person proved they can read it back. */
    onconfirmed: () => void;
    /** The person backed out; the key is dropped. */
    onback: () => void;
    backLabel?: string;
    /** The vault locked or another device changed it. */
    onerror?: (code: string, message: string) => void;
  };
  let { onconfirmed, onback, backLabel = "Back", onerror }: Props = $props();

  type Step = "choose" | "show" | "verify";
  let step = $state<Step>("choose");
  let material = $state<RecoveryMaterial | null>(null);
  let saved = $state(false);
  let answer = $state("");
  let busy = $state(false);
  let error = $state("");
  let wrong = $state("");

  // A key that was never confirmed must not linger in memory if this screen goes away.
  let done = false;
  onDestroy(() => {
    if (!done) void recoveryCancel().catch(() => {});
  });

  async function make(e: Event) {
    e.preventDefault();
    error = "";
    busy = true;
    try {
      material = await recoveryPrepare();
      answer = "";
      saved = false;
      step = "show";
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "locked") onerror?.(err.code, err.message);
      else error = err.message;
    } finally {
      busy = false;
    }
  }

  async function save() {
    error = "";
    try {
      if (await saveRecoveryKit()) saved = true;
    } catch (e) {
      error = errorOf(e).message;
    }
  }

  async function verify(e: Event) {
    e.preventDefault();
    error = "";
    wrong = "";
    if (answer.trim() === "") {
      wrong = "Fill in the box.";
      return;
    }
    busy = true;
    try {
      if (await recoveryCheck(answer)) {
        done = true;
        onconfirmed();
      } else {
        wrong = "These do not match the end of the Recovery Key. Check and try again.";
      }
    } catch (e) {
      error = errorOf(e).message;
    } finally {
      busy = false;
    }
  }

  async function startOver() {
    await recoveryCancel().catch(() => {});
    material = null;
    step = "choose";
  }
</script>

{#if step === "choose"}
  <form class="flex flex-col gap-4" onsubmit={make}>
    {#if error}<Alert>{error}</Alert>{/if}
    <p class="text-text-secondary">
      If you forget your password, a Recovery Key opens the vault so you can set a new one. Anyone
      who holds it can open your vault, so keep it apart from your password and your Emergency Kit.
    </p>
    <div class="flex gap-2">
      <Button onclick={onback}>{backLabel}</Button>
      <div class="flex-1"></div>
      <Button variant="primary" type="submit" disabled={busy}>
        {busy ? "Making..." : "Make Recovery Key"}
      </Button>
    </div>
  </form>
{:else if step === "show" && material}
  <div class="flex flex-col gap-4">
    {#if error}<Alert>{error}</Alert>{/if}
    <Alert tone="warning">
      FigLock shows this only once and does not keep it. Anyone who has this code can open your
      vault without your password.
    </Alert>
    <p
      class="rounded-(--radius-sm) bg-surface-sunken p-4 font-mono text-(length:--text-md) leading-7 break-words"
      data-testid="recovery-code"
    >
      {material.code}
    </p>
    <div class="flex flex-wrap gap-2">
      <Button onclick={save}>{saved ? "Saved. Save again" : "Save as PDF"}</Button>
    </div>
    <div class="flex gap-2">
      <Button onclick={startOver}>Start over</Button>
      <div class="flex-1"></div>
      <Button variant="primary" onclick={() => (step = "verify")}>I have saved it</Button>
    </div>
  </div>
{:else if step === "verify" && material}
  <form class="flex flex-col gap-4" onsubmit={verify}>
    {#if error}<Alert>{error}</Alert>{/if}
    <p class="text-text-secondary">
      To be sure you can read it, type the last two groups of the Recovery Key from what you saved.
    </p>
    <TextField
      label="Last two groups"
      bind:value={answer}
      error={wrong}
      placeholder="XXXX-XXXX"
      mono
      focus
    />
    <div class="flex gap-2">
      <Button onclick={() => (step = "show")}>Back</Button>
      <div class="flex-1"></div>
      <Button variant="primary" type="submit" disabled={busy}>
        {busy ? "Checking..." : "Confirm"}
      </Button>
    </div>
  </form>
{/if}
