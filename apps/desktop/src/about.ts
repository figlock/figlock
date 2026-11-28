import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { IconName } from "@figlock/ui";

/** The pages on figlock.com the About screen opens inside the app, in the order iOS lists them (FIG-241). */
export const ABOUT_LINKS: { id: string; title: string; icon: IconName }[] = [
  { id: "protects", title: "How FigLock protects your data", icon: "shield-check" },
  { id: "privacy", title: "Privacy policy", icon: "file-text" },
  { id: "terms", title: "Terms", icon: "file-text" },
  { id: "support", title: "Support", icon: "life-buoy" },
];

/** Show a figlock.com page in its own window. The app core only knows these ids, not addresses. */
export const openAboutPage = (id: string) => invoke<void>("open_about_page", { pageId: id });

/** The person chose FigLock > About FigLock in the app menu. */
export const onOpenAbout = (handler: () => void) =>
  listen("figlock://open-about", handler).catch(() => () => {});
