import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type SyncState =
  | "off"
  | "idle"
  | "syncing"
  | "offline"
  | "sign_in"
  | "lost"
  | "foreign"
  | "unavailable"
  | "waiting"
  | "failed";

/** One place the vault syncs to, with its own state: one failing never holds up another (ADR-0043). */
export type RemoteView = {
  /** Stable name to act on it: `drive`, or `folder:<path>`. */
  id: string;
  kind: "drive" | "icloud" | "folder";
  /** "Google Drive", "iCloud Drive", or the folder's name. */
  name: string;
  path: string | null;
  state: Exclude<SyncState, "off">;
  lastSyncSecsAgo: number | null;
  /** Words for the person when this place needs attention. */
  message: string | null;
};

/** iCloud Drive on this Mac. It has no sign-in: it is a folder the system syncs. */
export type IcloudView = {
  /** An Apple computer, where iCloud Drive can exist. */
  available: boolean;
  /** iCloud Drive is on and signed in. */
  ready: boolean;
  /** `iCloud Drive/FigLock` already exists. */
  folderExists: boolean;
  /** What to switch on in System Settings when it is not ready. */
  help: string | null;
};

/** How sync is going to every place, with nothing secret in it. */
export type SyncView = {
  /** Sync is on to at least one place. */
  enabled: boolean;
  /** All places in one word, for the sidebar. */
  state: SyncState;
  signedIn: boolean;
  /** How long ago this Mac signed in to Google; the sign-in carries no account name. */
  signedInSecsAgo: number | null;
  /** This build can sign in to Google at all (source builds cannot, ADR-0021). */
  canSignIn: boolean;
  lastSyncSecsAgo: number | null;
  /** Words for the person when something needs their attention. */
  message: string | null;
  /** Two devices changed the vault's settings at once and one copy was kept. */
  fork: boolean;
  /** Another device changed the password, keys or devices: lock and unlock to take it in. */
  needsUnlock: boolean;
  localFolder: string | null;
  remotes: RemoteView[];
  icloud: IcloudView;
};

export const syncStatus = () => invoke<SyncView>("sync_status");
/**
 * Sync now: every place, or only `remote` (its id). `reupload` is the person's yes to uploading everything again
 * after the vault was lost from a place.
 */
export const syncNow = (reupload = false, remote?: string) =>
  invoke<SyncView>("sync_now", { remote: remote ?? null, reupload });
export const syncEnable = () => invoke<SyncView>("sync_enable");
/** Turns iCloud Drive sync on: no sign-in; makes `iCloud Drive/FigLock` when it is not there. */
export const syncEnableIcloud = () => invoke<SyncView>("sync_enable_icloud");
/** Stops syncing to one place (its id), leaving the others. Nothing already stored there is touched. */
export const syncDisableRemote = (remote: string) =>
  invoke<SyncView>("sync_disable_remote", { remote });
/**
 * Signs out of Google and turns sync off. The vault on this Mac and the copy in Drive are kept. `revoked` is
 * false when Google could not be reached, so access may still be there.
 */
export const syncSignOut = () => invoke<{ revoked: boolean }>("sync_sign_out");
export const syncDisable = () => invoke<SyncView>("sync_disable");

/** What the person types to delete the vault from Google Drive. The app core checks it too (ADR-0035). */
export const DELETE_DRIVE_PHRASE = "delete from Google Drive";
/**
 * Permanently deletes the vault's copy from Google Drive, after the typed phrase. The vault on this Mac is kept
 * and sync ends up off.
 */
export const syncDeleteRemote = (confirmation: string) =>
  invoke<SyncView>("sync_delete_remote", { confirmation });

/** Called after every sync the app core runs, so the window can reload what it shows. */
export const onSynced = (handler: () => void) => listen("figlock://synced", handler);

/** "just now", "3 minutes ago", for the last sync. */
export function ago(secs: number | null): string {
  if (secs === null) return "not yet";
  if (secs < 20) return "just now";
  if (secs < 90) return "a minute ago";
  const m = Math.round(secs / 60);
  if (m < 60) return `${m} minutes ago`;
  const h = Math.round(m / 60);
  if (h < 36) return h === 1 ? "an hour ago" : `${h} hours ago`;
  return `${Math.round(h / 24)} days ago`;
}

/** The short line under the sidebar: what the person most needs to know about sync right now. */
export function badge(v: SyncView): { text: string; attention: boolean } {
  // The place that needs attention, so the line can name it.
  const at = v.remotes.find((r) => r.state === v.state);
  const where = !at || at.kind === "drive" ? "Drive" : at.name;
  switch (v.state) {
    case "syncing":
      return { text: "Syncing...", attention: false };
    case "offline":
      return { text: "Offline, will sync later", attention: false };
    case "sign_in":
      return { text: "Sign in to keep syncing", attention: true };
    case "lost":
      return { text: `Vault missing from ${where}`, attention: true };
    case "unavailable":
      return { text: `${where} not available`, attention: true };
    case "waiting":
      return { text: "Waiting for iCloud...", attention: false };
    case "foreign":
      return { text: "Sync paused", attention: true };
    case "failed":
      return { text: "Sync problem", attention: true };
    default:
      return v.needsUnlock
        ? { text: "Unlock to see new changes", attention: true }
        : { text: `Synced ${ago(v.lastSyncSecsAgo)}`, attention: false };
  }
}

/** What the Settings list says next to Sync: one plain word. */
export function syncValue(v: SyncView | null): string {
  if (!v) return "";
  if (v.remotes.length === 0) return "Off";
  switch (v.state) {
    case "syncing":
      return "Syncing";
    case "offline":
      return "Offline";
    case "idle":
    case "waiting":
      return v.needsUnlock ? "Needs attention" : "Synced";
    default:
      return "Needs attention";
  }
}
