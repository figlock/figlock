import {
  onUpdateProgress,
  restartApp,
  updateCheck,
  updateInstall,
  type Progress,
  type UpdateView,
} from "./updates";
import { errorOf } from "./vault";

/**
 * What the window knows about app updates. One instance for the whole app: the check at launch and the one in
 * Settings share it. Nothing is installed without the person asking.
 */
export class Updates {
  view = $state<UpdateView | null>(null);
  checking = $state(false);
  installing = $state(false);
  progress = $state<Progress | null>(null);
  /** The last failure, in words for the person. */
  error = $state("");

  /** `quiet` is the check at launch: it says nothing when it fails, and nothing when there is no update. */
  async check(quiet = false) {
    if (this.checking || this.installing) return;
    this.error = "";
    this.checking = true;
    try {
      this.view = await updateCheck();
    } catch (e) {
      if (!quiet) this.error = errorOf(e).message;
    } finally {
      this.checking = false;
    }
  }

  async install() {
    if (this.installing || this.view?.state !== "available") return;
    this.error = "";
    this.installing = true;
    this.progress = null;
    const off = onUpdateProgress((p) => (this.progress = p)).catch(() => () => {});
    try {
      this.view = await updateInstall();
    } catch (e) {
      this.error = errorOf(e).message;
      // The feed may have moved on since the check (the kept copy of it is stale now), so ask again; the
      // person then retries against what is published now. The failure stays on screen.
      try {
        this.view = await updateCheck();
      } catch {
        // Offline: the failure above is the news, and Install stays available.
      }
    } finally {
      this.installing = false;
      void off.then((f) => f());
    }
  }

  async restart() {
    try {
      await restartApp();
    } catch (e) {
      this.error = errorOf(e).message;
    }
  }

  /** A newer version is waiting, or already installed and waiting for a restart. */
  get pending() {
    return this.view?.state === "available" || this.view?.state === "ready";
  }
}

export const updates = new Updates();
