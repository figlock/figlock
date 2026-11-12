<script lang="ts">
  import { Alert, Button } from "@figlock/ui";

  import ChangeCredentialsDialog from "./ChangeCredentialsDialog.svelte";
  import { changeCredentials, type Changed } from "./credentials";
  import NewKitDialog from "./NewKitDialog.svelte";
  import { errorOf, lock, vaultState } from "./vault";
  import { getWorkspace } from "./workspace";

  const w = getWorkspace();

  let changing = $state<"password" | "key" | null>(null);
  let keyKept = $state(false);
  /** The new Secret Key, while its Emergency Kit has not been saved. */
  let newKey = $state("");
  let done = $state("");

  /** Asks whether this Mac keeps the Secret Key first, so the dialog only asks for it when it must. */
  async function start(mode: "password" | "key") {
    done = "";
    try {
      keyKept = (await vaultState()).secretKeyKept;
    } catch {
      keyKept = false;
    }
    changing = mode;
  }

  async function change(
    password: string,
    secretKey: string,
    newPassword: string | null,
  ): Promise<Changed> {
    const rotate = changing === "key";
    let changed: Changed;
    try {
      changed = await changeCredentials(password, secretKey, newPassword, rotate);
    } catch (e) {
      // A locked vault goes back to the unlock screen; anything else is shown in the dialog.
      const err = errorOf(e);
      if (err.code === "locked" || err.code === "change_uncertain" || err.code === "needs_unlock") {
        changing = null;
        if (err.code === "needs_unlock") await lock();
        if (err.code === "locked") w.relock();
        else w.relock(err.message);
      }
      throw e;
    }
    changing = null;
    if (changed.secretKey) newKey = changed.secretKey;
    else
      done =
        "Your master password is changed. Your other devices will ask for the new password the next time they unlock.";
    return changed;
  }
</script>

<section aria-labelledby="credentials-title" class="grid gap-3">
  <h3 id="credentials-title" class="font-semibold">Password and Secret Key</h3>
  <p class="text-text-secondary">
    Change either one without removing a device. To lock out a lost device, revoke it below.
  </p>
  {#if done}<Alert tone="info">{done}</Alert>{/if}
  <div class="flex flex-wrap gap-2">
    <Button onclick={() => start("password")}>Change master password</Button>
    <Button onclick={() => start("key")}>Rotate Secret Key</Button>
  </div>
</section>

{#if changing}
  <ChangeCredentialsDialog
    mode={changing}
    {keyKept}
    onchange={change}
    oncancel={() => (changing = null)}
  />
{/if}

{#if newKey}
  <NewKitDialog
    secretKey={newKey}
    ondone={() => {
      newKey = "";
      done =
        "Your Secret Key is rotated and the new Emergency Kit is confirmed. Your other devices will ask for the new key the next time they unlock.";
    }}
  />
{/if}
