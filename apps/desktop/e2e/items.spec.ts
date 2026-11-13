import { expect, test } from "@playwright/test";
import { GITHUB, NOTE, STRIPE, openApp } from "./core";

const core = (page: import("@playwright/test").Page) =>
  page.evaluate(() => {
    const c = (window as any).__core;
    return { items: [...c.items.values()], deleted: [...c.deleted], copied: c.copied as string[] };
  });

test.describe("items", () => {
  test("a login is created, shown with its password hidden, and the secret stays in the core", async ({
    page,
  }) => {
    await openApp(page);
    await page.getByRole("banner").getByRole("button", { name: "New item" }).click();
    await page.getByRole("button", { name: /^Login\b/ }).click();

    await page.getByLabel("Title").fill("Example bank");
    await page.getByLabel("Username").fill("me@example.test");
    await page.getByRole("textbox", { name: "Password" }).fill("s3cret-pass-demo");
    await page.getByRole("textbox", { name: "Website 1" }).fill("https://bank.example.test");
    await page.getByRole("button", { name: "Add item" }).click();

    await expect(page.getByRole("heading", { name: "Example bank", level: 1 })).toBeVisible();
    await expect(page.getByText("me@example.test")).toBeVisible();
    await expect(page.getByText("https://bank.example.test")).toBeVisible();
    // The page never received the value: it is only fetched on Show.
    await expect(page.getByText("s3cret-pass-demo")).toHaveCount(0);
    await expect(page.getByRole("group", { name: "Password" })).toContainText("Hidden");
    await page.getByRole("button", { name: "Show password" }).click();
    await expect(page.getByRole("group", { name: "Password" })).toContainText("s3cret-pass-demo");
    await page.getByRole("button", { name: "Hide password" }).click();
    await expect(page.getByText("s3cret-pass-demo")).toHaveCount(0);

    const { items } = await core(page);
    expect(items).toHaveLength(1);
    expect(items[0].fields.password).toBeUndefined();
    expect(items[0].secrets.password).toBe("s3cret-pass-demo");
  });

  test("saving without a title shows the error and focuses the field", async ({ page }) => {
    await openApp(page);
    await page.getByRole("banner").getByRole("button", { name: "New item" }).click();
    await page.getByRole("button", { name: /^Login\b/ }).click();
    await page.getByRole("button", { name: "Add item" }).click();
    await expect(page.getByLabel("Title")).toBeFocused();
    await expect(page.getByLabel("Title")).toHaveAttribute("aria-invalid", "true");
    expect((await core(page)).items).toHaveLength(0);
  });

  test("editing keeps a secret that is left blank and changes the title", async ({ page }) => {
    await openApp(page, { items: [GITHUB] });
    await page.getByRole("button", { name: /^GitHub/ }).click();
    await page.getByRole("button", { name: "Edit" }).click();
    await expect(page.getByRole("textbox", { name: "Password" })).toHaveAttribute(
      "placeholder",
      "Unchanged",
    );
    await page.getByLabel("Title").fill("GitHub (work)");
    await page.getByRole("button", { name: "Save changes" }).click();

    await expect(page.getByRole("heading", { name: "GitHub (work)", level: 1 })).toBeVisible();
    const { items } = await core(page);
    expect(items[0].fields.title).toBe("GitHub (work)");
    expect(items[0].secrets.password).toBe("Tr0ub4dor&3-demo");
  });

  test("Cancel after a change asks before throwing it away", async ({ page }) => {
    await openApp(page, { items: [GITHUB] });
    await page.getByRole("button", { name: /^GitHub/ }).click();
    await page.getByRole("button", { name: "Edit" }).click();
    await page.getByLabel("Title").fill("Changed my mind");
    await page.getByRole("button", { name: "Cancel" }).click();
    const dialog = page.getByRole("alertdialog").or(page.getByRole("dialog"));
    await expect(dialog).toBeVisible();
  });

  test("copying a secret asks the core; the value is not in the window", async ({ page }) => {
    await openApp(page, { items: [GITHUB] });
    await page.getByRole("button", { name: /^GitHub/ }).click();
    await page.getByRole("button", { name: "Copy password" }).click();
    expect((await core(page)).copied).toEqual(["item-1:password"]);
    await expect(page.getByText("Tr0ub4dor&3-demo")).toHaveCount(0);
  });

  test("delete moves an item to Recently deleted and restore brings it back", async ({ page }) => {
    await openApp(page, { items: [GITHUB, STRIPE] });
    await page.getByRole("button", { name: /^GitHub/ }).click();
    await page.getByRole("button", { name: "Delete", exact: true }).click();
    const confirm = page.getByRole("alertdialog").or(page.getByRole("dialog"));
    await confirm.getByRole("button", { name: /Delete|Move/ }).click();

    await expect(page.getByRole("button", { name: "All items" })).toContainText("1");
    await page.getByRole("button", { name: "Recently deleted" }).click();
    await page.getByRole("button", { name: /^GitHub/ }).click();
    await page.getByRole("button", { name: "Restore" }).click();
    await expect(page.getByRole("button", { name: "All items" })).toContainText("2");
    expect((await core(page)).deleted).toEqual([]);
  });

  test("the sidebar filters by type and counts follow", async ({ page }) => {
    await openApp(page, { items: [GITHUB, STRIPE, NOTE] });
    await expect(page.getByRole("button", { name: "All items" })).toContainText("3");
    await page.getByRole("button", { name: "API tokens" }).click();
    await expect(page.getByRole("button", { name: /^Stripe key/ })).toBeVisible();
    await expect(page.getByRole("button", { name: /^GitHub/ })).toHaveCount(0);
  });

  test("Back returns to the list", async ({ page }) => {
    await openApp(page, { items: [GITHUB] });
    await page.getByRole("button", { name: /^GitHub/ }).click();
    await page.getByRole("button", { name: "Back" }).click();
    await expect(page.getByRole("button", { name: /^GitHub/ })).toBeVisible();
  });
});
