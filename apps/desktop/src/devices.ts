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
export const revokeDevice = (
  device: string,
  password: string,
  secretKey: string,
  newPassword: string | null,
) => invoke<Device[]>("revoke_device", { device, password, secretKey, newPassword });
