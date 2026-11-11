<script lang="ts">
  import { Alert, Button, SecretKeyField, TextField } from "@figlock/ui";

  import Shell from "./Shell.svelte";
  import { StorageChoice } from "./storage.svelte";
  import StoragePicker from "./StoragePicker.svelte";
  import TouchIdOption from "./TouchIdOption.svelte";
  import { errorOf, openExisting, type VaultState } from "./vault";

  type Props = { touchAvailable: boolean; onback: () => void; ondone: (s: VaultState) => void };
  let { touchAvailable, onback, ondone }: Props = $props();

  const storage = new StorageChoice();
  let password = $state("");
  let secretKey = $state("");
  let touchId = $state(true);
  let busy = $state(false);
  let error = $state("");
  let fieldError = $state<{ folder?: string; password?: string; key?: string }>({});

  async function submit(e: Event) {
    e.preventDefault();
    error = "";
    fieldError = {};
    if (storage.kind === "folder" && !storage.folder) {
      fieldError.folder = "Choose the folder that holds your vault.";
    }
    if (storage.kind === "drive" && !storage.ready) error = "Sign in to Google first.";
    if (!password) fieldError.password = "Enter your master password.";
    if (secretKey.split("-").length !== 9 || secretKey.split("-").some((g) => g.length === 0)) {
      fieldError.key = "This Secret Key looks incomplete. It has eight groups after FL1.";
    }
    if (error || fieldError.folder || fieldError.password || fieldError.key) return;
    busy = true;
    try {
      ondone(await openExisting(storage.value, password, secretKey, touchAvailable && touchId));
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "bad_secret_key") fieldError.key = err.message;
      else error = err.message;
    } finally {
      busy = false;
    }
  }
</script>

<Shell
  title="Open your vault"
  subtitle="Pick where it is kept, then enter your master password and Secret Key."
>
  <form class="flex flex-col gap-4" onsubmit={submit}>
    {#if error}<Alert>{error}</Alert>{/if}
    <StoragePicker {storage} folderError={fieldError.folder} />
    <TextField
      label="Master password"
      type="password"
      bind:value={password}
      error={fieldError.password}
      focus
    />
    <SecretKeyField
      bind:value={secretKey}
      error={fieldError.key}
      help="It is on your Emergency Kit. You can paste the whole key into any box."
    />
    {#if touchAvailable}<TouchIdOption bind:checked={touchId} />{/if}
    <div class="flex gap-2">
      <Button onclick={onback}>Back</Button>
      <div class="flex-1"></div>
      <Button variant="primary" type="submit" disabled={busy}>
        {busy ? "Opening..." : "Open vault"}
      </Button>
    </div>
  </form>
</Shell>
