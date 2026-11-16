<script lang="ts">
  import { Button, ConfirmDialog, Alert } from "@figlock/ui";

  import Shell from "./Shell.svelte";
  import { errorOf, forgetLockedVault } from "./vault";

  type Props = {
    /** Whether the vault has a Recovery Key is not known while locked: the flow says so if it has none. */
    onrecovery: () => void;
    onback: () => void;
    /** This Mac stopped using the vault. */
    onremoved: () => void;
  };
  let { onrecovery, onback, onremoved }: Props = $props();

  let confirming = $state(false);
  let busy = $state(false);
  let error = $state("");

  async function remove() {
    confirming = false;
    busy = true;
    error = "";
    try {
      await forgetLockedVault(true);
      onremoved();
    } catch (e) {
      error = errorOf(e).message;
    } finally {
      busy = false;
    }
  }
</script>

<Shell title="Can't unlock?" subtitle="What you can do depends on what you still have.">
  <div class="flex flex-col gap-4">
    {#if error}<Alert>{error}</Alert>{/if}

    <section class="flex flex-col gap-2" aria-labelledby="cu-password">
      <h2 id="cu-password" class="font-semibold">I forgot my master password</h2>
      <ul class="flex list-disc flex-col gap-1 pl-5 text-(length:--text-sm) text-text-secondary">
        <li>
          <strong class="font-medium text-text">Another device is unlocked</strong> (or opens with Touch
          ID): change the master password there, in Settings, then unlock here with the new one.
        </li>
        <li>
          <strong class="font-medium text-text">You have a Recovery Key</strong>: use it to choose a
          new master password. The Secret Key and Emergency Kit are made again.
        </li>
        <li>
          <strong class="font-medium text-text">Neither:</strong> the vault cannot be opened. FigLock
          has no copy of your password and no way to reset it, by design. You can stop using this vault
          on this Mac and start a new one.
        </li>
      </ul>
      <Button block onclick={onrecovery} disabled={busy}>Use my Recovery Key</Button>
    </section>

    <section class="flex flex-col gap-1" aria-labelledby="cu-key">
      <h2 id="cu-key" class="font-semibold">I lost my Secret Key</h2>
      <p class="text-(length:--text-sm) text-text-secondary">
        It is on your Emergency Kit. Another device that is unlocked shows it in Settings, and can
        make a new one. If you have neither, and no Recovery Key, the vault cannot be opened.
      </p>
    </section>

    <div class="flex flex-col gap-2">
      <Button onclick={() => (confirming = true)} disabled={busy}>
        Remove this vault from this Mac
      </Button>
      <Button onclick={onback} disabled={busy}>Back to unlock</Button>
    </div>
  </div>
</Shell>

{#if confirming}
  <ConfirmDialog
    title="Remove this vault from this Mac?"
    body="This Mac forgets the vault. Nothing is deleted from the folder or Google Drive, and you can open the vault again later with your Emergency Kit and master password."
    confirm="Remove from this Mac"
    danger
    onconfirm={remove}
    oncancel={() => (confirming = false)}
  />
{/if}
