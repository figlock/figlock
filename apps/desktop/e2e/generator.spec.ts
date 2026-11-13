import { expect, test } from "@playwright/test";
import { openApp } from "./core";

test.describe("generator", () => {
  test("a password follows the length and character choices", async ({ page }) => {
    await openApp(page);
    await page.getByRole("button", { name: "Generator" }).click();
    await expect(page.getByRole("heading", { name: "Generator" })).toBeVisible();

    const length = page.getByRole("slider", { name: /Length/ });
    await length.fill("32");
    await expect(page.getByText(/^[^\s]{32}$/)).toBeVisible();

    for (const name of [/Symbols/, /Uppercase/, /Numbers/])
      await page.getByRole("checkbox", { name }).uncheck();
    await expect(page.getByText(/^[a-z]{32}$/)).toBeVisible();
  });

  test("turning every character type off says why there is no password", async ({ page }) => {
    await openApp(page);
    await page.getByRole("button", { name: "Generator" }).click();
    for (const name of [/Symbols/, /Uppercase/, /Numbers/, /Lowercase/])
      await page.getByRole("checkbox", { name }).uncheck();
    await expect(page.getByText("Turn on at least one character type.")).toBeVisible();
    await page.getByRole("checkbox", { name: /Numbers/ }).check();
    await expect(page.getByText(/^[0-9]{20}$/).or(page.getByText(/^[^\s]{20}$/))).toBeVisible();
  });

  test("a passphrase has the chosen number of words", async ({ page }) => {
    await openApp(page);
    await page.getByRole("button", { name: "Generator" }).click();
    await page.getByRole("radio", { name: "Passphrase" }).check();
    await expect(page.getByText(/^(\w+-){5}\w+$/)).toBeVisible();
  });

  test("Generate again changes the value and Copy goes through the app core", async ({ page }) => {
    await openApp(page);
    await page.getByRole("button", { name: "Generator" }).click();
    const first = await page.getByText(/^[^\s]{20}$/).textContent();
    await page.getByRole("button", { name: "Generate again" }).click();
    await expect(page.getByText(/^[^\s]{20}$/)).not.toHaveText(first!);
    await page.getByRole("button", { name: "Copy", exact: true }).click();
    const copied = await page.evaluate(() => (window as any).__core.copied);
    expect(copied).toEqual(["text:20"]);
  });
});
