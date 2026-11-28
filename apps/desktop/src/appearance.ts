/** Light, Dark or follow the Mac. The choice is a window setting, not vault data, so it lives in this window's storage. */
export type Appearance = "system" | "light" | "dark";

export const APPEARANCES: { value: Appearance; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

const KEY = "figlock.appearance";

const isAppearance = (v: unknown): v is Appearance =>
  v === "system" || v === "light" || v === "dark";

export function savedAppearance(): Appearance {
  try {
    const v = localStorage.getItem(KEY);
    return isAppearance(v) ? v : "system";
  } catch {
    return "system";
  }
}

/** Show `choice` now: the tokens read `data-theme`, and no attribute means follow the Mac. */
export function applyAppearance(choice: Appearance) {
  const root = document.documentElement;
  if (choice === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", choice);
}

/** Keep `choice` for the next launch and show it. */
export function setAppearance(choice: Appearance) {
  try {
    if (choice === "system") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, choice);
  } catch {
    // Storage can be unavailable; the choice then lasts until the window closes.
  }
  applyAppearance(choice);
}
