import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/**
 * `unavailable`: this build has no update key (a development build). `ready`: the update is installed and
 * FigLock needs a restart to run it.
 */
export type UpdateState = "unavailable" | "up_to_date" | "available" | "ready";

export type UpdateView = {
  state: UpdateState;
  current: string;
  version: string | null;
  notes: string | null;
};

export type Progress = { downloaded: number; total: number | null };

export const updateCheck = () => invoke<UpdateView>("update_check");
export const updateInstall = () => invoke<UpdateView>("update_install");
export const restartApp = () => invoke<void>("restart_app");

/** Download progress of an install, as the app core reports it. */
export const onUpdateProgress = (handler: (p: Progress) => void) =>
  listen<Progress>("update-progress", (e) => handler(e.payload));
