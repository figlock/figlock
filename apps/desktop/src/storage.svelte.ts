import {
  driveSignIn,
  driveSignOut,
  driveStatus,
  errorOf,
  type DriveStatus,
  type Storage,
} from "./vault";

/** Where the vault comes from when opening one: a folder, or the person's Google Drive. */
export type StorageKind = "folder" | "drive";

/**
 * Where the person is putting, or finding, the vault.
 *
 * Creating (`mode: "create"`): a folder on this Mac, and whether to also sync it to Google Drive.
 * Opening or joining (`mode: "source"`): a folder that holds the vault, or Google Drive, which downloads a
 * copy into this Mac's default folder and keeps it synced.
 */
export class StorageChoice {
  /** Source mode only. */
  kind = $state<StorageKind>("folder");
  folder = $state("");
  /** Create mode only: also keep a synced copy in Google Drive. */
  syncDrive = $state(false);
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
  /** What the commands take. */
  value = $derived<Storage>(
    this.mode === "create"
      ? { path: this.folder, drive: this.syncDrive }
      : this.kind === "drive"
        ? { drive: true }
        : { path: this.folder },
  );
  /** Enough is chosen to go on: a folder (or Drive), and a sign-in when Drive is used. */
  ready = $derived(
    (this.mode === "create" || this.kind === "folder" ? this.folder !== "" : true) &&
      (!this.usesDrive || this.drive?.signedIn === true),
  );

  async refresh() {
    try {
      this.drive = await driveStatus();
    } catch {
      this.drive = { canSignIn: false, signedIn: false };
    }
  }

  async signIn() {
    this.signInError = "";
    this.notRevoked = false;
    this.signingIn = true;
    try {
      this.drive = await driveSignIn();
    } catch (e) {
      this.signInError = errorOf(e).message;
    } finally {
      this.signingIn = false;
    }
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
