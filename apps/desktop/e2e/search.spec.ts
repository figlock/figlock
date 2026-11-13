import { expect, test } from "@playwright/test";
import { GITHUB, NOTE, STRIPE, openApp } from "./core";

test.describe("search", () => {
  test("typing narrows the list and clearing brings everything back", async ({ page }) => {
    await openApp(page, { items: [GITHUB, STRIPE, NOTE] });
    const box = page.getByRole("searchbox", { name: "Search items" });
    await box.fill("stripe");
    await expect(page.getByRole("button", { name: /^Stripe key/ })).toBeVisible();
    await expect(page.getByRole("button", { name: /^GitHub/ })).toHaveCount(0);
    await expect(page.getByText(/^1 item for "stripe"/)).toBeVisible();
    await box.fill("");
    await expect(page.getByText("3 items")).toBeVisible();
  });

  test("no match says so", async ({ page }) => {
    await openApp(page, { items: [GITHUB] });
    await page.getByRole("searchbox").fill("zzzz");
    await expect(page.getByRole("button", { name: /^GitHub/ })).toHaveCount(0);
    await expect(page.getByText(/No items|nothing|No matches/i).first()).toBeVisible();
  });

  test("searching from another page jumps to the results; Cmd+F focuses the box", async ({
    page,
  }) => {
    await openApp(page, { items: [GITHUB, STRIPE] });
    await page.getByRole("button", { name: "Generator" }).click();
    await page.keyboard.press("Meta+f");
    await expect(page.getByRole("searchbox")).toBeFocused();
    await page.keyboard.type("git");
    await expect(page.getByRole("button", { name: /^GitHub/ })).toBeVisible();
    await expect(page.getByRole("button", { name: /^Stripe key/ })).toHaveCount(0);
  });
});
