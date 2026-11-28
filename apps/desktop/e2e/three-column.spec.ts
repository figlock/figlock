import { expect, test } from "@playwright/test";
import { GITHUB, NOTE, STRIPE, expectAccessible, openApp } from "./core";

const detail = (page: import("@playwright/test").Page) =>
  page.getByRole("region", { name: "Item", exact: true });
const items = (page: import("@playwright/test").Page) =>
  page.getByRole("region", { name: "Items", exact: true });

test.describe("three-column vault screen", () => {
  test("the list stays beside the open item, and nothing is open at first", async ({ page }) => {
    await openApp(page, { items: [GITHUB, STRIPE, NOTE] });
    await expect(detail(page).getByText("No item selected")).toBeVisible();
    await items(page)
      .getByRole("button", { name: /^GitHub/ })
      .click();
    await expect(detail(page).getByRole("heading", { name: "GitHub", level: 1 })).toBeVisible();
    await expect(items(page).getByRole("button", { name: /^Stripe key/ })).toBeVisible();
    await items(page)
      .getByRole("button", { name: /^Stripe key/ })
      .click();
    await expect(detail(page).getByRole("heading", { name: "Stripe key", level: 1 })).toBeVisible();
    // Picking items does not add a page per click.
    await expect(page.getByRole("button", { name: "Back" })).toBeDisabled();
    await expectAccessible(page);
  });

  test("secrets stay masked and hide again when another item is picked", async ({ page }) => {
    await openApp(page, { items: [GITHUB, STRIPE] });
    await items(page)
      .getByRole("button", { name: /^GitHub/ })
      .click();
    await expect(detail(page).getByRole("group", { name: "Password" })).toContainText("Hidden");
    await detail(page).getByRole("button", { name: "Show password" }).click();
    await expect(detail(page).getByText("Tr0ub4dor&3-demo")).toBeVisible();
    await items(page)
      .getByRole("button", { name: /^Stripe key/ })
      .click();
    await items(page)
      .getByRole("button", { name: /^GitHub/ })
      .click();
    await expect(detail(page).getByRole("group", { name: "Password" })).toContainText("Hidden");
    await expect(page.getByText("Tr0ub4dor&3-demo")).toHaveCount(0);
  });

  test("up and down move the selection, Return edits, Esc leaves the editor", async ({ page }) => {
    await openApp(page, { items: [GITHUB, STRIPE, NOTE] });
    const rows = items(page).getByRole("button");
    await rows.first().click();
    const first = await rows.first().getAttribute("aria-label");
    await page.keyboard.press("ArrowDown");
    await expect(rows.nth(1)).toBeFocused();
    await expect(rows.nth(1)).toHaveAttribute("aria-current", "true");
    await expect(rows.first()).not.toHaveAttribute("aria-current", "true");
    await page.keyboard.press("ArrowUp");
    await expect(rows.first()).toHaveAttribute("aria-label", first!);
    await expect(rows.first()).toHaveAttribute("aria-current", "true");
    await page.keyboard.press("Enter");
    await expect(page.getByLabel("Title")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByLabel("Title")).toHaveCount(0);
    // Back on the same selection.
    await expect(rows.first()).toHaveAttribute("aria-current", "true");
  });

  test("Esc with unsaved changes asks first", async ({ page }) => {
    await openApp(page, { items: [GITHUB] });
    await items(page)
      .getByRole("button", { name: /^GitHub/ })
      .click();
    await detail(page).getByRole("button", { name: "Edit" }).click();
    await page.getByLabel("Title").fill("Changed");
    await page.keyboard.press("Escape");
    await expect(page.getByRole("alertdialog").or(page.getByRole("dialog"))).toBeVisible();
  });

  test("/ and Cmd+F focus search, and search narrows the list beside the item", async ({
    page,
  }) => {
    await openApp(page, { items: [GITHUB, STRIPE, NOTE] });
    await items(page)
      .getByRole("button", { name: /^GitHub/ })
      .click();
    await page.evaluate(() => (document.activeElement as HTMLElement).blur());
    await page.keyboard.press("/");
    await expect(page.getByRole("searchbox")).toBeFocused();
    await page.keyboard.type("stripe");
    await expect(items(page).getByRole("button", { name: /^Stripe key/ })).toBeVisible();
    await expect(items(page).getByRole("button", { name: /^GitHub/ })).toHaveCount(0);
    // The open item stays open while the list narrows.
    await expect(detail(page).getByRole("heading", { name: "GitHub", level: 1 })).toBeVisible();
    await page.getByRole("searchbox").blur();
    await page.keyboard.press("Meta+f");
    await expect(page.getByRole("searchbox")).toBeFocused();
  });

  test("deleting the open item leaves the list with nothing open", async ({ page }) => {
    await openApp(page, { items: [GITHUB, STRIPE] });
    await items(page)
      .getByRole("button", { name: /^GitHub/ })
      .click();
    await detail(page).getByRole("button", { name: "Delete" }).click();
    await page
      .getByRole("alertdialog")
      .or(page.getByRole("dialog"))
      .getByRole("button", { name: "Delete" })
      .click();
    await expect(detail(page).getByText("No item selected")).toBeVisible();
    await expect(items(page).getByRole("button", { name: /^GitHub/ })).toHaveCount(0);
    await expect(items(page).getByRole("button", { name: /^Stripe key/ })).toBeVisible();
  });

  test("a new item opens beside the list after it is saved", async ({ page }) => {
    await openApp(page, { items: [GITHUB] });
    await page.getByRole("banner").getByRole("button", { name: "New item" }).click();
    await page.getByRole("button", { name: /^Login\b/ }).click();
    await page.getByLabel("Title").fill("Example bank");
    await page.getByRole("textbox", { name: "Password" }).fill("generated-demo-pass");
    await page.getByRole("button", { name: "Add item" }).click();
    await expect(
      detail(page).getByRole("heading", { name: "Example bank", level: 1 }),
    ).toBeVisible();
    await expect(items(page).getByRole("button", { name: /^Example bank/ })).toBeVisible();
  });

  test("below the narrow width it is one page at a time, and the selection survives resizing", async ({
    page,
  }) => {
    await openApp(page, { items: [GITHUB, STRIPE] });
    await items(page)
      .getByRole("button", { name: /^Stripe key/ })
      .click();
    await page.setViewportSize({ width: 900, height: 700 });
    await expect(page.getByRole("region", { name: "Items", exact: true })).toHaveCount(0);
    await expect(page.getByRole("heading", { name: "Stripe key", level: 1 })).toBeVisible();
    await page.getByRole("button", { name: "Back" }).click();
    await expect(page.getByRole("button", { name: /^GitHub/ })).toBeVisible();
    await page.getByRole("button", { name: /^GitHub/ }).click();
    await page.setViewportSize({ width: 1280, height: 760 });
    await expect(detail(page).getByRole("heading", { name: "GitHub", level: 1 })).toBeVisible();
    await expect(items(page).getByRole("button", { name: /^GitHub/ })).toHaveAttribute(
      "aria-current",
      "true",
    );
  });

  test("the list keeps its scroll when an item is picked", async ({ page }) => {
    const many = Array.from({ length: 400 }, (_, i) => ({
      kind: "note",
      fields: { title: `Note ${String(i).padStart(3, "0")}` },
      secrets: {},
    }));
    await openApp(page, { items: many });
    const pane = items(page).locator("xpath=..");
    await pane.evaluate((el) => (el.scrollTop = 2000));
    const before = await pane.evaluate((el) => el.scrollTop);
    expect(before).toBeGreaterThan(1000);
    // Click a row in the middle of the pane, so the click itself does not scroll.
    const box = (await pane.boundingBox())!;
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
    await expect(detail(page).getByRole("heading", { level: 1 })).toBeVisible();
    expect(await pane.evaluate((el) => el.scrollTop)).toBe(before);
  });

  test("list and search stay under 100 ms with 10,000 items", async ({ page }) => {
    const many = Array.from({ length: 10_000 }, (_, i) => ({
      kind: "note",
      fields: { title: `Item ${i}` },
      secrets: {},
    }));
    const t0 = Date.now();
    await openApp(page, { items: many });
    await expect(page.getByText("10000 items").or(page.getByText("10,000 items"))).toBeVisible();
    const load = Date.now() - t0;
    // Timed inside the page, from the keystroke to the narrowed list, so test-runner round trips do not count.
    const search = await page.evaluate(
      () =>
        new Promise<number>((resolve) => {
          const input = document.querySelector<HTMLInputElement>('input[type="search"]')!;
          const t = performance.now();
          const done = () => {
            if (document.querySelectorAll('section[aria-label="Items"] li').length === 1) {
              resolve(performance.now() - t);
            } else requestAnimationFrame(done);
          };
          input.value = "Item 9999";
          input.dispatchEvent(new Event("input", { bubbles: true }));
          done();
        }),
    );
    console.log(`load ${load} ms, search ${Math.round(search)} ms`);
    expect(search).toBeLessThan(100);
  });
});
