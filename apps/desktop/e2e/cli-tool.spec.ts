import { expect, test } from "@playwright/test";
import { calls, expectAccessible, openApp } from "./core";

test.describe("command line tool", () => {
  test("nothing is installed until the person chooses it, and it can be removed again", async ({
    page,
  }) => {
    await openApp(page);
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByRole("heading", { name: "Command line tool" })).toBeVisible();
    await expect(page.getByText("It does not change your PATH or shell settings.")).toBeVisible();
    expect(await calls(page)).not.toContain("install_cli_tool");

    await page.getByRole("button", { name: "Install command line tool" }).click();
    await expect(page.getByTestId("cli-where")).toContainText("~/.local/bin/figlock");
    await expect(page.getByText("FigLock never edits it for you.")).toBeVisible();
    await expectAccessible(page);

    await page.getByRole("button", { name: "Remove the link" }).click();
    await expect(page.getByRole("button", { name: "Install command line tool" })).toBeVisible();
  });

  test("a build without the tool says so", async ({ page }) => {
    await openApp(page, { cli: { state: "unavailable" } });
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByTestId("cli-unavailable")).toBeVisible();
    await expect(page.getByRole("button", { name: "Install command line tool" })).toHaveCount(0);
  });

  test("a different tool already at the path is left alone and explained", async ({ page }) => {
    await openApp(page, { cli: { state: "conflict" } });
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByRole("alert")).toContainText("left it alone");
    await expect(page.getByRole("button", { name: "Install command line tool" })).toHaveCount(0);
    await expectAccessible(page);
  });

  test("a command line tool installed on its own is named and not duplicated", async ({ page }) => {
    await openApp(page, { cli: { state: "external" } });
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByTestId("cli-external")).toContainText("/opt/homebrew/bin/figlock");
    await expect(page.getByRole("button", { name: "Install command line tool" })).toHaveCount(0);
    await expectAccessible(page);
  });
});
