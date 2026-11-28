import { expect, test } from "@playwright/test";
import { GITHUB, expectAccessible, openApp } from "./core";

const core = (page: import("@playwright/test").Page) =>
  page.evaluate(() => {
    const c = (window as any).__core;
    return { asked: c.importAsked, held: c.importHeld, added: c.importAdded };
  });

const PREVIEW = {
  logins: 2,
  notes: 1,
  tokens: 0,
  tokensDetected: 0,
  skipped: 1,
  titles: ["GitHub", "Bank", "Trip plan"],
  more: 0,
};

async function openImport(page: import("@playwright/test").Page) {
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByTestId("settings-import").click();
  await expect(page.getByRole("heading", { name: "Import" })).toBeVisible();
}

test.describe("import (FIG-285)", () => {
  test("choosing a file shows a preview and writes nothing until confirmed", async ({ page }) => {
    await openApp(page, { items: [GITHUB], importFile: { preview: PREVIEW } });
    await openImport(page);
    await expectAccessible(page);
    await expect(page.getByRole("alert")).toContainText("plain text");

    await page.getByLabel("Where is the file from?").selectOption("apple");
    await page.getByRole("button", { name: "Choose file..." }).click();
    await expect(page.getByTestId("import-preview")).toBeVisible();
    await expect(page.getByTestId("import-counts")).toContainText("2 logins, 1 note");
    await expect(page.getByRole("list", { name: "Items found" })).toContainText("Trip plan");
    await expectAccessible(page);
    expect(await core(page)).toEqual({ asked: ["apple"], held: true, added: [] });

    await page.getByRole("button", { name: "Import 3 items" }).click();
    await expect(page.getByTestId("import-done")).toContainText("Imported 3 items");
    await expect(page.getByTestId("import-done")).toContainText("Delete the export file");
    expect((await core(page)).added).toEqual([3]);
  });

  test("cancelling the preview forgets the file", async ({ page }) => {
    await openApp(page, { items: [GITHUB], importFile: { preview: PREVIEW } });
    await openImport(page);
    await page.getByRole("button", { name: "Choose file..." }).click();
    await page.getByRole("button", { name: "Cancel" }).click();
    await expect(page.getByTestId("import-preview")).toBeHidden();
    expect(await core(page)).toEqual({ asked: ["chrome"], held: false, added: [] });
  });

  test("a file that is not that export shows a clear error and writes nothing", async ({
    page,
  }) => {
    await openApp(page, {
      items: [GITHUB],
      importFile: {
        error: {
          code: "bad_file",
          message: "the file does not look like an export from that source",
        },
      },
    });
    await openImport(page);
    await page.getByRole("button", { name: "Choose file..." }).click();
    await expect(page.getByRole("alert").last()).toContainText("does not look like an export");
    await expect(page.getByTestId("import-preview")).toBeHidden();
    expect(await core(page)).toEqual({ asked: ["chrome"], held: false, added: [] });
  });

  test("closing the file dialog changes nothing", async ({ page }) => {
    await openApp(page, { items: [GITHUB], importFile: { cancelled: true } });
    await openImport(page);
    await page.getByRole("button", { name: "Choose file..." }).click();
    await expect(page.getByTestId("import-preview")).toBeHidden();
    await expect(page.getByRole("button", { name: "Choose file..." })).toBeEnabled();
  });
});
