import { expect, test } from "@playwright/test";
import { GITHUB, KEY, PASSWORD, calls, expectAccessible, openApp } from "./core";

test.describe("Touch ID in Settings (FIG-141)", () => {
  test("off, then on again: the Unlock screen follows each state", async ({ page }) => {
    await openApp(page, { items: [GITHUB], touchAvailable: true, quickUnlock: true });
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByTestId("touch-id-state")).toContainText("fingerprint");
    await expectAccessible(page);

    await page.getByRole("button", { name: "Turn off Touch ID" }).click();
    await page.getByRole("button", { name: "Turn off Touch ID" }).last().click();
    await expect(page.getByRole("button", { name: "Turn on Touch ID" })).toBeVisible();
    expect(await calls(page)).toContain("turn_touch_id_off");

    await page.getByRole("button", { name: "Lock", exact: true }).click();
    await expect(page.getByRole("button", { name: "Use Touch ID" })).toHaveCount(0);
    await page.getByLabel("Master password").fill(PASSWORD);
    await page.getByRole("button", { name: "Unlock" }).click();

    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("button", { name: "Turn on Touch ID" }).click();
    // The Secret Key is kept on this Mac, so only the master password is asked for.
    await expect(page.getByLabel("Secret Key group 1 of 8")).toHaveCount(0);
    await page.getByLabel("Master password").fill("wrong wrong wrong");
    await page.getByRole("button", { name: "Turn on", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText("do not match");
    await page.getByLabel("Master password").fill(PASSWORD);
    await page.getByRole("button", { name: "Turn on", exact: true }).click();
    await expect(page.getByRole("button", { name: "Turn off Touch ID" })).toBeVisible();

    await page.getByRole("button", { name: "Lock", exact: true }).click();
    await expect(page.getByRole("button", { name: "Use Touch ID" })).toBeVisible();
  });

  test("a Mac that does not keep the Secret Key is asked for it to turn Touch ID on", async ({
    page,
  }) => {
    await openApp(page, {
      touchAvailable: true,
      quickUnlock: false,
      keyNotKept: true,
      keyHeld: false,
    });
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("button", { name: "Turn on Touch ID" }).click();
    await expect(page.getByLabel("Secret Key group 1 of 8")).toBeVisible();
    await page.getByLabel("Master password").fill(PASSWORD);
    await page.getByLabel("Secret Key group 1 of 8").click();
    await page.keyboard.insertText(KEY);
    await page.getByRole("button", { name: "Turn on", exact: true }).click();
    await expect(page.getByRole("button", { name: "Turn off Touch ID" })).toBeVisible();
  });

  test("no panel on a Mac without Touch ID", async ({ page }) => {
    await openApp(page);
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByRole("heading", { name: "Touch ID" })).toHaveCount(0);
  });
});
