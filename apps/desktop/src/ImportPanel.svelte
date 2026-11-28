<script lang="ts">
  import { Alert, Button, Icon, SelectField } from "@figlock/ui";
  import { onDestroy } from "svelte";

  import {
    errorOf,
    importChoose,
    importCommit,
    importDiscard,
    type ImportPreview,
    type ImportSource,
  } from "./vault";
  import { failure, getWorkspace } from "./workspace";

  const w = getWorkspace();

  const SOURCES = [
    { value: "chrome", label: "Chrome (CSV)" },
    { value: "apple", label: "Apple Passwords (CSV)" },
    { value: "notes", label: "Notes (text file)" },
  ];
  const HELP: Record<ImportSource, string> = {
    chrome: "In Chrome, open Password Manager, Settings, and choose Export passwords.",
    apple: "In Passwords on your Mac, choose File, Export All Passwords.",
    notes: "A text file where notes are separated by a line of --- or ===.",
  };

  let source = $state<ImportSource>("chrome");
  let busy = $state(false);
  let error = $state("");
  let preview = $state<ImportPreview | null>(null);
  let added = $state<number | null>(null);

  const total = $derived(preview ? preview.logins + preview.notes + preview.tokens : 0);

  async function choose() {
    error = "";
    added = null;
    busy = true;
    try {
      preview = await importChoose(source);
    } catch (e) {
      preview = null;
      error = errorOf(e).code === "locked" ? failure(w, e) : errorOf(e).message;
    } finally {
      busy = false;
    }
  }

  async function confirm() {
    error = "";
    busy = true;
    try {
      added = await importCommit();
      preview = null;
      await w.library.refresh();
    } catch (e) {
      preview = null;
      error = failure(w, e);
      await w.library.refresh().catch(() => {});
    } finally {
      busy = false;
    }
  }

  async function cancel() {
    preview = null;
    error = "";
    try {
      await importDiscard();
    } catch {
      // Nothing was held, or the vault locked: either way nothing is waiting.
    }
  }

  // Leaving the page forgets a file that was previewed but not confirmed.
  onDestroy(() => {
    if (preview) void importDiscard().catch(() => {});
  });
</script>

<section aria-label="Import" class="grid gap-3">
  {#if preview}
    <div class="grid gap-3" data-testid="import-preview">
      <p class="font-medium">
        {total} item{total === 1 ? "" : "s"} found
      </p>
      <p class="text-text-secondary" data-testid="import-counts">
        {preview.logins} login{preview.logins === 1 ? "" : "s"}, {preview.notes}
        note{preview.notes === 1 ? "" : "s"}, {preview.tokens} API token{preview.tokens === 1
          ? ""
          : "s"}.
        {#if preview.skipped > 0}{preview.skipped} skipped (empty, too large or not supported).{/if}
      </p>
      {#if preview.tokensDetected > 0}
        <p class="text-text-secondary">
          Tokens found in notes are saved as token items and removed from the note text.
        </p>
      {/if}
      <ul
        class="max-h-64 overflow-auto rounded-(--radius-md) border border-border bg-surface p-2"
        aria-label="Items found"
      >
        {#each preview.titles as t, i (i)}
          <li class="truncate py-1 text-(length:--text-sm)">{t || "Untitled"}</li>
        {/each}
        {#if preview.more > 0}
          <li class="py-1 text-(length:--text-sm) text-text-secondary">and {preview.more} more</li>
        {/if}
      </ul>
      <p class="text-text-secondary">Nothing has been added yet.</p>
      <div class="grid grid-cols-2 gap-2">
        <Button onclick={cancel} disabled={busy}>Cancel</Button>
        <Button variant="primary" onclick={confirm} disabled={busy}>
          {busy ? "Importing..." : `Import ${total} item${total === 1 ? "" : "s"}`}
        </Button>
      </div>
    </div>
  {:else}
    <div class="flex items-start gap-3 rounded-(--radius-md) border border-border bg-surface p-3">
      <Icon name="upload" class="mt-0.5 text-text-tertiary" />
      <p class="text-text-secondary">
        Bring your items from another app. You see what was found before anything is added.
      </p>
    </div>
    <SelectField label="Where is the file from?" bind:value={source} options={SOURCES} />
    <p class="text-(length:--text-sm) text-text-secondary">{HELP[source]}</p>
    <Alert
      >The file holds your passwords in plain text. Delete it once the import is done, and empty the
      Trash.</Alert
    >
    <div>
      <Button variant="primary" onclick={choose} disabled={busy}>
        {busy ? "Reading..." : "Choose file..."}
      </Button>
    </div>
    {#if added !== null}
      <p class="text-text-secondary" data-testid="import-done" aria-live="polite">
        Imported {added} item{added === 1 ? "" : "s"}. Delete the export file now.
      </p>
    {/if}
  {/if}
  {#if error}<Alert>{error}</Alert>{/if}
</section>
