import { expect, test } from "@playwright/test";
import { calls, expectAccessible, openApp } from "./core";

const drive = { enabled: true, state: "idle", signedIn: true, lastSyncSecsAgo: 120 };
const MISSING = { path: "/Users/test/FigLock", canRestore: false };

test.describe("iCloud Drive sync", () => {
  test("turns on with no sign-in, shows its own row, and turns off alone", async ({ page }) => {
    await openApp(page);
    await page.getByRole("button", { name: "Settings" }).click();
    const section = page.getByTestId("icloud-section");
    await expect(section.getByText("Off", { exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Sign in with Google" })).toHaveCount(0);

    await section.getByRole("button", { name: "Turn on iCloud Drive sync" }).click();
    await expect(page.getByTestId("icloud-status")).toHaveText("Last synced just now");
    await expect(page.getByTestId("sync-badge")).toHaveText("Synced just now");
    expect(await calls(page)).toContain("sync_enable_icloud");
    expect(await calls(page)).not.toContain("drive_sign_in");
    await expectAccessible(page);

    await section.getByRole("button", { name: "Turn off iCloud Drive sync" }).click();
    await expect(page.getByRole("dialog")).toContainText("stays where it is");
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Turn off iCloud Drive sync" })
      .click();
    await expect(section.getByText("Off", { exact: true })).toBeVisible();
    await expect(page.getByTestId("sync-badge")).toHaveCount(0);
    expect(await calls(page)).toContain("sync_disable_remote");
  });

  test("with iCloud Drive off it says what to switch on and cannot be turned on", async ({
    page,
  }) => {
    await openApp(page, { icloud: { ready: false } });
    await page.getByRole("button", { name: "Settings" }).click();
    const section = page.getByTestId("icloud-section");
    await expect(section.getByRole("alert")).toContainText("System Settings");
    await expect(section.getByRole("button", { name: "Turn on iCloud Drive sync" })).toBeDisabled();
    await expectAccessible(page);
  });

  test("a computer without iCloud Drive shows no iCloud section", async ({ page }) => {
    await openApp(page, { icloud: { available: false, ready: false } });
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByText("Google Drive sync", { exact: true })).toBeVisible();
    await expect(page.getByTestId("icloud-section")).toHaveCount(0);
  });

  test("Google Drive and iCloud Drive are both on, each with its own state and Sync now", async ({
    page,
  }) => {
    await openApp(page, { sync: drive, icloudOn: true });
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByTestId("last-sync")).toHaveText("Last synced 2 minutes ago");
    await expect(page.getByTestId("icloud-status")).toHaveText("Last synced 2 minutes ago");

    // Syncing iCloud alone leaves Drive's line as it was.
    await page.getByTestId("icloud-section").getByRole("button", { name: "Sync now" }).click();
    await expect(page.getByTestId("icloud-status")).toHaveText("Last synced just now");
    await expect(page.getByTestId("last-sync")).toHaveText("Last synced 2 minutes ago");

    // Turning iCloud off leaves Drive working.
    await page.getByRole("button", { name: "Turn off iCloud Drive sync" }).click();
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Turn off iCloud Drive sync" })
      .click();
    await expect(page.getByTestId("last-sync")).toHaveText("Last synced 2 minutes ago");
    await expect(
      page.getByTestId("icloud-section").getByText("Off", { exact: true }),
    ).toBeVisible();
  });

  test("while iCloud is still downloading the vault it says so and is not a warning", async ({
    page,
  }) => {
    await openApp(page, {
      icloudOn: true,
      icloudState: "waiting",
      icloudMessage: "Waiting for iCloud to finish syncing the vault to this Mac. It keeps trying.",
    });
    await expect(page.getByTestId("sync-badge")).toHaveText("Waiting for iCloud...");
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByTestId("icloud-status")).toHaveText(
      "Waiting for iCloud to finish downloading the vault...",
    );
    await expect(page.getByTestId("icloud-section").locator(".text-warning")).toHaveCount(0);
  });

  test("iCloud Drive signed out mid-use shows as not available, with words, and no data lost", async ({
    page,
  }) => {
    await openApp(page, {
      sync: drive,
      icloudOn: true,
      icloudState: "unavailable",
      icloudMessage:
        'iCloud Drive is turned off or not signed in on this Mac. Open System Settings, click your name, choose iCloud, then Drive, and turn on "Sync this Mac". Then try again.',
    });
    await expect(page.getByTestId("sync-badge")).toHaveText("iCloud Drive not available");
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByTestId("icloud-section").getByRole("alert")).toContainText(
      "System Settings",
    );
    // Google Drive is unaffected.
    await expect(page.getByTestId("last-sync")).toHaveText("Last synced 2 minutes ago");
  });

  test("the recovery screen offers iCloud Drive next to Google Drive when that remote was on", async ({
    page,
  }) => {
    await openApp(page, {
      phase: "locked",
      folderMissing: { ...MISSING, canRestore: true, canRestoreIcloud: true },
    });
    await expect(page.getByRole("button", { name: "Restore from Google Drive" })).toBeVisible();
    await page.getByRole("button", { name: "Restore from iCloud Drive folder" }).click();
    await expect(page.getByRole("heading", { name: "Unlock your vault" })).toBeVisible();
    const made = await calls(page);
    expect(made).toContain("restore_from_icloud");
    // iCloud has no sign-in, so none is asked for.
    expect(made).not.toContain("drive_sign_in");
  });

  test("iCloud restore is not offered when that remote was never on", async ({ page }) => {
    await openApp(page, { phase: "locked", folderMissing: { ...MISSING, canRestore: true } });
    await expect(
      page.getByRole("button", { name: "Restore from iCloud Drive folder" }),
    ).toHaveCount(0);
  });

  test("a new vault can sync to iCloud Drive and Google Drive together", async ({ page }) => {
    await openApp(page, { phase: "none" });
    await page.getByRole("button", { name: "Create a vault" }).click();
    await expect(page.getByRole("checkbox", { name: /Sync with iCloud Drive/ })).toBeVisible();
    await page.getByRole("checkbox", { name: /Sync with iCloud Drive/ }).check();
    await expect(page.getByTestId("icloud-panel")).toContainText("There is nothing to sign in to");
    await page.getByLabel("Password", { exact: true }).fill("correct horse battery 42");
    await page.getByLabel("Repeat password").fill("correct horse battery 42");
    await page.getByRole("button", { name: "Continue" }).click();
    const sent = await page.evaluate(() => (window as any).__core.lastCreate);
    expect(sent).toMatchObject({ icloud: true, drive: false });
  });

  test("a new vault cannot pick iCloud while iCloud Drive is off", async ({ page }) => {
    await openApp(page, { phase: "none", icloud: { ready: false } });
    await page.getByRole("button", { name: "Create a vault" }).click();
    await page.getByRole("checkbox", { name: /Sync with iCloud Drive/ }).check();
    await expect(page.getByTestId("icloud-panel").getByRole("alert")).toContainText(
      "System Settings",
    );
    await expect(page.getByRole("button", { name: "Continue" })).toBeDisabled();
  });
});
