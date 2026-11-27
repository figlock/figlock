import { expect, test } from "@playwright/test";
import { GITHUB, NEW_KEY, PASSWORD, calls, expectAccessible, openApp } from "./core";

test.describe("a Secret Key changed on another device", () => {
  test("the unlock screen says so before anything is typed, and one entry of the new key fixes it", async ({
    page,
  }) => {
    await openApp(page, { phase: "locked", items: [GITHUB], secretKeyStale: true });
    await expect(page.getByText("Your Secret Key was changed on another device.")).toBeVisible();
    await expect(page.getByLabel("Secret Key group 1 of 8")).toBeVisible();
    await expectAccessible(page);

    await page.getByLabel("Password").fill(PASSWORD);
    await page.getByLabel("Secret Key group 1 of 8").click();
    await page.keyboard.insertText(NEW_KEY);
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.getByRole("button", { name: /^GitHub/ })).toBeVisible();

    // Locked and unlocked again: the key is kept, so nothing is said and nothing is asked.
    await page.getByRole("button", { name: "Lock", exact: true }).click();
    await expect(page.getByLabel("Secret Key group 1 of 8")).toHaveCount(0);
    await expect(page.getByText("Your Secret Key was changed on another device.")).toHaveCount(0);
    await page.getByLabel("Password").fill(PASSWORD);
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.getByRole("button", { name: /^GitHub/ })).toBeVisible();
  });

  test("a wrong password with a current key never shows the key fields", async ({ page }) => {
    await openApp(page, { phase: "locked" });
    await expect(page.getByLabel("Secret Key group 1 of 8")).toHaveCount(0);
    await page.getByLabel("Password").fill("not it at all");
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.getByRole("alert")).toContainText("That password did not open the vault");
    await expect(page.getByLabel("Secret Key group 1 of 8")).toHaveCount(0);
    expect(await calls(page)).toContain("unlock");
  });

  test("My Secret Key changed opens the key fields by hand", async ({ page }) => {
    await openApp(page, { phase: "locked" });
    await page.getByRole("button", { name: "My Secret Key changed" }).click();
    await expect(page.getByLabel("Secret Key group 1 of 8")).toBeVisible();
  });

  test("an open window shows a banner, and Update now keeps the new key while the session works on", async ({
    page,
  }) => {
    await openApp(page, { items: [GITHUB], secretKeyStale: true });
    const banner = page.getByTestId("secret-key-banner");
    await expect(banner).toContainText("Your Secret Key was changed on another device");
    await expect(page.getByRole("button", { name: /^GitHub/ })).toBeVisible();
    await expectAccessible(page);

    await banner.getByRole("button", { name: "Update now" }).click();
    const dialog = page.getByRole("dialog", { name: "Update your Secret Key" });
    await expectAccessible(page);

    // The wrong key is refused in words and the banner stays.
    await dialog.getByLabel("Password").fill(PASSWORD);
    await dialog.getByLabel("Secret Key group 1 of 8").click();
    await page.keyboard.insertText("FL1-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA");
    await dialog.getByRole("button", { name: "Update" }).click();
    await expect(dialog.getByRole("alert")).toContainText("do not match this vault");
    await expect(banner).toBeVisible();

    // Start the dialog again with the right key.
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await banner.getByRole("button", { name: "Update now" }).click();
    await dialog.getByLabel("Password").fill(PASSWORD);
    await dialog.getByLabel("Secret Key group 1 of 8").click();
    await page.keyboard.insertText(NEW_KEY);
    await dialog.getByRole("button", { name: "Update" }).click();
    await expect(banner).toHaveCount(0);
    await expect(page.getByRole("button", { name: /^GitHub/ })).toBeVisible();
  });

  test("a Mac whose key is current shows no banner", async ({ page }) => {
    await openApp(page, { items: [GITHUB] });
    await expect(page.getByRole("button", { name: /^GitHub/ })).toBeVisible();
    await expect(page.getByTestId("secret-key-banner")).toHaveCount(0);
  });
});
