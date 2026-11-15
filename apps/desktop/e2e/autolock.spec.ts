import { expect, test } from "@playwright/test";
import { calls, expectAccessible, openApp } from "./core";

test.describe("auto-lock", () => {
  test("Settings offers the lock times, applies a pick at once and the unlock screen follows", async ({
    page,
  }) => {
    await openApp(page);
    await page.getByRole("button", { name: "Settings" }).click();

    const select = page.getByLabel("Lock after");
    await expect(select).toHaveValue("300");
    await expect(select.locator("option")).toHaveText([
      "1 minute",
      "2 minutes",
      "5 minutes",
      "10 minutes",
      "15 minutes",
      "30 minutes",
      "1 hour",
    ]);
    await expectAccessible(page);

    await select.selectOption("60");
    await expect.poll(() => calls(page)).toContain("set_autolock");
    await expect(select).toHaveValue("60");

    await page.getByRole("button", { name: "Lock", exact: true }).click();
    await expect(
      page.getByText("FigLock locks itself after 1 minute without activity."),
    ).toBeVisible();
  });

  test("the saved time is what Settings shows after a restart, and one hour reads as 1 hour", async ({
    page,
  }) => {
    await openApp(page, { autolockSecs: 3600 });
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByLabel("Lock after")).toHaveValue("3600");
    await page.getByRole("button", { name: "Lock", exact: true }).click();
    await expect(
      page.getByText("FigLock locks itself after 1 hour without activity."),
    ).toBeVisible();
  });
});
