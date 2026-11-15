import { expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import path from "node:path";

export const PASSWORD = "correct horse battery";
export const KEY = "FL1-ABCD-EFGH-JKMN-PQRS-TVWX-YZ23-4567-89AB";

export type Seed = {
  kind: string;
  fields: Record<string, string>;
  secrets?: Record<string, string>;
  ageDays?: number;
};
export type Options = {
  phase?: "none" | "locked" | "unlocked";
  items?: Seed[];
  sync?: Record<string, unknown>;
  health?: unknown;
  cli?: { state: string };
  update?: { version: string; notes?: string };
  updateUnavailable?: boolean;
  folderMissing?: { path: string; canRestore: boolean };
  pickedFolder?: string;
  /** The Google sign-in waits for the browser until the person cancels. */
  signInHangs?: boolean;
  /** Restore from Google Drive fails with this error (code and words). */
  restoreFails?: { code: string; message: string };
  /** The auto-lock time this Mac starts with, in seconds. */
  autolockSecs?: number;
  /** Another device rotated the Secret Key, so the one this Mac keeps is out of date. */
  secretKeyStale?: boolean;
  /** The vault removed this Mac at this time (Unix milliseconds): every change is refused. */
  removedAtMs?: number;
  /** This Mac keeps no Secret Key. */
  keyNotKept?: boolean;
  /** The person cancels the save dialog of the Emergency Kit. */
  kitDialogCancelled?: boolean;
  /** Deleting the Drive copy fails with this error (code and words). */
  deleteFails?: { code: string; message: string };
};

/** The Secret Key another device made, in the fake app core's stale-key scenarios. */
export const NEW_KEY = "FL1-WXYZ-2345-6789-ABCD-EFGH-JKMN-PQRS-TVWX";

export const GITHUB: Seed = {
  kind: "login",
  fields: { title: "GitHub", username: "octo@example.test", "url/a": "https://github.test" },
  secrets: { password: "Tr0ub4dor&3-demo" },
};
export const STRIPE: Seed = {
  kind: "token",
  fields: { title: "Stripe key", provider: "Stripe" },
  secrets: { token: "sk_test_demo_000000" },
};
export const NOTE: Seed = { kind: "note", fields: { title: "Wifi at home" }, secrets: {} };

/** Open the app with the fake app core installed first. */
export async function openApp(page: Page, options: Options = {}) {
  await page.addInitScript((o) => ((window as any).__FAKE__ = o), options);
  await page.addInitScript({ path: path.join(import.meta.dirname, "fake-core.js") });
  await page.goto("/");
}

export const calls = (page: Page) => page.evaluate(() => (window as any).__core.calls as string[]);

/** WCAG 2.2 AA, with no violations of any impact. */
export async function expectAccessible(page: Page) {
  const result = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
    .analyze();
  const brief = result.violations.map(
    (v) => `${v.id}: ${v.help} (${v.nodes.length}) ${v.nodes[0]?.target}`,
  );
  expect(brief, "axe violations").toEqual([]);
}
