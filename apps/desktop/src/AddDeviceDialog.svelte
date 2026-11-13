<script lang="ts">
  import { Alert, Button, SecretKeyField, TextField } from "@figlock/ui";
  import { onMount } from "svelte";

  import {
    addDeviceApprove,
    addDeviceCancel,
    addDeviceStart,
    addDeviceStatus,
    type Device,
    type Invite,
    type PairingStatus,
  } from "./devices";
  import { errorOf } from "./vault";

  type Props = {
    /** This Mac keeps the Secret Key, so Touch ID approves. Otherwise the password and key are typed. */
    keyKept: boolean;
    /** The session holds the key and nothing prompts for it, so there is no Touch ID step (ADR-0030). */
    keyHeld?: boolean;
    /** The vault locked or another device changed it: the dialog has nothing left to do. */
    onerror: (code: string, message: string) => void;
    /** The new device was approved; `devices` is the refreshed list. */
    onadded: (devices: Device[], name: string) => void;
    onclose: () => void;
  };
  let { keyKept, keyHeld = false, onerror, onadded, onclose }: Props = $props();

  /** The new device is asked about every 3 seconds, as the pairing spec says. */
  const POLL_MS = 3000;

  const titleId = $props.id();
  let dialog = $state<HTMLDialogElement>();
  let invite = $state<Invite | null>(null);
  let status = $state<PairingStatus | null>(null);
  let now = $state(Date.now());
  let error = $state("");
  let busy = $state(false);
  let password = $state("");
  let secretKey = $state("");
  /** The key fields show when none is kept, or when Touch ID did not release it. */
  let typeKey = $state(false);
  const askKey = $derived(typeKey || !keyKept);
  const askPassword = $derived(!askKey && keyHeld);
  let fieldError = $state<{ password?: string; key?: string }>({});

  const expired = $derived(status?.state === "expired");
  const requested = $derived(status?.state === "request");
  const left = $derived(invite ? Math.max(0, Math.ceil((invite.expiresAtMs - now) / 1000)) : 0);
  const clock = $derived(`${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`);

  /** Dark modules as one path, with a 4 module quiet zone around them. */
  const qrPath = $derived.by(() => {
    if (!invite) return "";
    let d = "";
    invite.qr.forEach((row, y) => {
      for (let x = 0; x < row.length; x++) {
        if (row[x] === "1") d += `M${x + 4} ${y + 4}h1v1h-1z`;
      }
    });
    return d;
  });
  const qrSize = $derived((invite?.qr.length ?? 0) + 8);

  async function start() {
    error = "";
    status = null;
    invite = null;
    try {
      invite = await addDeviceStart();
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "locked") onerror(err.code, err.message);
      else error = err.message;
    }
  }

  async function poll() {
    if (!invite || expired || busy) return;
    try {
      status = await addDeviceStatus();
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "locked") onerror(err.code, err.message);
    }
  }

  onMount(() => {
    dialog?.showModal();
    void start();
    const tick = setInterval(() => (now = Date.now()), 1000);
    const timer = setInterval(() => void poll(), POLL_MS);
    return () => {
      clearInterval(tick);
      clearInterval(timer);
    };
  });

  async function close() {
    // An open invite must not outlive the dialog.
    try {
      await addDeviceCancel();
    } catch {
      // The vault locked meanwhile; the invite went with the session.
    }
    onclose();
  }

  async function approve(e: Event) {
    e.preventDefault();
    error = "";
    fieldError = {};
    if (askKey) {
      if (!password) fieldError.password = "Enter your master password.";
      if (secretKey.split("-").length !== 9 || secretKey.split("-").some((g) => g.length === 0)) {
        fieldError.key = "This Secret Key looks incomplete. It has eight groups after FL1.";
      }
      if (Object.keys(fieldError).length > 0) return;
    } else if (askPassword && !password) {
      fieldError.password = "Enter your master password.";
      return;
    }
    busy = true;
    const name = status?.deviceName ?? "The new device";
    try {
      const devices = await addDeviceApprove(
        askKey || askPassword ? password : "",
        askKey ? secretKey : "",
      );
      onadded(devices, name);
    } catch (e) {
      const err = errorOf(e);
      if (err.code === "needs_secret_key") {
        typeKey = true;
        error = keyKept
          ? "Touch ID did not release the Secret Key. Enter your master password and Secret Key to approve."
          : err.message;
      } else if (err.code === "bad_secret_key") fieldError.key = err.message;
      else if (err.code === "wrong_credentials") error = err.message;
      else if (err.code === "expired" || err.code === "no_invite") {
        status = { state: "expired", expiresAtMs: null, deviceName: null, fingerprint: null };
        error = "";
      } else if (err.code === "locked" || err.code === "needs_unlock") {
        onerror(err.code, err.message);
        return;
      } else error = err.message;
      busy = false;
    }
  }

  async function decline() {
    await close();
  }
</script>

<dialog
  bind:this={dialog}
  aria-labelledby={titleId}
  oncancel={(e) => {
    e.preventDefault();
    if (!busy) void close();
  }}
  class="m-auto w-[calc(100vw-2rem)] max-w-120 rounded-(--radius-lg) border border-border bg-surface-raised p-0 text-text shadow-xl backdrop:bg-black/40"
>
  <div class="flex flex-col gap-4 p-6">
    {#if requested && status}
      <form class="flex flex-col gap-4" onsubmit={approve}>
        <div class="flex flex-col gap-2">
          <h2 id={titleId} class="text-(length:--text-md) leading-6 font-semibold">
            Add "{status.deviceName}"?
          </h2>
          <p class="text-text-secondary">
            Check that the new device shows this same fingerprint. If it does not, cancel: someone
            else may have found your code.
          </p>
        </div>
        <p
          class="rounded-(--radius-md) border border-border bg-surface-sunken px-4 py-3 text-center font-mono text-(length:--text-lg) tracking-wider"
          data-testid="fingerprint"
        >
          {status.fingerprint}
        </p>
        {#if error}<Alert>{error}</Alert>{/if}
        {#if askKey}
          <TextField
            label="Master password"
            type="password"
            bind:value={password}
            error={fieldError.password}
            disabled={busy}
            focus
          />
          <SecretKeyField bind:value={secretKey} error={fieldError.key} disabled={busy} />
        {:else if askPassword}
          <p class="text-text-secondary">
            Approving sends your Secret Key to the new device. It still needs your master password.
          </p>
          <TextField
            label="Master password"
            type="password"
            bind:value={password}
            error={fieldError.password}
            disabled={busy}
            focus
          />
        {:else}
          <p class="text-text-secondary">
            Approving asks for Touch ID, then sends your Secret Key to the new device. It still
            needs your master password.
          </p>
        {/if}
        <div class="grid grid-cols-2 gap-2">
          <Button onclick={decline} disabled={busy}>Cancel</Button>
          <Button variant="primary" type="submit" disabled={busy}>
            {busy ? "Approving..." : askKey || askPassword ? "Approve" : "Approve with Touch ID"}
          </Button>
        </div>
      </form>
    {:else if expired}
      <div class="flex flex-col gap-2">
        <h2 id={titleId} class="text-(length:--text-md) leading-6 font-semibold">
          This code has expired
        </h2>
        <p class="text-text-secondary">
          A code works for 10 minutes and for one device. Make a new one to try again.
        </p>
      </div>
      {#if error}<Alert>{error}</Alert>{/if}
      <div class="grid grid-cols-2 gap-2">
        <Button onclick={close}>Close</Button>
        <Button variant="primary" onclick={start}>New code</Button>
      </div>
    {:else}
      <div class="flex flex-col gap-2">
        <h2 id={titleId} class="text-(length:--text-md) leading-6 font-semibold">Add a device</h2>
        <p class="text-text-secondary">
          On the new device, choose Add this device and enter this code. It works once and expires
          in 10 minutes.
        </p>
      </div>
      {#if error}<Alert>{error}</Alert>{/if}
      {#if invite}
        <div class="flex flex-col items-center gap-3">
          <p
            class="font-mono text-(length:--text-lg) font-semibold tracking-wider select-all"
            data-testid="invite-code"
          >
            {invite.code}
          </p>
          <svg
            viewBox="0 0 {qrSize} {qrSize}"
            role="img"
            aria-label="QR code of the invite code"
            class="size-44 rounded-(--radius-sm)"
            shape-rendering="crispEdges"
          >
            <rect width={qrSize} height={qrSize} fill="#fff" />
            <path d={qrPath} fill="#000" />
          </svg>
          <p class="text-(length:--text-sm) text-text-secondary" role="status">
            Waiting for the new device... expires in {clock}
          </p>
        </div>
      {:else if !error}
        <p class="text-text-secondary">Making a code...</p>
      {/if}
      <Button onclick={close}>Cancel</Button>
    {/if}
  </div>
</dialog>
