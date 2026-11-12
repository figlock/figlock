import { driveSignIn, errorOf } from "./vault";
import { onSynced, syncDisable, syncEnable, syncNow, syncStatus, type SyncView } from "./sync";

/** What the window knows about Google Drive sync, kept fresh while the vault is open. */
export class SyncStatus {
  view = $state<SyncView | null>(null);
  /** An action is running (turning sync on, syncing now). */
  busy = $state(false);
  /** The last action's failure, in words for the person. */
  error = $state("");

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

  /** Sign in to Google again, then sync. */
  async signInAgain() {
    this.error = "";
    this.busy = true;
    try {
      await driveSignIn();
    } catch (e) {
      this.error = errorOf(e).message;
      this.busy = false;
      return;
    }
    this.busy = false;
    await this.now();
  }
}
