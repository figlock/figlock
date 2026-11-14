import { expect, test } from "@playwright/test";
import { calls, expectAccessible, openApp } from "./core";

test.describe("updates", () => {
  test("an update found at launch is flagged, shown in Settings and installed only on a click", async ({
    page,
  }) => {
    await openApp(page, { update: { version: "9.9.10", notes: "Faster sync." } });
    await expect(page.getByTestId("update-badge")).toHaveText("Update available");
    expect(await calls(page)).not.toContain("update_install");

    await page.getByTestId("update-badge").click();
    await expect(page.getByTestId("update-available")).toContainText("9.9.10");
    await expect(page.getByTestId("update-notes")).toContainText("Faster sync.");
    await expectAccessible(page);

    await page.getByRole("button", { name: "Install update" }).click();
    await expect(page.getByText("FigLock 9.9.10 is installed")).toBeVisible();
    await expect(page.getByTestId("update-badge")).toHaveText("Restart to update");

    await page.getByRole("button", { name: "Restart FigLock" }).click();
    await page.getByRole("button", { name: "Restart", exact: true }).click();
    await expect.poll(() => page.evaluate(() => (window as any).__core.restarted)).toBe(true);
  });

  test("no update means no notice, and checking by hand says up to date", async ({ page }) => {
    await openApp(page);
    await expect(page.getByTestId("update-badge")).toHaveCount(0);
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("button", { name: "Check for updates" }).click();
    await expect(page.getByTestId("update-current")).toContainText("9.9.9");
    await expectAccessible(page);
  });

  test("a build that cannot update says so", async ({ page }) => {
    await openApp(page, { updateUnavailable: true });
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByTestId("update-unavailable")).toBeVisible();
    await expect(page.getByRole("button", { name: "Check for updates" })).toHaveCount(0);
  });
});
