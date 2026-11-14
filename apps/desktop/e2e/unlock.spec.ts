import { expect, test } from "@playwright/test";
import { GITHUB, KEY, PASSWORD, expectAccessible, openApp } from "./core";

test.describe("unlock", () => {
  test("a wrong master password is refused in words, then the right one opens the vault", async ({
    page,
  }) => {
    await openApp(page, { phase: "locked", items: [GITHUB] });
    await expect(page.getByRole("heading", { name: "Unlock your vault" })).toBeVisible();

    await page.getByLabel("Master password").fill("not it at all");
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.getByRole("alert")).toContainText("That master password is not right.");
    await expect(page.getByLabel("Master password")).toBeFocused();

    await page.getByLabel("Master password").fill(PASSWORD);
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.getByRole("button", { name: /^GitHub/ })).toBeVisible();
  });

  test("a denied keychain prompt says what to do, and trying again opens the vault", async ({
    page,
  }) => {
    await openApp(page, { phase: "locked", items: [GITHUB] });
    await page.evaluate(() => {
      (window as any).__core.failNext = {
        cmd: "unlock",
        code: "keychain_denied",
        message:
          "FigLock could not use the key saved on this Mac. If a Mac prompt asked for permission, choose Allow, then try again.",
      };
    });
    await page.getByLabel("Master password").fill(PASSWORD);
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.getByRole("alert")).toContainText(
      "FigLock could not use the key saved on this Mac. If a Mac prompt asked for permission, choose Allow, then try again.",
    );
    await expect(page.getByRole("alert")).not.toContainText("could not read the device keys");
    await expectAccessible(page);

    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.getByRole("button", { name: /^GitHub/ })).toBeVisible();
  });

  test("an empty form says what is missing and does not call the app core", async ({ page }) => {
    await openApp(page, { phase: "locked" });
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.getByText("Enter your master password.")).toBeVisible();
    expect(await page.evaluate(() => (window as any).__core.calls)).not.toContain("unlock");
  });

  test("locking returns to the unlock form and the items are no longer on screen", async ({
    page,
  }) => {
    await openApp(page, { items: [GITHUB] });
    await expect(page.getByRole("button", { name: /^GitHub/ })).toBeVisible();
    await page.getByRole("button", { name: "Lock" }).click();
    await expect(page.getByRole("heading", { name: "Unlock your vault" })).toBeVisible();
    await expect(page.getByText("GitHub")).toHaveCount(0);
  });

  test("Cmd+L locks from the keyboard", async ({ page }) => {
    await openApp(page, { items: [GITHUB] });
    await page.getByRole("searchbox").press("Meta+l");
    await expect(page.getByRole("heading", { name: "Unlock your vault" })).toBeVisible();
  });
});

void KEY;
