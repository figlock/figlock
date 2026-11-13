<script lang="ts">
  import { Alert, Badge, Button, Icon } from "@figlock/ui";

  import AddDeviceDialog from "./AddDeviceDialog.svelte";
  import { listDevices, revokeDevice, type Device } from "./devices";
  import RevokeDialog from "./RevokeDialog.svelte";
  import { errorOf, lock, vaultState } from "./vault";
  import { failure, getWorkspace } from "./workspace";

  const w = getWorkspace();

  let devices = $state<Device[] | null>(null);
  let error = $state("");
  let done = $state("");
  let revoking = $state<Device | null>(null);
  let keyKept = $state(false);
  /** The session holds the key and the device does not keep one, so nothing prompts (ADR-0030). */
  let keyHeld = $state(false);
  let adding = $state(false);

  async function load() {
    try {
      devices = await listDevices();
    } catch (e) {
      error = failure(w, e);
    }
  }

  // Loads on mount, and again after each sync, which can bring in a device paired elsewhere.
  $effect(() => {
    void w.sync.view?.lastSyncSecsAgo;
    void load();
  });

  /** Asks whether the Secret Key is on hand first, so the dialog only asks for it when it must. */
  async function readKey() {
    try {
      const s = await vaultState();
      // Kept by this Mac (Touch ID releases it) or held by this session (ADR-0030): no typed key either way.
      keyKept = s.secretKeyKept || s.secretKeyHeld;
      keyHeld = s.secretKeyHeld;
    } catch {
      keyKept = false;
      keyHeld = false;
    }
  }

  async function startRevoke(d: Device) {
    done = "";
    await readKey();
    revoking = d;
  }

  async function startAdd() {
    done = "";
    await readKey();
    adding = true;
  }

  async function added(list: Device[], name: string) {
    adding = false;
    devices = list;
    error = "";
    done = `${name} is added. On ${name}, enter your master password to finish.`;
  }

  async function addFailed(code: string, message: string) {
    adding = false;
    if (code === "locked") w.relock();
    else if (code === "needs_unlock") {
      await lock();
      w.relock(message);
    } else error = message;
  }

  async function revoke(password: string, secretKey: string, newPassword: string | null) {
    if (!revoking) return;
    const target = revoking;
    try {
      devices = await revokeDevice(target.id, password, secretKey, newPassword);
    } catch (e) {
      // A locked vault goes back to the unlock screen; anything else is shown in the dialog.
      const err = errorOf(e);
      if (err.code === "locked" || err.code === "revoke_uncertain" || err.code === "needs_unlock") {
        revoking = null;
        if (err.code === "needs_unlock") await lock();
        if (err.code === "locked") w.relock();
        else w.relock(err.message);
      }
      throw e;
    }
    revoking = null;
    error = "";
    done = newPassword
      ? `${target.name ?? "The device"} is revoked and the master password is changed. Your other devices will ask for a full unlock with the new password.`
      : `${target.name ?? "The device"} is revoked.`;
  }
</script>

<section aria-labelledby="devices-title" class="grid gap-3">
  <div class="flex items-start justify-between gap-3">
    <div class="grid gap-1">
      <h3 id="devices-title" class="font-semibold">Devices</h3>
      <p class="text-text-secondary">The devices that can open this vault.</p>
    </div>
    <Button onclick={() => void startAdd()}><Icon name="plus" />Add a device</Button>
  </div>

  {#if error}<Alert>{error}</Alert>{/if}
  {#if done}<Alert tone="info">{done}</Alert>{/if}

  {#if devices === null}
    {#if !error}<p class="text-text-secondary">Checking...</p>{/if}
  {:else}
    <ul class="grid gap-2" aria-label="Devices">
      {#each devices as d (d.id)}
        <li
          class="flex items-center gap-3 rounded-(--radius-md) border border-border bg-surface p-3"
        >
          <Icon
            name={d.revoked ? "shield-alert" : "shield-check"}
            class={d.revoked ? "text-text-tertiary" : "text-success"}
          />
          <div class="min-w-0 flex-1">
            <p class="truncate font-medium">
              {d.name ?? "Revoked device"}
              {#if d.thisDevice}<span class="font-normal text-text-secondary">
                  (this Mac)</span
                >{/if}
            </p>
            <p class="font-mono text-(length:--text-xs) text-text-tertiary">{d.fingerprint}</p>
          </div>
          {#if d.revoked}
            <Badge>Revoked</Badge>
          {:else if !d.thisDevice}
            <button
              type="button"
              class="inline-flex h-(--control-height-md) items-center justify-center rounded-(--radius-sm) border border-border px-4 text-(length:--text-sm) font-medium text-danger transition-colors duration-(--duration-fast) hover:bg-danger-subtle"
              onclick={() => void startRevoke(d)}
              aria-label="Revoke {d.name ?? 'device'}"
            >
              Revoke
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

{#if adding}
  <AddDeviceDialog
    {keyKept}
    {keyHeld}
    onerror={addFailed}
    onadded={added}
    onclose={() => (adding = false)}
  />
{/if}

{#if revoking}
  <RevokeDialog device={revoking} {keyKept} onrevoke={revoke} oncancel={() => (revoking = null)} />
{/if}
