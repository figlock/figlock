import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** What the window may know about the vault. Keys and plaintext stay in the Rust side (ADR-0020). */
export type VaultState = {
  /** `none`: nothing set up on this device. */
  phase: "none" | "locked" | "unlocked";
  version: string;
  /** Touch ID is turned on for this vault. */
  quickUnlock: boolean;
  /** This Mac can ask for Touch ID. */
  touchAvailable: boolean;
  /** This Mac keeps the Secret Key, so unlocking asks for the master password only (ADR-0027). */
  secretKeyKept: boolean;
  /** How the kept key is protected, when there is one. */
  secretKeyProtection: "secure-enclave" | "keychain" | null;
  autolockSecs: number;
};

/** A failed command: a code to branch on and a message that is safe to show. */
export type CmdError = { code: string; message: string };

export const errorOf = (e: unknown): CmdError =>
  typeof e === "object" && e !== null && "message" in e && "code" in e
    ? (e as CmdError)
    : { code: "failed", message: "Something went wrong. Try again." };

export const vaultState = () => invoke<VaultState>("vault_state");
export const suggestedFolder = () => invoke<string | null>("suggested_folder");
export const chooseFolder = () => invoke<string | null>("choose_folder");

/**
 * Where a vault is kept (ADR-0026): always a folder on this Mac, and optionally synced to the hidden app
 * folder in the person's Google Drive. `drive` without a `path` means the default folder.
 */
export type Storage = { path?: string; drive?: boolean };

/** `canSignIn` is false in a build without a Google client (source builds, ADR-0021). */
export type DriveStatus = { canSignIn: boolean; signedIn: boolean };

export const driveStatus = () => invoke<DriveStatus>("drive_status");
/** Opens the browser and resolves when the person finishes signing in, or the sign-in times out. */
export const driveSignIn = () => invoke<DriveStatus>("drive_sign_in");
/** `revoked` is false when Google could not be reached, so access may still be there. */
export const driveSignOut = () => invoke<{ revoked: boolean }>("drive_sign_out");

export const createVault = (storage: Storage, password: string) =>
  invoke<{ secretKey: string }>("create_vault", { storage, password });
/** Resolves `false` when the person cancels the save dialog. */
export const saveEmergencyKit = () => invoke<boolean>("save_emergency_kit");
export const checkKitKey = (typed: string) => invoke<boolean>("check_kit_key", { typed });
export const finishCreate = (touchId: boolean) => invoke<VaultState>("finish_create", { touchId });
export const cancelCreate = () => invoke<void>("cancel_create");

export const openExisting = (
  storage: Storage,
  password: string,
  secretKey: string,
  touchId: boolean,
) => invoke<VaultState>("open_existing", { storage, password, secretKey, touchId });

/** Resolves the fingerprint to compare with the trusted device. */
export const joinStart = (storage: Storage, code: string) =>
  invoke<string>("join_start", { storage, code });
export const joinAnswered = () => invoke<boolean>("join_answered");
export const joinCancel = () => invoke<void>("join_cancel");
export const joinFinish = (password: string, touchId: boolean) =>
  invoke<VaultState>("join_finish", { password, touchId });

/** `secretKey` is left out when this Mac keeps it; `needs_secret_key` says the kept one did not open the vault. */
export const unlock = (password: string, secretKey?: string) =>
  invoke<VaultState>("unlock", { password, secretKey: secretKey || null });
/** The kept Secret Key, after Touch ID or the device password. */
export const showSecretKey = () => invoke<string>("show_secret_key");
export const forgetSecretKey = () => invoke<VaultState>("forget_secret_key");
export const quickUnlock = () => invoke<VaultState>("quick_unlock");
export const lock = () => invoke<void>("lock");
export const activity = () => invoke<void>("activity");

/** Called when the vault locks by itself (idle, or the Mac woke from sleep). */
export const onAutoLock = (handler: () => void) => listen("figlock://locked", handler);
