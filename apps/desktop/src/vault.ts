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

export const createVault = (folder: string, password: string) =>
  invoke<{ secretKey: string }>("create_vault", { folder, password });
/** Resolves `false` when the person cancels the save dialog. */
export const saveEmergencyKit = () => invoke<boolean>("save_emergency_kit");
export const checkKitKey = (typed: string) => invoke<boolean>("check_kit_key", { typed });
export const finishCreate = (touchId: boolean) => invoke<VaultState>("finish_create", { touchId });
export const cancelCreate = () => invoke<void>("cancel_create");

export const openExisting = (
  folder: string,
  password: string,
  secretKey: string,
  touchId: boolean,
) => invoke<VaultState>("open_existing", { folder, password, secretKey, touchId });

/** Resolves the fingerprint to compare with the trusted device. */
export const joinStart = (folder: string, code: string) =>
  invoke<string>("join_start", { folder, code });
export const joinAnswered = () => invoke<boolean>("join_answered");
export const joinCancel = () => invoke<void>("join_cancel");
export const joinFinish = (password: string, touchId: boolean) =>
  invoke<VaultState>("join_finish", { password, touchId });

export const unlock = (password: string, secretKey: string) =>
  invoke<VaultState>("unlock", { password, secretKey });
export const quickUnlock = () => invoke<VaultState>("quick_unlock");
export const lock = () => invoke<void>("lock");
export const activity = () => invoke<void>("activity");

/** Called when the vault locks by itself (idle, or the Mac woke from sleep). */
export const onAutoLock = (handler: () => void) => listen("figlock://locked", handler);
