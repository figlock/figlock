<script lang="ts">
  import { Alert, Button, TextField } from "@figlock/ui";

  import { endRotation } from "./credentials";
  import { checkKitKey, errorOf, saveEmergencyKit } from "./vault";

  type Props = {
    /** The new Secret Key a rotation just made. */
    secretKey: string;
    ondone: () => void;
  };

  let { secretKey, ondone }: Props = $props();

  const titleId = $props.id();
  let dialog = $state<HTMLDialogElement>();
  let shown = $state(false);
  let kitSaved = $state(false);
  let typed = $state("");
  let busy = $state(false);
  let error = $state("");
  let fieldError = $state("");

  $effect(() => {
    dialog?.showModal();
  });

  async function saveKit() {
    error = "";
    try {
      if (await saveEmergencyKit()) kitSaved = true;
    } catch (e) {
      error = errorOf(e).message;
    }
  }

  async function done(e: Event) {
    e.preventDefault();
    error = "";
    fieldError = "";
    busy = true;
    try {
      if (!(await checkKitKey(typed))) {
        fieldError = "These do not match the end of your new Secret Key. Check and try again.";
        return;
      }
      await endRotation();
      ondone();
    } catch (e) {
      error = errorOf(e).message;
    } finally {
      busy = false;
    }
  }
</script>

<!-- No cancel: the old key is already gone, so the new one has to be saved before this closes. -->
<dialog
  bind:this={dialog}
  aria-labelledby={titleId}
  oncancel={(e) => e.preventDefault()}
  class="m-auto w-[calc(100vw-2rem)] max-w-120 rounded-(--radius-lg) border border-border bg-surface-raised p-0 text-text shadow-xl backdrop:bg-black/40"
>
  <form class="flex flex-col gap-4 p-6" onsubmit={done}>
    <div class="flex flex-col gap-2">
      <h2 id={titleId} class="text-(length:--text-md) leading-6 font-semibold">
        Save your new Secret Key
      </h2>
      <p class="text-text-secondary">
        The old Secret Key no longer opens this vault. Replace your Emergency Kit with the new one.
      </p>
    </div>
    {#if error}<Alert>{error}</Alert>{/if}
    <Alert tone="warning">
      FigLock cannot recover this key. Keep your Emergency Kit somewhere safe, apart from your
      master password.
    </Alert>
    <div class="rounded-(--radius-sm) bg-surface-sunken p-4">
      <p
        class="flex flex-wrap gap-x-3 font-mono text-(length:--text-md) leading-6"
        data-testid="new-secret-key"
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
      error={fieldError}
      help="Type the last two groups of the new Secret Key."
      placeholder="XXXX-XXXX"
      mono
    />
    <div class="flex">
      <div class="flex-1"></div>
      <Button variant="primary" type="submit" disabled={busy || typed.trim() === ""}>
        {busy ? "Checking..." : "Done"}
      </Button>
    </div>
  </form>
</dialog>
