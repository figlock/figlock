import {
  driveSignIn,
  driveSignOut,
  driveStatus,
  errorOf,
  type DriveStatus,
  type Storage,
} from "./vault";

export type StorageKind = "folder" | "drive";

/** Where the person is putting, or finding, the vault: a folder, or their Google Drive. */
export class StorageChoice {
  kind = $state<StorageKind>("folder");
  folder = $state("");
  /** `null` until Drive's state is known. */
  drive = $state<DriveStatus | null>(null);
  signingIn = $state(false);
  /** A sign-in that failed, in words for the person. */
  signInError = $state("");
  /** Set after signing out: Google could not confirm the access is gone. */
  notRevoked = $state(false);

  /** What the commands take. */
  value = $derived<Storage>(
    this.kind === "drive" ? { kind: "drive" } : { kind: "folder", path: this.folder },
  );
  /** Enough is chosen to go on: a folder, or a Drive sign-in. */
  ready = $derived(this.kind === "folder" ? this.folder !== "" : this.drive?.signedIn === true);

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
