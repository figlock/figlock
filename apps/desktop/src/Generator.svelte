<script lang="ts">
  import { Button, Icon, SelectField } from "@figlock/ui";
  import { untrack } from "svelte";

  import {
    DEFAULT_PASSPHRASE,
    DEFAULT_PASSWORD,
    LENGTH,
    SEPARATORS,
    WORDS,
    copyText,
    generatePassphrase,
    generatePassword,
    type Generated,
  } from "./generator";
  import StrengthMeter from "./StrengthMeter.svelte";
  import { toast } from "./toast.svelte";
  import { failure, getWorkspace } from "./workspace";

  type Props = {
    /** Shown as a "Use this password" button; left out on the Generator page, which saves nothing. */
    onuse?: (value: string) => void;
    /** Fewer options, for the panel under a password field. */
    compact?: boolean;
  };
  let { onuse, compact = false }: Props = $props();

  const ws = getWorkspace();

  let mode = $state<"password" | "passphrase">("password");
  // `compact` is fixed for the life of the panel, so only its first value matters.
  let pw = $state({
    ...DEFAULT_PASSWORD,
    length: untrack(() => (compact ? 16 : DEFAULT_PASSWORD.length)),
  });
  let pp = $state({ ...DEFAULT_PASSPHRASE });
  let out = $state<Generated | null>(null);
  let error = $state("");

  const noClasses = $derived(!pw.lower && !pw.upper && !pw.digits && !pw.symbols);

  // The latest request wins, so a slow answer cannot replace a newer one.
  let ticket = 0;
  async function run() {
    const mine = ++ticket;
    const password = mode === "password";
    const spec = $state.snapshot(password ? pw : pp);
    if (password && noClasses) {
      out = null;
      error = "Turn on at least one character type.";
      return;
    }
    try {
      const next = password
        ? await generatePassword(spec as typeof pw)
        : await generatePassphrase(spec as typeof pp);
      if (mine !== ticket) return;
      out = next;
      error = "";
    } catch (e) {
      if (mine !== ticket) return;
      out = null;
      error = failure(ws, e);
    }
  }

  // Any option change makes a fresh value.
  $effect(() => {
    mode;
    $state.snapshot(pw);
    $state.snapshot(pp);
    void run();
  });

  async function copy() {
    if (!out) return;
    try {
      await copyText(out.value);
      toast.show("Copied");
    } catch (e) {
      error = failure(ws, e);
    }
  }

  const MODES = [
    { value: "password", label: "Password" },
    { value: "passphrase", label: "Passphrase" },
  ] as const;

  const TYPES = [
    { key: "lower", label: "Lowercase letters", example: "a-z" },
    { key: "upper", label: "Uppercase letters", example: "A-Z" },
    { key: "digits", label: "Numbers", example: "0-9" },
    { key: "symbols", label: "Symbols", example: "!#$%" },
  ] as const;
</script>

<div class="grid gap-4" data-testid="generator">
  <div class="rounded-(--radius-md) border border-border bg-surface-sunken p-4">
    <p
      class="min-h-6 font-mono text-(length:--text-md) leading-6 break-all {out
        ? 'text-text'
        : 'text-text-tertiary'}"
      aria-live="polite"
      data-testid="generated"
    >
      {out ? out.value : error || "Generating"}
    </p>
    <div class="mt-3"><StrengthMeter strength={out?.strength ?? null} /></div>
    {#if mode === "passphrase" && out && out.strength.level !== "strong"}
      <p class="mt-1 text-(length:--text-xs) leading-4 text-text-tertiary">
        Add more words to make this passphrase stronger.
      </p>
    {/if}
  </div>

  <div class="flex flex-wrap gap-2">
    {#if onuse}
      <Button variant="primary" onclick={() => out && onuse(out.value)} disabled={!out}>
        Use this {mode}
      </Button>
    {:else}
      <Button variant="primary" onclick={copy} disabled={!out}><Icon name="copy" />Copy</Button>
    {/if}
    <Button onclick={run} label="Generate again"><Icon name="refresh" />Generate again</Button>
    {#if onuse}
      <Button variant="ghost" onclick={copy} disabled={!out} label="Copy"
        ><Icon name="copy" /></Button
      >
    {/if}
  </div>

  <div
    role="radiogroup"
    aria-label="Type"
    class="inline-flex w-fit rounded-(--radius-sm) bg-surface-sunken p-0.5"
  >
    {#each MODES as m (m.value)}
      <button
        type="button"
        role="radio"
        aria-checked={mode === m.value}
        onclick={() => (mode = m.value)}
        class="h-7 rounded-(--radius-sm) px-3 text-(length:--text-sm) font-medium {mode === m.value
          ? 'bg-surface text-text shadow-sm'
          : 'text-text-secondary hover:text-text'}"
      >
        {m.label}
      </button>
    {/each}
  </div>

  {#if mode === "password"}
    <label class="grid gap-1">
      <span class="flex justify-between text-(length:--text-sm) font-medium">
        Length <span class="font-mono" data-testid="length">{pw.length}</span>
      </span>
      <input
        type="range"
        min={LENGTH.min}
        max={compact ? 64 : LENGTH.max}
        bind:value={pw.length}
        class="accent-primary"
      />
    </label>
    {#if !compact}
      <div class="grid gap-1" role="group" aria-label="Character types">
        {#each TYPES as t (t.key)}
          <label class="flex items-center gap-3 py-1">
            <input type="checkbox" bind:checked={pw[t.key]} class="size-4 accent-primary" />
            <span class="flex-1">{t.label}</span>
            <span class="font-mono text-(length:--text-xs) text-text-tertiary">{t.example}</span>
          </label>
        {/each}
        <label class="flex items-center gap-3 py-1">
          <input type="checkbox" bind:checked={pw.avoidLookalikes} class="size-4 accent-primary" />
          <span class="flex-1">Avoid look-alike characters</span>
          <span class="font-mono text-(length:--text-xs) text-text-tertiary">Il1O0o</span>
        </label>
      </div>
    {/if}
  {:else}
    <label class="grid gap-1">
      <span class="flex justify-between text-(length:--text-sm) font-medium">
        Words <span class="font-mono" data-testid="words">{pp.words}</span>
      </span>
      <input
        type="range"
        min={WORDS.min}
        max={WORDS.max}
        bind:value={pp.words}
        class="accent-primary"
      />
    </label>
    <SelectField label="Separator" bind:value={pp.separator} options={SEPARATORS} />
    {#if !compact}
      <label class="flex items-center gap-3 py-1">
        <input type="checkbox" bind:checked={pp.capitalize} class="size-4 accent-primary" />
        <span class="flex-1">Capitalize words</span>
      </label>
      <label class="flex items-center gap-3 py-1">
        <input type="checkbox" bind:checked={pp.addNumber} class="size-4 accent-primary" />
        <span class="flex-1">Include a number</span>
      </label>
    {/if}
  {/if}
</div>
