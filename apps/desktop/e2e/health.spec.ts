import { expect, test } from "@playwright/test";
import { GITHUB, openApp } from "./core";

const weak = {
  kind: "login",
  fields: { title: "Old forum", username: "me" },
  secrets: { password: "abc123" },
};

test.describe("password health", () => {
  test("a clean vault says there are no problems", async ({ page }) => {
    await openApp(page, { items: [GITHUB] });
    await page.getByRole("button", { name: "Password health" }).click();
    await expect(page.getByText("No problems found")).toBeVisible();
    await expect(page.getByText("Checked 1 password")).toBeVisible();
  });

  test("a weak password is listed and opens its item", async ({ page }) => {
    await openApp(page, { items: [GITHUB, weak] });
    await page.getByRole("button", { name: "Password health" }).click();
    await expect(page.getByText("No problems found")).toHaveCount(0);
    await page.getByRole("button", { name: /Old forum/ }).click();
    await expect(page.getByRole("heading", { name: "Old forum", level: 1 })).toBeVisible();
  });

  test("the breach check is opt-in and sends only what the core asks for", async ({ page }) => {
    await openApp(page, { items: [GITHUB] });
    await page.getByRole("button", { name: "Password health" }).click();
    expect(
      await page.evaluate(
        () => (window as any).__core.calls.filter((c: string) => c === "health_report").length,
      ),
    ).toBe(1);
    await page.getByRole("button", { name: "Check for breaches" }).click();
    await expect(page.getByText(/breach/i).first()).toBeVisible();
    expect(
      await page.evaluate(
        () => (window as any).__core.calls.filter((c: string) => c === "health_report").length,
      ),
    ).toBe(2);
  });

  test("an unreachable breach service is reported, not treated as clean", async ({ page }) => {
    await openApp(page, {
      items: [GITHUB],
      health: { checked: 1, rows: [], breachChecked: false, breachFailed: true },
    });
    await page.getByRole("button", { name: "Password health" }).click();
    await page.getByRole("button", { name: "Check for breaches" }).click();
    await expect(page.getByText(/could not|couldn't|unreachable|try again/i).first()).toBeVisible();
  });
});
