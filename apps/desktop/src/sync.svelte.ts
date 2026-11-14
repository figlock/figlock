import { driveSignIn, errorOf } from "./vault";
import {
  onSynced,
  syncDisable,
  syncEnable,
  syncNow,
  syncSignOut,
  syncStatus,
  type SyncView,
} from "./sync";

/** What the window knows about Google Drive sync, kept fresh while the vault is open. */
export class SyncStatus {
  view = $state<SyncView | null>(null);
  /** An action is running (turning sync on, syncing now). */
  busy = $state(false);
  /** While turning sync on: waiting for the Google sign-in, then turning sync on in the app core. */
  step = $state<"" | "google" | "enabling">("");
  /** The last action's failure, in words for the person. */
  error = $state("");
  /** What the last sign-out could not do, in words (Google could not be reached to revoke the access). */
  notice = $state("");

  constructor(private onsynced: () => void = () => {}) {}

  async refresh() {
    try {
      this.view = await syncStatus();
    } catch {
      // Sync is an extra: a window that cannot read its state just shows none.
    }
  }

  /** Follow the app core: on every sync it runs, and every 30 seconds so "synced 2 minutes ago" stays true. */
  start() {
    void this.refresh();
    const timer = setInterval(() => void this.refresh(), 30_000);
    const off = onSynced(() => {
      void this.refresh();
      this.onsynced();
    }).catch((e) => {
      // Without events the window still works (it polls), but say so: a missing permission is a bug.
      console.error("sync events are unavailable", e);
      return () => {};
    });
    return () => {
      clearInterval(timer);
      void off.then((f) => f());
    };
  }

  async #act(run: () => Promise<SyncView>) {
    this.error = "";
    this.busy = true;
    try {
      this.view = await run();
      this.onsynced();
    } catch (e) {
      this.error = errorOf(e).message;
      await this.refresh();
    } finally {
      this.busy = false;
    }
  }

  now(reupload = false) {
    return this.#act(() => syncNow(reupload));
  }
  enable() {
    return this.#act(syncEnable);
  }
  disable() {
    return this.#act(syncDisable);
  }

  /**
   * Turn sync on in one action: sign in to Google when this Mac is not signed in, then turn sync on and run the
   * first sync. When only the second step fails the sign-in is kept, so the next click retries just that step.
   */
  async turnOn() {
    this.error = "";
    this.busy = true;
    try {
      if (!this.view?.signedIn) {
        this.step = "google";
        try {
          await driveSignIn();
        } catch (e) {
          this.error = errorOf(e).message;
          return;
        } finally {
          // Never show a state older than the sign-in, whether or not the next step works.
          await this.refresh();
        }
      }
      this.step = "enabling";
      try {
        this.view = await syncEnable();
        this.onsynced();
      } catch (e) {
        this.error = `You are signed in to Google, but sync is not on yet. ${errorOf(e).message}`;
        await this.refresh();
      }
    } finally {
      this.step = "";
      this.busy = false;
    }
  }

  /** Sign out of Google (which also turns sync off). Resolves true when this Mac is signed out. */
  async signOut() {
    this.error = "";
    this.notice = "";
    this.busy = true;
    try {
      const { revoked } = await syncSignOut();
      if (!revoked) {
        this.notice =
          "Signed out on this Mac, but Google could not be reached to remove FigLock's access. You can remove it in your Google account under Security > Third-party apps.";
      }
      return true;
    } catch (e) {
      this.error = errorOf(e).message;
      return false;
    } finally {
      this.busy = false;
      await this.refresh();
      this.onsynced();
    }
  }

  /** Sign out, then go through Turn on sync, so a different Google account can be picked. */
  async useDifferentAccount() {
    if (await this.signOut()) await this.turnOn();
  }

  /** Sign in to Google again after the sign-in expired, then resume the sync that was blocked. */
  async signInAgain() {
    this.error = "";
    this.busy = true;
    this.step = "google";
    try {
      await driveSignIn();
    } catch (e) {
      this.error = errorOf(e).message;
      return;
    } finally {
      this.step = "";
      this.busy = false;
      await this.refresh();
    }
    await this.now();
  }
}
