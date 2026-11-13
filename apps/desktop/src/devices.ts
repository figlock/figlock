import { invoke } from "@tauri-apps/api/core";

/** One row of the device list. Names and fingerprints only, never keys. */
export type Device = {
  /** The full id; sent back to revoke. */
  id: string;
  /** Unknown for a revoked device. */
  name: string | null;
  /** Short code to compare with what the other device shows. */
  fingerprint: string;
  thisDevice: boolean;
  revoked: boolean;
};

export const listDevices = () => invoke<Device[]>("devices");

/** `newPassword` also changes the master password in the same step, for a device that may be compromised. */
/** A blank `secretKey` means "use the one this Mac keeps"; `needs_secret_key` says there is none to use. */
export const revokeDevice = (
  device: string,
  password: string,
  secretKey: string,
  newPassword: string | null,
) =>
  invoke<Device[]>("revoke_device", {
    device,
    password,
    secretKey: secretKey || null,
    newPassword,
  });

/** An open invite: the code to type or scan on the new device. Shown once, never stored. */
export type Invite = {
  /** `XXXX-XXXX-XXXX-XXXX-XXXX`. */
  code: string;
  /** QR modules of the code, one string of `1` and `0` per row. */
  qr: string[];
  expiresAtMs: number;
};

/** `request` means the new device answered; its name and fingerprint are then set. */
export type PairingStatus = {
  state: "none" | "waiting" | "request" | "expired";
  expiresAtMs: number | null;
  deviceName: string | null;
  fingerprint: string | null;
};

/** Opens an invite, replacing any that is open. */
export const addDeviceStart = () => invoke<Invite>("add_device_start");
export const addDeviceStatus = () => invoke<PairingStatus>("add_device_status");
/**
 * Approves the new device. With no `secretKey`, Touch ID releases the Secret Key this Mac keeps; otherwise the
 * master password and the typed key must open the vault.
 */
export const addDeviceApprove = (password: string, secretKey: string) =>
  invoke<Device[]>("add_device_approve", {
    password: password || null,
    secretKey: secretKey || null,
  });
export const addDeviceCancel = () => invoke<void>("add_device_cancel");
