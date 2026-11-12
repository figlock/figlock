<script lang="ts">
  import { Alert, Button, TextField } from "@figlock/ui";
  import { onMount } from "svelte";

  import Shell from "./Shell.svelte";
  import { StorageChoice } from "./storage.svelte";
  import StoragePicker from "./StoragePicker.svelte";
  import TouchIdOption from "./TouchIdOption.svelte";
  import {
    cancelCreate,
    checkKitKey,
    createVault,
    errorOf,
    finishCreate,
    saveEmergencyKit,
    suggestedFolder,
    type VaultState,
  } from "./vault";

  type Props = { touchAvailable: boolean; onback: () => void; ondone: (s: VaultState) => void };
  let { touchAvailable, onback, ondone }: Props = $props();

  let step = $state<"form" | "key">("form");
  const storage = new StorageChoice("create");
  let password = $state("");
  let again = $state("");
  let secretKey = $state("");
  let shown = $state(false);
  let kitSaved = $state(false);
  let typed = $state("");
  let touchId = $state(true);
  let busy = $state(false);
  let error = $state("");
  let fieldError = $state<{ password?: string; again?: string; typed?: string }>({});

  onMount(async () => {
    storage.folder = (await suggestedFolder()) ?? "";
  });

  async function next(e: Event) {
    e.preventDefault();
    error = "";
    fieldError = {};
    if (password.length < 8) {
      fieldError = { password: "Use at least 8 characters." };
      return;
    }
    if (password !== again) {
      fieldError = { again: "The two passwords do not match." };
      return;
    }
    busy = true;
    try {
      secretKey = (await createVault(storage.value, password)).secretKey;
      step = "key";
    } catch (e) {
      error = errorOf(e).message;
    } finally {
      busy = false;
    }
  }

  async function back() {
    await cancelCreate().catch(() => {});
    secretKey = "";
    typed = "";
    kitSaved = false;
    shown = false;
    step = "form";
  }

  async function saveKit() {
    error = "";
    try {
      if (await saveEmergencyKit()) kitSaved = true;
    } catch (e) {
      error = errorOf(e).message;
    }
  }

  async function finish(e: Event) {
    e.preventDefault();
    error = "";
    fieldError = {};
    busy = true;
    try {
      if (!(await checkKitKey(typed))) {
        fieldError = {
          typed: "These do not match the end of your Secret Key. Check and try again.",
        };
        return;
      }
      ondone(await finishCreate(touchAvailable && touchId));
    } catch (e) {
      error = errorOf(e).message;
    } finally {
      busy = false;
    }
  }
</script>

{#if step === "form"}
  <Shell title="Create your vault" subtitle="Choose where it lives and a master password.">
    <form class="flex flex-col gap-4" onsubmit={next}>
      {#if error}<Alert>{error}</Alert>{/if}
      <StoragePicker
        {storage}
        folderHelp="Only encrypted files are kept here. A folder synced by iCloud Drive works well."
      />
      <TextField
        label="Master password"
        type="password"
        bind:value={password}
        error={fieldError.password}
        help="Pick something you can remember. FigLock cannot reset it."
        autocomplete="new-password"
        focus
      />
      <TextField
        label="Repeat master password"
        type="password"
        bind:value={again}
        error={fieldError.again}
        autocomplete="new-password"
      />
      <div class="flex gap-2">
        <Button onclick={onback}>Back</Button>
        <div class="flex-1"></div>
        <Button variant="primary" type="submit" disabled={busy || !storage.ready}>
          {busy ? "Creating..." : "Continue"}
        </Button>
      </div>
    </form>
  </Shell>
{:else}
  <Shell
    title="Save your Secret Key"
    subtitle="You need it, with your master password, to open this vault on a new device."
  >
    <form class="flex flex-col gap-4" onsubmit={finish}>
      {#if error}<Alert>{error}</Alert>{/if}
      <Alert tone="warning">
        FigLock cannot recover this key. Keep your Emergency Kit somewhere safe, apart from your
        master password.
      </Alert>
      <div class="rounded-(--radius-sm) bg-surface-sunken p-4">
        <p
          class="flex flex-wrap gap-x-3 font-mono text-(length:--text-md) leading-6"
          data-testid="secret-key"
        >
          {#each secretKey.split("-") as group, i (i)}
            <span>{shown ? group : group.replace(/[A-Z0-9]/g, "•")}</span>
          {/each}
        </p>
        <div class="mt-3 flex gap-2">
          <Button onclick={() => (shown = !shown)}>{shown ? "Hide" : "Show"}</Button>
          <Button onclick={saveKit}>{kitSaved ? "Saved. Save again" : "Save Emergency Kit"}</Button>
        </div>
      </div>
      <TextField
        label="Confirm you saved it"
        bind:value={typed}
        error={fieldError.typed}
        help="Type the last two groups of the Secret Key."
        placeholder="XXXX-XXXX"
        mono
      />
      {#if touchAvailable}<TouchIdOption bind:checked={touchId} />{/if}
      <div class="flex gap-2">
        <Button onclick={back}>Back</Button>
        <div class="flex-1"></div>
        <Button variant="primary" type="submit" disabled={busy || typed.trim() === ""}>
          {busy ? "Creating..." : "Create vault"}
        </Button>
      </div>
    </form>
  </Shell>
{/if}
