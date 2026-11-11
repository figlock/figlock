<script lang="ts">
  import { Button, Icon } from "@figlock/ui";
  import { onMount } from "svelte";

  import {
    chooseFiles,
    MAX_FILE_BYTES,
    MAX_FILES,
    onFileDrag,
    stageFiles,
    unstageFile,
    type FileInfo,
    type Staged,
    type StagedFile,
  } from "./items";
  import { fmtSize } from "./util";
  import { failure, getWorkspace } from "./workspace";

  type Props = {
    /** Files the item already has, minus the ones removed. */
    kept: FileInfo[];
    staged: StagedFile[];
    error?: string;
  };
  let { kept = $bindable(), staged = $bindable(), error = $bindable() }: Props = $props();

  const ws = getWorkspace();
  const labelId = $props.id();
  let over = $state(false);
  let refused = $state<{ name: string; message: string }[]>([]);

  const count = $derived(kept.length + staged.length);

  function take(result: Staged) {
    const room = Math.max(0, MAX_FILES - count);
    const fits = result.files.slice(0, room);
    for (const extra of result.files.slice(room)) void unstageFile(extra.token);
    staged = [...staged, ...fits];
    refused =
      result.files.length > room
        ? [...result.refused, { name: "", message: `You can add up to ${MAX_FILES} files.` }]
        : result.refused;
    if (fits.length) error = "";
  }

  async function choose() {
    try {
      take(await chooseFiles());
    } catch (e) {
      error = failure(ws, e);
    }
  }

  onMount(() => {
    const off = onFileDrag(async (isOver, paths) => {
      over = isOver;
      if (!paths?.length) return;
      try {
        take(await stageFiles(paths));
      } catch (e) {
        error = failure(ws, e);
      }
    });
    return () => void off.then((f) => f());
  });

  const dropStaged = (f: StagedFile) => {
    staged = staged.filter((s) => s.token !== f.token);
    void unstageFile(f.token);
  };
</script>

<div>
  <span id={labelId} class="mb-1 block text-(length:--text-sm) leading-5 font-medium">
    Files<span class="text-danger" aria-hidden="true"> *</span>
  </span>
  <div
    role="group"
    aria-labelledby={labelId}
    class="flex flex-col items-center gap-2 rounded-(--radius-md) border-[1.5px] border-dashed px-4 py-6 text-center text-text-secondary transition-colors duration-(--duration-fast) {over
      ? 'border-primary bg-primary-subtle text-text'
      : 'border-border-strong bg-surface'}"
  >
    <Icon name="upload" size={24} />
    <span class="font-medium text-text">Drop files here or choose files</span>
    <Button onclick={choose}>Choose files</Button>
    <span class="text-(length:--text-xs) leading-4">
      Up to {MAX_FILES} files, {Math.round(MAX_FILE_BYTES / 1024)} KB each. Files are encrypted on this
      device before they sync.
    </span>
  </div>

  {#if count}
    <ul class="mt-2 divide-y divide-border">
      {#each kept as file (file.id)}
        <li class="flex items-center gap-3 py-2">
          <Icon name="file-text" size={20} class="text-text-secondary" />
          <span class="grid min-w-0 flex-1">
            <span class="truncate">{file.name}</span>
            <span class="text-(length:--text-xs) leading-4 text-text-tertiary"
              >{fmtSize(file.size)}</span
            >
          </span>
          <Button onclick={() => (kept = kept.filter((k) => k.id !== file.id))}>
            <Icon name="x" /><span class="sr-only">Remove {file.name}</span>
          </Button>
        </li>
      {/each}
      {#each staged as file (file.token)}
        <li class="flex items-center gap-3 py-2">
          <Icon name="file-text" size={20} class="text-text-secondary" />
          <span class="grid min-w-0 flex-1">
            <span class="truncate">{file.name}</span>
            <span class="text-(length:--text-xs) leading-4 text-text-tertiary"
              >{fmtSize(file.size)}</span
            >
          </span>
          <Button onclick={() => dropStaged(file)}>
            <Icon name="x" /><span class="sr-only">Remove {file.name}</span>
          </Button>
        </li>
      {/each}
    </ul>
    <p class="mt-1 text-(length:--text-xs) leading-4 text-text-tertiary">
      {count} of {MAX_FILES} files
    </p>
  {/if}

  {#each refused as r, i (i)}
    <p role="alert" class="mt-1 flex gap-1 text-(length:--text-xs) leading-4 text-danger">
      <Icon name="circle-alert" size={14} />
      <span
        >{#if r.name}<b class="font-medium">{r.name}:</b>
        {/if}{r.message}</span
      >
    </p>
  {/each}
  {#if error}
    <p role="alert" class="mt-1 text-(length:--text-xs) leading-4 text-danger">{error}</p>
  {/if}
</div>
