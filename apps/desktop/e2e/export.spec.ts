import { expect, test } from "@playwright/test";
import { GITHUB, PASSWORD, calls, expectAccessible, openApp } from "./core";

const exported = (page: import("@playwright/test").Page) =>
  page.evaluate(() => (window as any).__core.exports as { includeSecrets: boolean }[]);

async function openExport(page: import("@playwright/test").Page) {
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByTestId("settings-export").click();
  await expect(page.getByRole("heading", { name: "Export" })).toBeVisible();
}

test.describe("export (FIG-287)", () => {
  test("without secrets nothing is asked", async ({ page }) => {
    await openApp(page, { items: [GITHUB] });
    await openExport(page);
    await expectAccessible(page);
    await page.getByRole("button", { name: "Export..." }).click();
    await expect(page.getByTestId("export-saved")).toBeVisible();
    expect(await exported(page)).toEqual([
      { includeSecrets: false, includeDeleted: false, password: null },
    ]);
  });

  test("with secrets and no Touch ID the password is typed, and a wrong one writes nothing", async ({
    page,
  }) => {
    await openApp(page, { items: [GITHUB] });
    await openExport(page);
    await page.getByTestId("include-secrets").check();
    await page.getByRole("button", { name: "Export..." }).click();
    const dialog = page.getByRole("dialog", { name: "Export with secrets" });
    await expectAccessible(page);
    await dialog.getByLabel("Password").fill("not it at all");
    await dialog.getByRole("button", { name: "Export", exact: true }).click();
    await expect(dialog.getByRole("alert")).toContainText("do not match");
    expect(await exported(page)).toEqual([]);

    await dialog.getByLabel("Password").fill(PASSWORD);
    await dialog.getByRole("button", { name: "Export", exact: true }).click();
    await expect(page.getByTestId("export-saved")).toBeVisible();
    expect((await exported(page))[0].includeSecrets).toBe(true);
  });

  test("with Touch ID on, it is asked instead of the password, and declining writes nothing", async ({
    page,
  }) => {
    await openApp(page, {
      items: [GITHUB],
      route: "settings",
      touchAvailable: true,
      quickUnlock: true,
      touchDeclined: true,
    });
    await openExport(page);
    await page.getByTestId("include-secrets").check();
    await page.getByRole("button", { name: "Export..." }).click();
    await expect(page.getByRole("alert").last()).toContainText("Touch ID was not confirmed");
    expect(await exported(page)).toEqual([]);
    expect(await calls(page)).toContain("export_vault");
  });

  test("cancelling the save dialog says nothing was saved", async ({ page }) => {
    await openApp(page, { items: [GITHUB], kitDialogCancelled: true });
    await openExport(page);
    await page.getByRole("button", { name: "Export..." }).click();
    await expect(page.getByTestId("export-saved")).toHaveCount(0);
  });
});
