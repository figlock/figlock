import { test } from "@playwright/test";
import { GITHUB, NOTE, STRIPE, expectAccessible, openApp } from "./core";

const items = [GITHUB, STRIPE, NOTE];
const synced = { enabled: true, state: "idle", signedIn: true, lastSyncSecsAgo: 120 };

// WCAG 2.2 AA with axe on each screen a person can reach, in the order they meet them.
test.describe("accessibility (axe, WCAG 2.2 AA)", () => {
  test("first run: welcome, create, open existing, join", async ({ page }) => {
    await openApp(page, { phase: "none" } as never);
    await page.evaluate(() => ((window as any).__core.phase = "none"));
    await page.reload();
    await expectAccessible(page);
    for (const name of [
      "Create a vault",
      "Open an existing vault",
      "Add this Mac with an invite code",
    ]) {
      await page.getByRole("button", { name }).click();
      await expectAccessible(page);
      await page.getByRole("button", { name: /Back/ }).first().click();
    }
  });

  test("unlock, with an error showing", async ({ page }) => {
    await openApp(page, { phase: "locked" });
    await expectAccessible(page);
    await page.getByLabel("Master password").fill("wrong");
    await page.getByRole("button", { name: "Unlock" }).click();
    await page.getByRole("alert").waitFor();
    await expectAccessible(page);
  });

  test("the list: empty, full, searched, and recently deleted", async ({ page }) => {
    await openApp(page);
    await expectAccessible(page);
    await openApp(page, { items, sync: synced });
    await expectAccessible(page);
    await page.getByRole("searchbox").fill("zzz");
    await expectAccessible(page);
    await page.getByRole("button", { name: "Recently deleted" }).click();
    await expectAccessible(page);
  });

  test("an item, its editor, and the new item picker", async ({ page }) => {
    await openApp(page, { items });
    await page.getByRole("button", { name: /^GitHub/ }).click();
    await expectAccessible(page);
    await page.getByRole("button", { name: "Show password" }).click();
    await expectAccessible(page);
    await page.getByRole("button", { name: "Edit" }).click();
    await expectAccessible(page);
    await page.getByRole("button", { name: "Cancel" }).click();
    await page.getByRole("button", { name: "Delete", exact: true }).click();
    await expectAccessible(page);
  });

  test("every item type's editor", async ({ page }) => {
    await openApp(page);
    await page.getByRole("banner").getByRole("button", { name: "New item" }).click();
    await expectAccessible(page);
    const kinds = ["Login", "API token", "Env bundle", "SSH key", "Secure note", "Document"];
    for (const kind of kinds) {
      await page.getByRole("button", { name: new RegExp(`^${kind}\\b`) }).click();
      await expectAccessible(page);
      await page.getByRole("button", { name: "Back" }).first().click();
    }
  });

  test("generator, password health and settings", async ({ page }) => {
    await openApp(page, { items, sync: synced });
    await page.getByRole("button", { name: "Generator" }).click();
    await expectAccessible(page);
    await page.getByRole("radio", { name: "Passphrase" }).check();
    await expectAccessible(page);
    await page.getByRole("button", { name: "Password health" }).click();
    await expectAccessible(page);
    await page.getByRole("button", { name: "Settings" }).click();
    await expectAccessible(page);
  });

  test("settings with sync needing attention", async ({ page }) => {
    await openApp(page, {
      sync: { ...synced, state: "sign_in", message: "Sign in to Google again to keep syncing." },
    });
    await page.getByRole("button", { name: "Settings" }).click();
    await expectAccessible(page);
  });
});
