<script lang="ts">
  import { Alert, Button, TextField } from "@figlock/ui";
  import { onDestroy } from "svelte";

  import RecoverySetup from "./RecoverySetup.svelte";
  import Shell from "./Shell.svelte";
  import { StorageChoice } from "./storage.svelte";
  import StoragePicker from "./StoragePicker.svelte";
  import TouchIdOption from "./TouchIdOption.svelte";
  import { ageText } from "./util";
  import {
    checkKitKey,
    errorOf,
    forgotCancel,
    forgotCommit,
    forgotFind,
    forgotPrepare,
    recoveryCancel,
    saveEmergencyKit,
    type Found,
    type VaultState,
  } from "./vault";

  type Props = {
    touchAvailable: boolean;
    /** This Mac already has the vault (the Unlock screen): the vault is the one it is set up with. */
    here?: boolean;
    onback: () => void;
    ondone: (s: VaultState) => void;
  };
  let { touchAvailable, here = false, onback, ondone }: Props = $props();

  type Step = "key" | "found" | "newkey" | "kit";
  let step = $state<Step>("key");
  const storage = new StorageChoice();

  // Step 1: the Recovery Key code.
  let code = $state("");

  // Step 2: what was found, and the new master password.
  let found = $state<Found | null>(null);
  let password = $state("");
  let again = $state("");

  // Step 3: a new Recovery Key, or keep the one just used.
  let makeNew = $state(true);
  let newKeyReady = $state(false);
  let adding = $state(false);

  // Step 4: the new Secret Key and Emergency Kit.
  let secretKey = $state("");
  let shown = $state(false);
  let kitSaved = $state(false);
  let typed = $state("");
  let touchId = $state(true);

  let busy = $state(false);
  let error = $state("");
  let fieldError = $state<{
    folder?: string;
    code?: string;
    password?: string;
    again?: string;
    typed?: string;
  }>({});

  // Nothing is stored until the end, so leaving only drops what is held in memory.
  let finished = false;
  onDestroy(() => {
    if (finished) return;
    void forgotCancel().catch(() => {});
    void recoveryCancel().catch(() => {});
  });

  async function find(e: Event) {
    e.preventDefault();
    error = "";
    fieldError = {};
    if (!here && storage.kind === "folder" && !storage.folder) {
      fieldError.folder = "Choose the folder that holds your vault.";
    }
    if (!here && storage.kind === "drive" && !storage.ready) error = "Sign in to Google first.";
    if (code.trim() === "") fieldError.code = "Enter your Recovery Key.";
    if (error || fieldError.folder || fieldError.code) return;
    busy = true;
    try {
      found = await forgotFind(here ? {} : storage.value, code);
      step = "found";
    } catch (e) {
      error = errorOf(e).message;
    } finally {
      busy = false;
    }
  }

  function choosePassword(e: Event) {
    e.preventDefault();
    fieldError = {};
    if (password.length < 8) fieldError = { password: "Use at least 8 characters." };
    else if (password !== again) fieldError = { again: "The two passwords do not match." };
    else step = "newkey";
  }

  async function makeCredentials() {
    error = "";
    busy = true;
    try {
      secretKey = (await forgotPrepare(password, makeNew)).secretKey;
      shown = false;
      kitSaved = false;
      typed = "";
      step = "kit";
    } catch (e) {
      const err = errorOf(e);
      error = err.message;
      if (err.code === "recovery_not_confirmed") newKeyReady = false;
    } finally {
      busy = false;
    }
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
      const state = await forgotCommit(touchAvailable && touchId);
      finished = true;
      ondone(state);
    } catch (e) {
      error = errorOf(e).message;
    } finally {
      busy = false;
    }
  }

  async function back() {
    await forgotCancel().catch(() => {});
    onback();
  }
</script>

{#if step === "key"}
  <Shell
    title="I forgot my password"
    subtitle="Use your Recovery Key to get back in and choose a new password."
  >
    <form class="flex flex-col gap-4" onsubmit={find}>
      {#if error}<Alert>{error}</Alert>{/if}
      {#if !here}
        <StoragePicker {storage} folderError={fieldError.folder} />
      {/if}
      <TextField
        label="Recovery Key"
        bind:value={code}
        error={fieldError.code}
        placeholder="FLR1-XXXX-XXXX-..."
        mono
        focus
      />
      <div class="flex gap-2">
        <Button onclick={back}>Back</Button>
        <div class="flex-1"></div>
        <Button variant="primary" type="submit" disabled={busy || (!here && !storage.ready)}>
          {busy ? "Opening..." : "Continue"}
        </Button>
      </div>
    </form>
  </Shell>
{:else if step === "found" && found}
  <Shell title="Found your vault" subtitle="Choose a new master password.">
    <form class="flex flex-col gap-4" onsubmit={choosePassword}>
      <div class="rounded-(--radius-sm) bg-surface-sunken p-4" data-testid="found">
        <p class="font-semibold">{found.name}</p>
        <p class="text-text-secondary">
          {found.items === 1 ? "1 item" : `${found.items} items`}{found.lastChangeMs
            ? `, last changed ${ageText(found.lastChangeMs)}`
            : ""}.
          {found.devices === 1 ? "1 other device" : `${found.devices} other devices`} can open it.
        </p>
      </div>
      <TextField
        label="New master password"
        type="password"
        bind:value={password}
        error={fieldError.password}
        help="Pick something you can remember. FigLock cannot reset it."
        autocomplete="new-password"
        focus
      />
      <TextField
        label="Repeat new master password"
        type="password"
        bind:value={again}
        error={fieldError.again}
        autocomplete="new-password"
      />
      <div class="flex gap-2">
        <Button onclick={back}>Back</Button>
        <div class="flex-1"></div>
        <Button variant="primary" type="submit">Continue</Button>
      </div>
    </form>
  </Shell>
{:else if step === "newkey"}
  <Shell
    title="Make a new Recovery Key?"
    subtitle="You just used yours, on a device you may not fully trust."
  >
    {#if adding}
      <RecoverySetup
        onconfirmed={() => {
          newKeyReady = true;
          adding = false;
        }}
        onback={() => (adding = false)}
      />
    {:else}
      <div class="flex flex-col gap-4">
        {#if error}<Alert>{error}</Alert>{/if}
        <label class="flex items-start gap-2">
          <input
            type="radio"
            name="newkey"
            checked={makeNew}
            onchange={() => (makeNew = true)}
            class="mt-1"
          />
          <span>
            Make a new one (recommended)
            <span class="block text-(length:--text-xs) leading-4 text-text-tertiary">
              The code you just used stop working.
            </span>
          </span>
        </label>
        <label class="flex items-start gap-2">
          <input
            type="radio"
            name="newkey"
            checked={!makeNew}
            onchange={() => (makeNew = false)}
            class="mt-1"
          />
          <span>
            Keep the one I just used
            <span class="block text-(length:--text-xs) leading-4 text-text-tertiary">
              Anyone who still has it can open your vault.
            </span>
          </span>
        </label>
        {#if makeNew && newKeyReady}
          <Alert tone="info">Your new Recovery Key is ready.</Alert>
        {/if}
        <div class="flex gap-2">
          <Button onclick={() => (step = "found")} disabled={busy}>Back</Button>
          <div class="flex-1"></div>
          {#if makeNew && !newKeyReady}
            <Button variant="primary" onclick={() => (adding = true)}>Make a Recovery Key</Button>
          {:else}
            <Button variant="primary" onclick={makeCredentials} disabled={busy}>
              {busy ? "Working..." : "Continue"}
            </Button>
          {/if}
        </div>
      </div>
    {/if}
  </Shell>
{:else if step === "kit"}
  <Shell
    title="Save your new Secret Key"
    subtitle="Your old Secret Key no longer works. Save the new one in a new Emergency Kit."
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
      <Alert tone="info">
        After this, open Settings, then Devices, and revoke any device you do not trust. Whoever had
        your old password and Secret Key can still open an old copy of your vault.
      </Alert>
      {#if touchAvailable}<TouchIdOption bind:checked={touchId} />{/if}
      <div class="flex gap-2">
        <Button onclick={() => (step = "newkey")} disabled={busy}>Back</Button>
        <div class="flex-1"></div>
        <Button variant="primary" type="submit" disabled={busy || typed.trim() === ""}>
          {busy ? "Opening..." : "Open my vault"}
        </Button>
      </div>
    </form>
  </Shell>
{/if}
