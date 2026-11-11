<script lang="ts">
  import { Button, Icon } from "@figlock/ui";
  import { onDestroy } from "svelte";

  import { copyField, revealField } from "./items";
  import { toast } from "./toast.svelte";
  import { REVEAL_MS } from "./util";
  import { failure, getWorkspace } from "./workspace";

  type Props = {
    id: string;
    field: string;
    label: string;
    mono?: boolean;
    /** A value with line breaks, such as a private key. */
    multi?: boolean;
  };
  let { id, field, label, mono = false, multi = false }: Props = $props();

  const ws = getWorkspace();
  const labelId = $props.id();

  // The value is in this component only while it is shown, and is dropped when it hides or the page closes.
  let value = $state<string | null>(null);
  let hideAt = $state(0);
  let clearAt = $state(0);
  let clearMs = $state(0);
  let now = $state(Date.now());
  let error = $state("");

  const hideLeft = $derived(Math.max(0, hideAt - now));
  const clearLeft = $derived(Math.max(0, clearAt - now));

  const ticker = setInterval(() => {
    now = Date.now();
    if (value !== null && now >= hideAt) hide();
    if (clearAt && now >= clearAt) {
      clearAt = 0;
      toast.show("Clipboard cleared");
    }
  }, 250);
  onDestroy(() => {
    clearInterval(ticker);
    value = null;
  });

  const hide = () => {
    value = null;
    hideAt = 0;
  };

  async function toggle() {
    error = "";
    if (value !== null) return hide();
    try {
      value = await revealField(id, field);
      now = Date.now();
      hideAt = now + REVEAL_MS;
    } catch (e) {
      error = failure(ws, e);
    }
  }

  async function copy() {
    error = "";
    try {
      const secs = await copyField(id, field);
      now = Date.now();
      clearMs = secs * 1000;
      clearAt = now + clearMs;
      toast.show(`Copied ${label.toLowerCase()}`);
    } catch (e) {
      error = failure(ws, e);
    }
  }
</script>

<div class="grid gap-1 px-4 py-3">
  <div
    id={labelId}
    class="text-(length:--text-sm) leading-5 font-medium text-text-secondary {mono
      ? 'font-mono'
      : ''}"
  >
    {label}
  </div>
  <div class="flex items-center gap-2">
    <div
      role="group"
      aria-labelledby={labelId}
      class="flex min-h-(--control-height-md) min-w-0 flex-1 items-center rounded-(--radius-sm) bg-surface-sunken px-3 font-mono text-(length:--text-sm) break-all {multi
        ? 'max-h-45 items-start overflow-auto py-2 whitespace-pre-wrap'
        : ''}"
    >
      {#if value !== null}
        {value}
      {:else}
        <span aria-hidden="true">••••••••••••</span><span class="sr-only">Hidden</span>
      {/if}
    </div>
    <Button onclick={toggle} label="{value !== null ? 'Hide' : 'Show'} {label.toLowerCase()}">
      <Icon name={value !== null ? "eye-off" : "eye"} />
      {value !== null ? "Hide" : "Show"}
    </Button>
    <Button onclick={copy} label="Copy {label.toLowerCase()}">
      <Icon name="copy" />
      Copy
    </Button>
  </div>
  {#if error}
    <p class="text-(length:--text-xs) leading-4 text-danger">{error}</p>
  {/if}
  {#if value !== null && hideLeft > 0}
    <div class="mt-2 grid gap-1.5 text-(length:--text-xs) leading-4 text-text-secondary">
      <span class="flex items-center gap-1.5">
        <Icon name="eye" size={12} />Hidden again in {Math.ceil(hideLeft / 1000)}s
      </span>
      <div class="h-0.75 overflow-hidden rounded-full bg-border" aria-hidden="true">
        <i
          class="block h-full origin-left bg-primary"
          style:transform="scaleX({hideLeft / REVEAL_MS})"
        ></i>
      </div>
    </div>
  {/if}
  {#if clearLeft > 0}
    <div class="mt-2 grid gap-1.5 text-(length:--text-xs) leading-4 text-text-secondary">
      <span class="flex items-center gap-1.5">
        <Icon name="clock" size={12} />Clipboard clears in {Math.ceil(clearLeft / 1000)}s
      </span>
      <div class="h-0.75 overflow-hidden rounded-full bg-border" aria-hidden="true">
        <i
          class="block h-full origin-left bg-primary"
          style:transform="scaleX({clearMs ? clearLeft / clearMs : 0})"
        ></i>
      </div>
    </div>
  {/if}
</div>
