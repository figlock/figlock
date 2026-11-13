<script lang="ts">
  import { Alert, Button, SelectField, TextField } from "@figlock/ui";
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
  let mode = $state<"code" | "shares">("code");
  let threshold = $state("2");
  let count = $state("3");
  let material = $state<RecoveryMaterial | null>(null);
  let saved = $state(false);
  let answers = $state<string[]>([]);
  let busy = $state(false);
  let error = $state("");
  let wrong = $state("");

  const thresholds = $derived(
    Array.from({ length: Number(count) - 1 }, (_, i) => ({
      value: String(i + 2),
      label: `${i + 2} of ${count}`,
    })),
  );
  const counts = Array.from({ length: 15 }, (_, i) => ({
    value: String(i + 2),
    label: `${i + 2} cards`,
  }));

  $effect(() => {
    // Keep the threshold valid when the number of cards shrinks.
    if (Number(threshold) > Number(count)) threshold = count;
  });

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
      material = await recoveryPrepare(mode === "shares", Number(threshold), Number(count));
      answers = Array(material.mode === "code" ? 1 : material.cards.length).fill("");
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
    if (answers.some((a) => a.trim() === "")) {
      wrong = "Fill in every box.";
      return;
    }
    busy = true;
    try {
      if (await recoveryCheck(answers)) {
        done = true;
        onconfirmed();
      } else {
        wrong =
          material?.mode === "code"
            ? "These do not match the end of the Recovery Key. Check and try again."
            : "One of these words does not match its card. Check and try again.";
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
      If you forget your master password, a Recovery Key opens the vault so you can set a new one.
      Anyone who holds it can open your vault, so keep it apart from your master password and your
      Emergency Kit.
    </p>
    <fieldset class="flex flex-col gap-2">
      <legend class="mb-1 text-(length:--text-sm) font-medium">How to keep it</legend>
      <label class="flex items-start gap-2">
        <input type="radio" name="rk-mode" value="code" bind:group={mode} class="mt-1" />
        <span>
          One code
          <span class="block text-(length:--text-xs) leading-4 text-text-tertiary">
            Print it or write it down and keep it somewhere safe.
          </span>
        </span>
      </label>
      <label class="flex items-start gap-2">
        <input type="radio" name="rk-mode" value="shares" bind:group={mode} class="mt-1" />
        <span>
          Cards
          <span class="block text-(length:--text-xs) leading-4 text-text-tertiary">
            Split it into cards for different people or places. Only some of them are needed, and
            fewer than that reveal nothing.
          </span>
        </span>
      </label>
    </fieldset>
    {#if mode === "shares"}
      <div class="flex flex-wrap gap-4">
        <SelectField label="Cards to make" bind:value={count} options={counts} />
        <SelectField label="Needed to open the vault" bind:value={threshold} options={thresholds} />
      </div>
    {/if}
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
      FigLock shows this only once and does not keep it. {material.mode === "code"
        ? "Anyone who has this code can open your vault without your master password."
        : `Anyone who gathers ${material.threshold} of the ${material.count} cards can open your vault without your master password.`}
    </Alert>
    {#if material.mode === "code"}
      <p
        class="rounded-(--radius-sm) bg-surface-sunken p-4 font-mono text-(length:--text-md) leading-7 break-words"
        data-testid="recovery-code"
      >
        {material.code}
      </p>
    {:else}
      <ul class="grid gap-3" aria-label="Recovery cards">
        {#each material.cards as card, i (i)}
          <li class="rounded-(--radius-sm) bg-surface-sunken p-4" data-testid="recovery-card">
            <p class="mb-2 font-semibold">Card {i + 1} of {material.count}</p>
            <ol class="grid grid-cols-3 gap-x-4 font-mono text-(length:--text-sm) leading-6">
              {#each card as word, w (w)}
                <li class="flex gap-2">
                  <span class="w-5 text-right text-text-tertiary">{w + 1}.</span>
                  <span>{word}</span>
                </li>
              {/each}
            </ol>
          </li>
        {/each}
      </ul>
    {/if}
    <div class="flex flex-wrap gap-2">
      <Button onclick={save}>
        {saved
          ? "Saved. Save again"
          : material.mode === "code"
            ? "Save as PDF"
            : "Save cards as PDF"}
      </Button>
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
      {material.mode === "code"
        ? "To be sure you can read it, type the last two groups of the Recovery Key from what you saved."
        : "To be sure the cards are readable, type the word asked for from each card you saved."}
    </p>
    {#if material.mode === "code"}
      <TextField
        label="Last two groups"
        bind:value={answers[0]}
        error={wrong}
        placeholder="XXXX-XXXX"
        mono
        focus
      />
    {:else}
      {#each material.challenge as pos, i (i)}
        <TextField
          label="Card {i + 1}, word {pos}"
          bind:value={answers[i]}
          error={i === 0 ? wrong : undefined}
          focus={i === 0}
          mono
        />
      {/each}
    {/if}
    <div class="flex gap-2">
      <Button onclick={() => (step = "show")}>Back</Button>
      <div class="flex-1"></div>
      <Button variant="primary" type="submit" disabled={busy}>
        {busy ? "Checking..." : "Confirm"}
      </Button>
    </div>
  </form>
{/if}
