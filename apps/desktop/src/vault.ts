import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { IcloudView } from "./sync";

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
  /** The Secret Key was changed on another device, so the one this Mac keeps opens nothing (ADR-0034). */
  secretKeyStale: boolean;
  /** How the kept key is protected, when there is one. */
  secretKeyProtection: "secure-enclave" | "keychain" | null;
  /** The person stopped keeping the key on this Mac; it is asked for at every unlock until they keep it again (FIG-142). */
  secretKeyOptedOut: boolean;
  /** This unlocked session holds the Secret Key it was unlocked with, until the vault locks (ADR-0030). */
  secretKeyHeld: boolean;
  autolockSecs: number;
  /** The vault has a Recovery Key. Only while unlocked. */
  recoveryEnabled: boolean;
  /** The new Secret Key of a rotation whose Emergency Kit step is not finished, while unlocked. */
  pendingKit: string | null;
  /** This Mac knows a vault but its local folder is gone: offer a way forward, ask for no credentials (FIG-118). */
  folderMissing: MissingFolder | null;
  /**
   * When the vault removed this Mac (Unix milliseconds), known once it is unlocked: every change is refused, the
   * copy stays readable (FIG-135).
   */
  removedAtMs: number | null;
};

export type MissingFolder = {
  /** Where this Mac expects the vault. */
  path: string;
  /** Sync was on, so a copy may be in Google Drive. */
  canRestore: boolean;
  /** Sync to iCloud Drive was on, so a copy may be in its `FigLock` folder. */
  canRestoreIcloud: boolean;
};

/** A failed command: a code to branch on and a message that is safe to show. */
export type CmdError = { code: string; message: string };

export const errorOf = (e: unknown): CmdError =>
  typeof e === "object" && e !== null && "message" in e && "code" in e
    ? (e as CmdError)
    : { code: "failed", message: "Something went wrong. Try again." };

export const vaultState = () => invoke<VaultState>("vault_state");

/** Who answers agents on the broker socket: this window, a broker started in a terminal, or nobody. */
export type BrokerMode = "off" | "hosting" | "cli";
export const brokerStatus = () => invoke<BrokerMode>("broker_status");
export const brokerTakeOver = () => invoke<BrokerMode>("broker_take_over");
export const suggestedFolder = () => invoke<string | null>("suggested_folder");
export const chooseFolder = () => invoke<string | null>("choose_folder");

/**
 * Where a vault is kept (ADR-0026, ADR-0043): always a folder on this Mac, and optionally synced to the hidden
 * app folder in the person's Google Drive, the `FigLock` folder in iCloud Drive, or both. `drive` or `icloud`
 * without a `path` means the default folder.
 */
export type Storage = { path?: string; drive?: boolean; icloud?: boolean };

/** `canSignIn` is false in a build without a Google client (source builds, ADR-0021). */
export type DriveStatus = { canSignIn: boolean; signedIn: boolean };

export const driveStatus = () => invoke<DriveStatus>("drive_status");
/** Opens the browser and resolves when the person finishes signing in, or the sign-in times out. */
export const driveSignIn = () => invoke<DriveStatus>("drive_sign_in");
/** Ends a sign-in that is waiting for the browser; that sign-in then fails with the code `sign_in_cancelled`. */
export const driveCancelSignIn = () => invoke<void>("drive_cancel_sign_in");
/** `revoked` is false when Google could not be reached, so access may still be there. */
export const driveSignOut = () => invoke<{ revoked: boolean }>("drive_sign_out");

/** Whether iCloud Drive can be used on this Mac. Needs no vault, so setup screens can ask. */
export const icloudStatus = () => invoke<IcloudView>("icloud_status");

/** The folder was moved: point this Mac at it. Refused when it holds no vault. */
export const locateFolder = (folder: string) => invoke<void>("locate_folder", { folder });
/** Downloads the vault from Google Drive into a new local folder; sign in first. */
export const restoreFromDrive = (folder?: string) =>
  invoke<void>("restore_from_drive", { folder: folder ?? null });
/** Copies the vault from the `FigLock` folder in iCloud Drive into a new local folder. */
export const restoreFromIcloud = (folder?: string) =>
  invoke<void>("restore_from_icloud", { folder: folder ?? null });
/** This Mac stops using the vault; nothing in the folder or Drive is deleted. Needs the person's yes. */
/** A locked Mac that cannot get back in stops using its vault (FIG-139, R3). */
export const forgetLockedVault = (confirmed: boolean) =>
  invoke<void>("forget_locked_vault", { confirmed });
export const forgetVault = (confirmed: boolean) => invoke<void>("forget_vault", { confirmed });

export const createVault = (storage: Storage, password: string) =>
  invoke<{ secretKey: string }>("create_vault", { storage, password });
/** Resolves `false` when the person cancels the save dialog. */
export const saveEmergencyKit = () => invoke<boolean>("save_emergency_kit");
/**
 * Save the Emergency Kit again, made now from the Secret Key this Mac keeps (Touch ID or the device password
 * first). When none is kept, pass the master password and the Secret Key typed from the old kit. Resolves
 * `false` when the person cancels the save dialog (FIG-134).
 */
export const saveEmergencyKitAgain = (password?: string, secretKey?: string) =>
  invoke<boolean>("save_emergency_kit_again", {
    password: password ?? null,
    secretKey: secretKey ?? null,
  });
/** The vault removed this Mac: stop using it here. Nothing is deleted from the folder or Google Drive. */
export const leaveVault = () => invoke<void>("leave_vault");
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
/** Keep the Secret Key another device made, checked against the vault with the master password. */
export const updateSecretKey = (password: string, secretKey: string) =>
  invoke<VaultState>("update_secret_key", { password, secretKey });
export const forgetSecretKey = () => invoke<VaultState>("forget_secret_key");
/** Keep the Secret Key on this Mac again. Blank fields use the key this unlocked window holds. */
export const keepSecretKey = (password: string, secretKey: string) =>
  invoke<VaultState>("keep_secret_key", { password, secretKey });
/** How long the vault waits without activity before it locks, in seconds. Applies at once. */
export const setAutolock = (secs: number) => invoke<VaultState>("set_autolock", { secs });
export const quickUnlock = () => invoke<VaultState>("quick_unlock");
/** Stop unlocking with Touch ID; the Secret Key stays kept on this Mac. */
export const turnTouchIdOff = () => invoke<VaultState>("turn_touch_id_off");
/** Unlock with Touch ID from now on. The master password confirms; the Secret Key only when none is on hand. */
export const turnTouchIdOn = (password: string, secretKey: string) =>
  invoke<VaultState>("turn_touch_id_on", { password, secretKey });
export const lock = () => invoke<void>("lock");
export const activity = () => invoke<void>("activity");

/** Called when the vault locks by itself (idle, or the Mac woke from sleep). */
export const onAutoLock = (handler: () => void) => listen("figlock://locked", handler);

/** A new Recovery Key as shown once: one code. */
export type RecoveryMaterial = {
  code: string;
};

export const recoveryPrepare = () => invoke<RecoveryMaterial>("recovery_prepare");
/** Resolves `false` when what was typed does not match, so the key is not confirmed. */
/** `answer` is the last two groups of the code, typed back. */
export const recoveryCheck = (answer: string) => invoke<boolean>("recovery_check", { answer });
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

/** What a Recovery Key opened, shown before anything is changed. */
export type Found = { name: string; items: number; lastChangeMs: number | null; devices: number };

/** Opens the vault in `storage` with the Recovery Key code. Stores nothing. */
export const forgotFind = (storage: Storage, code: string) =>
  invoke<Found>("forgot_find", { storage, code });
/** Chooses the new master password and makes the new Secret Key, to be saved in a new Emergency Kit. */
export const forgotPrepare = (newPassword: string, newRecoveryKey: boolean) =>
  invoke<{ secretKey: string }>("forgot_prepare", { newPassword, newRecoveryKey });
/** Stores the new credentials and unlocks. Needs the new Emergency Kit confirmed first. */
export const forgotCommit = (touchId: boolean) => invoke<VaultState>("forgot_commit", { touchId });
export const forgotCancel = () => invoke<void>("forgot_cancel");
