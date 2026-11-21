import { expect, test } from "@playwright/test";
import { GITHUB, KEY, PASSWORD, calls, expectAccessible, openApp } from "./core";

const typeKey = async (page: import("@playwright/test").Page) => {
  await page.getByLabel("Secret Key group 1 of 8").click();
  await page.keyboard.insertText(KEY);
};

test.describe("keeping the Secret Key on this Mac again (FIG-142)", () => {
  test("opt out, lock, unlock with the key typed, opt back in with nothing typed, then password only", async ({
    page,
  }) => {
    await openApp(page, { items: [GITHUB], touchAvailable: true });
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("button", { name: "Stop keeping it on this Mac" }).click();
    await page.getByRole("button", { name: "Stop keeping it", exact: true }).click();
    await expect(page.getByTestId("key-why")).toContainText("You stopped keeping it here");
    await expectAccessible(page);

    // Locked: the key is asked for, with a note that says why. Typing it does not bring the keeping back.
    await page.getByRole("button", { name: "Lock", exact: true }).click();
    await expect(page.getByLabel("Secret Key group 1 of 8")).toBeVisible();
    await expect(page.getByText("You stopped keeping it on this Mac")).toBeVisible();
    await page.getByLabel("Master password").fill(PASSWORD);
    await typeKey(page);
    await page.getByRole("button", { name: "Unlock" }).click();
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByTestId("key-why")).toContainText("You stopped keeping it here");

    // Opted back in from the open window: the key is held, so nothing is typed.
    await page.getByRole("button", { name: "Keep it on this Mac" }).click();
    await expect(page.getByTestId("key-where")).toBeVisible();
    expect(await calls(page)).toContain("keep_secret_key");
    await expect(page.getByLabel("Secret Key group 1 of 8")).toHaveCount(0);

    await page.getByRole("button", { name: "Lock", exact: true }).click();
    await expect(page.getByLabel("Secret Key group 1 of 8")).toHaveCount(0);
    await page.getByLabel("Master password").fill(PASSWORD);
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.getByRole("button", { name: /^GitHub/ })).toBeVisible();
  });

  test("a window that does not hold the key asks for it once to keep it again", async ({
    page,
  }) => {
    await openApp(page, { touchAvailable: true, keyOptedOut: true, keyHeld: false });
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("button", { name: "Keep it on this Mac" }).click();
    await expect(
      page.getByRole("heading", { name: "Keep your Secret Key on this Mac" }),
    ).toBeVisible();
    await page.getByLabel("Master password").fill("wrong wrong wrong");
    await typeKey(page);
    await page.getByRole("button", { name: "Keep it", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText("do not match");
    await page.getByLabel("Master password").fill(PASSWORD);
    await page.getByRole("button", { name: "Keep it", exact: true }).click();
    await expect(page.getByTestId("key-where")).toBeVisible();
  });

  test("a Mac with no secure place offers no keep button", async ({ page }) => {
    await openApp(page, { keyNotKept: true });
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByRole("button", { name: "Keep it on this Mac" })).toHaveCount(0);
  });
});
