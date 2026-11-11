<script lang="ts">
  import { Button, Icon } from "@figlock/ui";
  import type { Snippet } from "svelte";

  import { copyField } from "./items";
  import { toast } from "./toast.svelte";
  import { failure, getWorkspace } from "./workspace";

  type Props = {
    label: string;
    /** The text to show; the row is left out when it is empty. */
    value?: string;
    mono?: boolean;
    /** Schema field to copy; no Copy button without it. */
    copy?: { id: string; field: string };
    children?: Snippet;
  };
  let { label, value = "", mono = false, copy, children }: Props = $props();

  const ws = getWorkspace();

  async function doCopy() {
    if (!copy) return;
    try {
      await copyField(copy.id, copy.field);
      toast.show(`Copied ${label.toLowerCase()}`);
    } catch (e) {
      toast.show(failure(ws, e));
    }
  }
</script>

{#if value || children}
  <div class="grid gap-1 px-4 py-3">
    <div class="text-(length:--text-sm) leading-5 font-medium text-text-secondary">{label}</div>
    <div class="flex items-center gap-2">
      <div
        class="min-w-0 flex-1 wrap-anywhere whitespace-pre-wrap {mono
          ? 'font-mono text-(length:--text-sm)'
          : ''}"
      >
        {#if children}{@render children()}{:else}{value}{/if}
      </div>
      {#if copy}
        <Button variant="ghost" onclick={doCopy} label="Copy {label.toLowerCase()}">
          <Icon name="copy" />Copy
        </Button>
      {/if}
    </div>
  </div>
{/if}
