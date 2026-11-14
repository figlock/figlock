import { expect, test } from "@playwright/test";
import { PASSWORD, calls, expectAccessible, openApp } from "./core";

const MISSING = { path: "/Users/test/FigLock", canRestore: false };

test.describe("a missing vault folder (FIG-118)", () => {
  test("it is shown before any credentials are asked for, with the options", async ({ page }) => {
    await openApp(page, { phase: "locked", folderMissing: MISSING });
    await expect(page.getByRole("heading", { name: "Your vault folder is missing" })).toBeVisible();
    await expect(page.getByText(MISSING.path)).toBeVisible();
    await expect(page.getByLabel("Master password")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Locate the folder" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Create a new vault" })).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Remove this vault from this Mac" }),
    ).toBeVisible();
    // Sync was not on, so there is nothing to restore.
    await expect(page.getByRole("button", { name: "Restore from Google Drive" })).toHaveCount(0);
    await expectAccessible(page);
  });

  test("locating the moved folder brings back the unlock screen", async ({ page }) => {
    await openApp(page, {
      phase: "locked",
      folderMissing: MISSING,
      pickedFolder: "/Users/test/Moved",
    });
    await page.getByRole("button", { name: "Locate the folder" }).click();
    await expect(page.getByRole("heading", { name: "Unlock your vault" })).toBeVisible();
    await page.getByLabel("Master password").fill(PASSWORD);
    await page.getByRole("button", { name: "Unlock", exact: true }).click();
    await expect(page.getByRole("button", { name: "Lock" })).toBeVisible();
  });

  test("a folder without a vault is refused in words and nothing changes", async ({ page }) => {
    await openApp(page, {
      phase: "locked",
      folderMissing: MISSING,
      pickedFolder: "/Users/test/empty",
    });
    await page.getByRole("button", { name: "Locate the folder" }).click();
    await expect(page.getByRole("alert")).toContainText(
      "There is no FigLock vault in that folder.",
    );
    await expect(page.getByRole("heading", { name: "Your vault folder is missing" })).toBeVisible();
  });

  test("cancelling the folder picker does nothing", async ({ page }) => {
    await openApp(page, { phase: "locked", folderMissing: MISSING });
    await page.getByRole("button", { name: "Locate the folder" }).click();
    await expect(page.getByRole("heading", { name: "Your vault folder is missing" })).toBeVisible();
    expect(await calls(page)).not.toContain("locate_folder");
  });

  test("restore from Google Drive is offered when sync was on, signs in, and then unlocks", async ({
    page,
  }) => {
    await openApp(page, { phase: "locked", folderMissing: { ...MISSING, canRestore: true } });
    await page.getByRole("button", { name: "Restore from Google Drive" }).click();
    await expect(page.getByRole("heading", { name: "Unlock your vault" })).toBeVisible();
    const made = await calls(page);
    expect(made).toContain("drive_sign_in");
    expect(made).toContain("restore_from_drive");
  });

  test("creating a new vault needs a confirmation, then goes to the create screen", async ({
    page,
  }) => {
    await openApp(page, { phase: "locked", folderMissing: MISSING });
    await page.getByRole("button", { name: "Create a new vault" }).click();
    await expect(page.getByRole("dialog")).toContainText("This Mac will stop using the old vault");
    // Cancelling forgets nothing.
    await page.getByRole("button", { name: "Cancel" }).click();
    expect(await calls(page)).not.toContain("forget_vault");
    await expect(page.getByRole("heading", { name: "Your vault folder is missing" })).toBeVisible();

    await page.getByRole("button", { name: "Create a new vault" }).click();
    await page.getByRole("button", { name: "Stop using the old vault" }).click();
    await expect(page.getByRole("heading", { name: "Create your vault" })).toBeVisible();
  });

  test("removing the vault from this Mac needs a confirmation, then shows the welcome screen", async ({
    page,
  }) => {
    await openApp(page, { phase: "locked", folderMissing: MISSING });
    await page.getByRole("button", { name: "Remove this vault from this Mac" }).click();
    await expect(page.getByRole("dialog")).toContainText("Nothing is deleted");
    await page.keyboard.press("Escape");
    expect(await calls(page)).not.toContain("forget_vault");

    await page.getByRole("button", { name: "Remove this vault from this Mac" }).click();
    await page.getByRole("button", { name: "Remove from this Mac" }).click();
    await expect(page.getByRole("heading", { name: "Welcome to FigLock" })).toBeVisible();
  });
});
