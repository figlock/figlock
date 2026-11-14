import { expect, test } from "@playwright/test";
import { calls, openApp } from "./core";

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

  test("a vault that is not on Drive turns sync on in one click and turns it off again", async ({
    page,
  }) => {
    await openApp(page);
    await expect(page.getByTestId("sync-badge")).toHaveCount(0);
    await page.getByRole("button", { name: "Settings" }).click();
    // One click from a signed-out Mac: Google, then sync on, no second click.
    await page.getByRole("button", { name: "Turn on sync" }).click();
    await expect(page.getByRole("button", { name: "Turn off sync" })).toBeVisible();
    await expect(page.getByTestId("sync-badge")).toHaveText("Synced just now");
    expect(await calls(page)).toEqual(expect.arrayContaining(["drive_sign_in", "sync_enable"]));
    await page.getByRole("button", { name: "Turn off sync" }).click();
    const confirm = page.getByRole("alertdialog").or(page.getByRole("dialog"));
    if (await confirm.isVisible().catch(() => false))
      await confirm.getByRole("button", { name: /Turn off/ }).click();
    await expect(page.getByTestId("sync-badge")).toHaveCount(0);
  });

  test("turn off sync, sign out of Google, then turn sync on again with a fresh sign-in", async ({
    page,
  }) => {
    await openApp(page, { sync: { ...synced, signedInSecsAgo: 86_400 } });
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByText("Signed in to Google")).toBeVisible();
    await expect(page.getByTestId("google-signin")).toContainText("Signed in 24 hours ago");

    // Turning sync off keeps the Google sign-in, and says so.
    await page.getByRole("button", { name: "Turn off sync" }).click();
    await expect(page.getByRole("dialog")).toContainText("stays signed in to Google");
    await page.getByRole("dialog").getByRole("button", { name: "Turn off sync" }).click();
    await expect(page.getByRole("button", { name: "Turn on sync" })).toBeVisible();
    await expect(page.getByText("Signed in to Google")).toBeVisible();

    // Sign out needs a confirmation; cancelling does nothing.
    await page.getByRole("button", { name: "Sign out of Google" }).click();
    await page.getByRole("dialog").getByRole("button", { name: "Cancel" }).click();
    expect(await calls(page)).not.toContain("sync_sign_out");
    await expect(page.getByText("Signed in to Google")).toBeVisible();
    await page.getByRole("button", { name: "Sign out of Google" }).click();
    await page.getByRole("dialog").getByRole("button", { name: "Sign out of Google" }).click();
    await expect(page.getByText("Signed in to Google")).toHaveCount(0);

    // Turn on sync signs in again and turns sync on, with no second click.
    await page.getByRole("button", { name: "Turn on sync" }).click();
    await expect(page.getByRole("button", { name: "Turn off sync" })).toBeVisible();
    await expect(page.getByText("Signed in to Google")).toBeVisible();
  });

  test("use a different Google account in one flow", async ({ page }) => {
    await openApp(page, { sync: { ...synced, signedInSecsAgo: 60 } });
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("button", { name: "Use a different Google account" }).click();
    await expect(page.getByRole("dialog")).toContainText("different FigLock vault");
    await page.getByRole("button", { name: "Sign out and choose another" }).click();
    await expect(page.getByRole("button", { name: "Turn off sync" })).toBeVisible();
    const made = await calls(page);
    expect(made.indexOf("sync_sign_out")).toBeLessThan(made.indexOf("drive_sign_in"));
    expect(made.indexOf("drive_sign_in")).toBeLessThan(made.lastIndexOf("sync_enable"));
  });
});
