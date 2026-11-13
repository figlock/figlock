import { expect, test } from "@playwright/test";
import { openApp } from "./core";

const synced = { enabled: true, state: "idle", signedIn: true, lastSyncSecsAgo: 120 };

test.describe("sync", () => {
  test("the sidebar says when the vault last synced", async ({ page }) => {
    await openApp(page, { sync: synced });
    await expect(page.getByTestId("sync-badge")).toHaveText("Synced 2 minutes ago");
  });

  test("Sync now runs and the status moves to just now", async ({ page }) => {
    await openApp(page, { sync: synced });
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByText("Last synced 2 minutes ago")).toBeVisible();
    await page.getByRole("button", { name: "Sync now" }).click();
    await expect(page.getByText("Last synced just now")).toBeVisible();
  });

  test("a sync that needs sign-in is called out with a warning", async ({ page }) => {
    await openApp(page, {
      sync: { ...synced, state: "sign_in", message: "Sign in to Google again to keep syncing." },
    });
    await expect(page.getByTestId("sync-badge")).toHaveText("Sign in to keep syncing");
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByText("Sign in to Google again to keep syncing.")).toBeVisible();
  });

  test("a vault that is not on Drive signs in, turns sync on, and turns it off again", async ({
    page,
  }) => {
    await openApp(page);
    await expect(page.getByTestId("sync-badge")).toHaveCount(0);
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("button", { name: "Sign in with Google" }).click();
    await page.getByRole("button", { name: "Turn on sync" }).click();
    await expect(page.getByTestId("sync-badge")).toHaveText("Synced just now");
    await page.getByRole("button", { name: "Turn off sync" }).click();
    const confirm = page.getByRole("alertdialog").or(page.getByRole("dialog"));
    if (await confirm.isVisible().catch(() => false))
      await confirm.getByRole("button", { name: /Turn off/ }).click();
    await expect(page.getByTestId("sync-badge")).toHaveCount(0);
  });
});
