import {
  driveCancelSignIn,
  driveSignIn,
  driveSignOut,
  driveStatus,
  errorOf,
  icloudStatus,
  type DriveStatus,
  type Storage,
} from "./vault";
import type { IcloudView } from "./sync";

/** Where the vault comes from when opening one: a folder, the person's Google Drive, or their iCloud Drive. */
export type StorageKind = "folder" | "drive" | "icloud";

/**
 * Where the person is putting, or finding, the vault.
 *
 * Creating (`mode: "create"`): a folder on this Mac, and whether to also sync it to Google Drive, iCloud Drive
 * or both (ADR-0043).
 * Opening or joining (`mode: "source"`): a folder that holds the vault, Google Drive or iCloud Drive, which
 * download a copy into this Mac's default folder and keep it synced.
 */
export class StorageChoice {
  /** Source mode only. */
  kind = $state<StorageKind>("folder");
  folder = $state("");
  /** Create mode only: also keep a synced copy in Google Drive. */
  syncDrive = $state(false);
  /** Create mode only: also keep a synced copy in iCloud Drive. */
  syncIcloud = $state(false);
  /** `null` until iCloud Drive's state is known. It has no sign-in: only whether it is on. */
  icloud = $state<IcloudView | null>(null);
  /** `null` until Drive's state is known. */
  drive = $state<DriveStatus | null>(null);
  signingIn = $state(false);
  /** A sign-in that failed, in words for the person. */
  signInError = $state("");
  /** Set after signing out: Google could not confirm the access is gone. */
  notRevoked = $state(false);

  constructor(readonly mode: "create" | "source" = "source") {}

  /** Whether Drive is part of the choice. */
  usesDrive = $derived(this.mode === "create" ? this.syncDrive : this.kind === "drive");
  /** Whether iCloud Drive is part of the choice. */
  usesIcloud = $derived(this.mode === "create" ? this.syncIcloud : this.kind === "icloud");
  /** What the commands take. */
  value = $derived<Storage>(
    this.mode === "create"
      ? { path: this.folder, drive: this.syncDrive, icloud: this.syncIcloud }
      : this.kind === "drive"
        ? { drive: true }
        : this.kind === "icloud"
          ? { icloud: true }
          : { path: this.folder },
  );
  /** Enough is chosen to go on: a folder (or Drive, or iCloud), a sign-in when Drive is used, iCloud Drive on. */
  ready = $derived(
    (this.mode === "create" || this.kind === "folder" ? this.folder !== "" : true) &&
      (!this.usesDrive || this.drive?.signedIn === true) &&
      (!this.usesIcloud || this.icloud?.ready === true),
  );

  async refresh() {
    try {
      this.drive = await driveStatus();
    } catch {
      this.drive = { canSignIn: false, signedIn: false };
    }
    try {
      this.icloud = await icloudStatus();
    } catch {
      this.icloud = { available: false, ready: false, folderExists: false, help: null };
    }
  }

  async signIn() {
    this.signInError = "";
    this.notRevoked = false;
    this.signingIn = true;
    try {
      this.drive = await driveSignIn();
    } catch (e) {
      if (errorOf(e).code !== "sign_in_cancelled") this.signInError = errorOf(e).message;
    } finally {
      this.signingIn = false;
    }
  }

  /** Stop waiting for the Google page. */
  cancelSignIn() {
    return driveCancelSignIn().catch(() => {});
  }

  async signOut() {
    this.signInError = "";
    try {
      this.notRevoked = !(await driveSignOut()).revoked;
    } catch (e) {
      this.signInError = errorOf(e).message;
    }
    await this.refresh();
  }
}
