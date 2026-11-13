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
  /** The vault has a Recovery Key. Only while unlocked. */
  recoveryEnabled: boolean;
  /** The new Secret Key of a rotation whose Emergency Kit step is not finished, while unlocked. */
  pendingKit: string | null;
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

/** A new Recovery Key as shown once: one code, or cards of which `threshold` open the vault. */
export type RecoveryMaterial = {
  mode: "code" | "shares";
  code: string | null;
  /** The words of each card. */
  cards: string[][];
  threshold: number;
  count: number;
  /** For cards: which word (1-based) of each card the person types back. For a code, the last two groups. */
  challenge: number[];
};

export const recoveryPrepare = (shares: boolean, threshold: number, count: number) =>
  invoke<RecoveryMaterial>("recovery_prepare", { shares, threshold, count });
/** Resolves `false` when what was typed does not match, so the key is not confirmed. */
export const recoveryCheck = (answers: string[]) => invoke<boolean>("recovery_check", { answers });
/** Resolves `false` when the person cancels the save dialog. */
export const saveRecoveryKit = () => invoke<boolean>("save_recovery_kit");
export const recoveryCancel = () => invoke<void>("recovery_cancel");
/** Setup: no Recovery Key, acknowledging that a forgotten password cannot be recovered. */
export const recoverySkip = (acknowledged: boolean) =>
  invoke<void>("recovery_skip", { acknowledged });
/** Setup: use the confirmed key; it is written with the vault. */
export const recoveryUseDraft = () => invoke<void>("recovery_use_draft");
/** Settings: write the confirmed key into the vault. A blank `secretKey` means the one this Mac keeps. */
export const recoveryEnable = (password: string, secretKey: string) =>
  invoke<void>("recovery_enable", { password, secretKey: secretKey || null });
export const recoveryDisable = (password: string, secretKey: string) =>
  invoke<void>("recovery_disable", { password, secretKey: secretKey || null });

/** What one Recovery Key card says: how many cards open the vault, and this card's number. */
export type CardInfo = { needed: number; number: number };
/** What a Recovery Key opened, shown before anything is changed. */
export type Found = { name: string; items: number; lastChangeMs: number | null; devices: number };

export const forgotCardInfo = (text: string) => invoke<CardInfo>("forgot_card_info", { text });
/** Opens the vault in `storage` with the code (`shares` false, one answer) or the cards. Stores nothing. */
export const forgotFind = (storage: Storage, answers: string[], shares: boolean) =>
  invoke<Found>("forgot_find", { storage, answers, shares });
/** Chooses the new master password and makes the new Secret Key, to be saved in a new Emergency Kit. */
export const forgotPrepare = (newPassword: string, newRecoveryKey: boolean) =>
  invoke<{ secretKey: string }>("forgot_prepare", { newPassword, newRecoveryKey });
/** Stores the new credentials and unlocks. Needs the new Emergency Kit confirmed first. */
export const forgotCommit = (touchId: boolean) => invoke<VaultState>("forgot_commit", { touchId });
export const forgotCancel = () => invoke<void>("forgot_cancel");
