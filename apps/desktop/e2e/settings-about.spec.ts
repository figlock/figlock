import { expect, test } from "@playwright/test";
import { calls, expectAccessible, openApp } from "./core";

test.describe("Settings list and About (FIG-304, FIG-305)", () => {
  test("Settings is a short list of groups, one level deep", async ({ page }) => {
    await openApp(page);
    await page.getByRole("button", { name: "Settings" }).click();
    for (const group of ["General", "This vault", "More"]) {
      await expect(page.getByRole("heading", { name: group })).toBeVisible();
    }
    for (const row of ["security", "sync", "devices", "updates", "advanced", "about"]) {
      await expect(page.getByTestId(`settings-${row}`)).toBeVisible();
    }
    // No paragraph of explanation on the list itself.
    await expect(page.getByTestId("settings-sync")).toContainText("Off");
    await expectAccessible(page);
  });

  test("Appearance follows the choice and is kept", async ({ page }) => {
    await openApp(page);
    await page.getByRole("button", { name: "Settings" }).click();
    const html = page.locator("html");
    await expect(html).not.toHaveAttribute("data-theme", /.+/);

    await page.getByLabel("Appearance").selectOption("dark");
    await expect(html).toHaveAttribute("data-theme", "dark");
    await expectAccessible(page);

    await page.reload();
    await expect(html).toHaveAttribute("data-theme", "dark");

    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByLabel("Appearance").selectOption("system");
    await expect(html).not.toHaveAttribute("data-theme", /.+/);
  });

  test("About shows the version and the pages open in the app", async ({ page }) => {
    await openApp(page);
    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByTestId("settings-about").click();
    await expect(page.getByRole("heading", { name: "FigLock", exact: true })).toBeVisible();
    await expect(page.getByTestId("about-version")).toHaveText(/^Version \d/);
    for (const id of ["protects", "privacy", "terms", "support"]) {
      await expect(page.getByTestId(`about-${id}`)).toBeVisible();
    }
    await expectAccessible(page);

    await page.getByTestId("about-privacy").click();
    await expect.poll(() => calls(page)).toContain("open_about_page");
    expect(await page.evaluate(() => (window as any).__core.aboutPages)).toEqual(["privacy"]);
  });
});
