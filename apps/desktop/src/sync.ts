import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type SyncState =
  "off" | "idle" | "syncing" | "offline" | "sign_in" | "lost" | "foreign" | "failed";

/** How Google Drive sync is going, with nothing secret in it. */
export type SyncView = {
  enabled: boolean;
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
};

export const syncStatus = () => invoke<SyncView>("sync_status");
/** `reupload` is the person's yes to uploading everything again after the vault was lost from Drive. */
export const syncNow = (reupload = false) => invoke<SyncView>("sync_now", { reupload });
export const syncEnable = () => invoke<SyncView>("sync_enable");
/**
 * Signs out of Google and turns sync off. The vault on this Mac and the copy in Drive are kept. `revoked` is
 * false when Google could not be reached, so access may still be there.
 */
export const syncSignOut = () => invoke<{ revoked: boolean }>("sync_sign_out");
export const syncDisable = () => invoke<SyncView>("sync_disable");

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
  switch (v.state) {
    case "syncing":
      return { text: "Syncing...", attention: false };
    case "offline":
      return { text: "Offline, will sync later", attention: false };
    case "sign_in":
      return { text: "Sign in to keep syncing", attention: true };
    case "lost":
      return { text: "Vault missing from Drive", attention: true };
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
