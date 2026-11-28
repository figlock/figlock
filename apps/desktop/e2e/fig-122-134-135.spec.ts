import { expect, test } from "@playwright/test";
import { GITHUB, KEY, PASSWORD, calls, expectAccessible, openApp } from "./core";

const synced = { enabled: true, state: "idle", signedIn: true, lastSyncSecsAgo: 120 };
const PHRASE = "delete from Google Drive";
const REMOVED = Date.UTC(2026, 10, 15, 12);

test.describe("a Mac the vault removed (FIG-135)", () => {
  test("is told in plain words, with the ways forward", async ({ page }) => {
    await openApp(page, { items: [GITHUB], removedAtMs: REMOVED });
    await expect(
      page.getByRole("heading", { name: "This Mac was removed from the vault" }),
    ).toBeVisible();
    await expect(page.getByText(/Changes made here are no longer accepted/).first()).toBeVisible();
    await expect(page.getByRole("alert")).toContainText("Your copy stays readable");
    await expect(page.getByRole("button", { name: /Add this Mac again/ })).toBeVisible();
    await expect(page.getByRole("button", { name: "Keep reading my copy" })).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Remove this vault from this Mac" }),
    ).toBeVisible();
    await expectAccessible(page);
  });

  test("keeps the copy readable behind a banner that leads back", async ({ page }) => {
    await openApp(page, { items: [GITHUB], removedAtMs: REMOVED });
    await page.getByRole("button", { name: "Keep reading my copy" }).click();
    await expect(page.getByRole("button", { name: /^GitHub/ })).toBeVisible();
    const banner = page.getByTestId("removed-banner");
    await expect(banner).toContainText("was removed from the vault on");
    await expect(banner).toContainText("Your copy stays readable");
    await expectAccessible(page);
    await banner.getByRole("button", { name: "See options" }).click();
    await expect(
      page.getByRole("heading", { name: "This Mac was removed from the vault" }),
    ).toBeVisible();
  });

  test("removing the vault from this Mac asks first and then starts over", async ({ page }) => {
    await openApp(page, { items: [GITHUB], removedAtMs: REMOVED });
    await page.getByRole("button", { name: "Remove this vault from this Mac" }).click();
    expect(await calls(page)).not.toContain("leave_vault");
    await expectAccessible(page);
    await page
      .getByRole("dialog", { name: "Remove this vault from this Mac?" })
      .getByRole("button", { name: "Remove from this Mac" })
      .click();
    await expect(page.getByRole("button", { name: "Create a vault" })).toBeVisible();
    expect(await calls(page)).toContain("leave_vault");
  });

  test("adding this Mac again leaves the vault and goes to the invite code", async ({ page }) => {
    await openApp(page, { items: [GITHUB], removedAtMs: REMOVED });
    await page.getByRole("button", { name: /Add this Mac again/ }).click();
    await page
      .getByRole("dialog", { name: "Add this Mac again?" })
      .getByRole("button", { name: "Continue" })
      .click();
    await expect(page.getByLabel(/invite code/i)).toBeVisible();
    expect(await calls(page)).toContain("leave_vault");
  });

  test("a vault that did not remove this Mac shows no banner", async ({ page }) => {
    await openApp(page, { items: [GITHUB] });
    await expect(page.getByRole("button", { name: /^GitHub/ })).toBeVisible();
    await expect(page.getByTestId("removed-banner")).toHaveCount(0);
  });
});

test.describe("save the Emergency Kit again (FIG-134)", () => {
  test("a Mac that keeps the key saves it from Settings", async ({ page }) => {
    await openApp(page, { sync: synced });
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByTestId("settings-security").click();
    await expectAccessible(page);
    await page.getByRole("button", { name: "Save Emergency Kit" }).click();
    await expect(page.getByTestId("kit-saved")).toContainText("Saved");
    const saved = await page.evaluate(() => (window as any).__core.kitsSaved as string[]);
    expect(saved).toEqual([KEY]);
  });

  test("cancelling the save dialog saves nothing", async ({ page }) => {
    await openApp(page, { sync: synced, kitDialogCancelled: true });
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByTestId("settings-security").click();
    await page.getByRole("button", { name: "Save Emergency Kit" }).click();
    await expect(page.getByTestId("kit-saved")).toHaveCount(0);
    expect(await page.evaluate(() => (window as any).__core.kitsSaved.length)).toBe(0);
  });

  test("a Mac with no kept key asks for the password and key, and refuses a wrong pair", async ({
    page,
  }) => {
    await openApp(page, { sync: synced, keyNotKept: true });
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByTestId("settings-security").click();
    await page.getByRole("button", { name: "Save Emergency Kit" }).click();
    const dialog = page.getByRole("dialog", { name: "Save your Emergency Kit" });
    await expectAccessible(page);

    await dialog.getByLabel("Password").fill("not it at all");
    await dialog.getByLabel("Secret Key group 1 of 8").click();
    await page.keyboard.insertText(KEY);
    await dialog.getByRole("button", { name: "Save kit" }).click();
    await expect(dialog.getByRole("alert")).toContainText("do not match this vault");
    expect(await page.evaluate(() => (window as any).__core.kitsSaved.length)).toBe(0);

    await dialog.getByLabel("Password").fill(PASSWORD);
    await dialog.getByRole("button", { name: "Save kit" }).click();
    await expect(page.getByTestId("kit-saved")).toBeVisible();
    expect(await page.evaluate(() => (window as any).__core.kitsSaved)).toEqual([KEY]);
  });
});

test.describe("delete the vault from Google Drive (FIG-122)", () => {
  test("needs the phrase typed, and leaves sync off and the vault here", async ({ page }) => {
    await openApp(page, { items: [GITHUB], sync: synced });
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByTestId("settings-sync").click();
    await page.getByRole("button", { name: "Delete from Google Drive" }).click();
    const dialog = page.getByRole("dialog", { name: "Delete this vault from Google Drive?" });
    await expect(dialog).toContainText("The vault on this Mac stays exactly as it is");
    await expect(dialog).toContainText("ask before they upload it again");
    await expectAccessible(page);

    const confirm = dialog.getByRole("button", { name: "Delete from Google Drive" });
    await expect(confirm).toBeDisabled();
    await dialog.getByLabel(`Type "${PHRASE}" to confirm`).fill("delete from drive");
    await expect(confirm).toBeDisabled();
    expect(await calls(page)).not.toContain("sync_delete_remote");

    await dialog.getByLabel(`Type "${PHRASE}" to confirm`).fill(PHRASE);
    await expect(confirm).toBeEnabled();
    await confirm.click();
    await expect(dialog).toHaveCount(0);
    await expect(page.getByTestId("last-sync")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Turn on sync" })).toBeVisible();
    expect(await page.evaluate(() => (window as any).__core.driveDeletes)).toEqual([PHRASE]);

    // The vault is still here.
    await page.getByRole("button", { name: "All items" }).click();
    await expect(page.getByRole("button", { name: /^GitHub/ })).toBeVisible();
  });

  test("works with sync off, and a refusal stays in the dialog", async ({ page }) => {
    await openApp(page, {
      sync: { enabled: false, state: "off", signedIn: true },
      deleteFails: {
        code: "foreign_vault",
        message:
          "Your Google Drive holds a different vault than this Mac's, so nothing was deleted.",
      },
    });
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByTestId("settings-sync").click();
    await page.getByRole("button", { name: "Delete from Google Drive" }).click();
    const dialog = page.getByRole("dialog", { name: "Delete this vault from Google Drive?" });
    await expect(dialog).toContainText("Sync stays off on this Mac");
    await dialog.getByLabel(`Type "${PHRASE}" to confirm`).fill(PHRASE);
    await dialog.getByRole("button", { name: "Delete from Google Drive" }).click();
    await expect(dialog.getByRole("alert")).toContainText("holds a different vault");
    expect(await page.evaluate(() => (window as any).__core.driveDeletes.length)).toBe(0);
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toHaveCount(0);
  });

  test("is not offered when this Mac is not signed in to Google", async ({ page }) => {
    await openApp(page, { sync: { enabled: false, state: "off", signedIn: false } });
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByTestId("settings-sync").click();
    await expect(page.getByRole("button", { name: "Delete from Google Drive" })).toHaveCount(0);
  });

  test("a second device finds the vault missing and offers to upload again", async ({ page }) => {
    await openApp(page, {
      sync: {
        ...synced,
        state: "lost",
        message:
          "The vault is no longer in your Google Drive. You can upload it again from this Mac.",
      },
    });
    await expect(page.getByTestId("sync-badge")).toHaveText("Vault missing from Drive");
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByTestId("settings-sync").click();
    await expect(page.getByRole("button", { name: "Upload from this Mac" })).toBeVisible();
  });
});
