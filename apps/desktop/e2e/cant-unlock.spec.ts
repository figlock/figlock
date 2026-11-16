import { expect, test } from "@playwright/test";
import { GITHUB, openApp } from "./core";

test.describe("can't unlock (FIG-139)", () => {
  test("a locked Mac with no way back stops using the vault after confirming", async ({ page }) => {
    await openApp(page, { phase: "locked", items: [GITHUB] });
    await page.getByRole("button", { name: "I forgot my password or Secret Key" }).click();
    await expect(page.getByRole("heading", { name: "Can't unlock?" })).toBeVisible();
    await expect(page.getByText("Another device is unlocked")).toBeVisible();

    // Cancelling the confirmation changes nothing.
    await page.getByRole("button", { name: "Remove this vault from this Mac" }).click();
    await page.getByRole("button", { name: /^Cancel/ }).click();
    await expect(page.getByRole("heading", { name: "Can't unlock?" })).toBeVisible();

    await page.getByRole("button", { name: "Remove this vault from this Mac" }).click();
    await page.getByRole("button", { name: "Remove from this Mac" }).click();
    await expect(page.getByRole("heading", { name: "Can't unlock?" })).toHaveCount(0);
    await expect(page.getByRole("heading", { name: "Unlock your vault" })).toHaveCount(0);
  });

  test("back returns to Unlock", async ({ page }) => {
    await openApp(page, { phase: "locked", items: [GITHUB] });
    await page.getByRole("button", { name: "I forgot my password or Secret Key" }).click();
    await page.getByRole("button", { name: "Back to unlock" }).click();
    await expect(page.getByRole("heading", { name: "Unlock your vault" })).toBeVisible();
  });
});
