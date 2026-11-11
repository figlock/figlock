import { render, screen, waitFor, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";

import type { ItemDetail, ItemSummary } from "./items";

/** A stand-in for the app core: the same commands, over a few made-up items. */
const core = vi.hoisted(() => {
  const state = {
    live: [] as ItemSummary[],
    deleted: [] as ItemSummary[],
    detail: {} as Record<string, ItemDetail>,
    secrets: {} as Record<string, string>,
  };
  return {
    state,
    listItems: vi.fn(),
    revealField: vi.fn(),
    copyField: vi.fn(),
    deleteItem: vi.fn(),
    restoreItem: vi.fn(),
    addItem: vi.fn(),
    itemForEdit: vi.fn(),
    updateItem: vi.fn(),
    chooseFiles: vi.fn(),
    clearStaged: vi.fn(),
  };
});

vi.mock("./items", async (original) => ({
  ...(await original<typeof import("./items")>()),
  listItems: core.listItems,
  listDeleted: () => Promise.resolve(core.state.deleted),
  itemDetail: (id: string) =>
    core.state.detail[id]
      ? Promise.resolve(core.state.detail[id])
      : Promise.reject({ code: "not_found", message: "gone" }),
  revealField: core.revealField,
  copyField: core.copyField,
  deleteItem: core.deleteItem,
  restoreItem: core.restoreItem,
  addItem: core.addItem,
  itemForEdit: core.itemForEdit,
  updateItem: core.updateItem,
  chooseFiles: core.chooseFiles,
  stageFiles: vi.fn(),
  unstageFile: vi.fn(),
  clearStaged: core.clearStaged,
  saveFile: vi.fn(),
  onFileDrag: () => Promise.resolve(() => {}),
}));
vi.mock("./vault", () => ({ errorOf: (e: unknown) => e }));

import Workspace from "./Workspace.svelte";

const row = (
  id: string,
  kind: ItemSummary["kind"],
  title: string,
  subtitle: string,
): ItemSummary => ({
  id,
  kind,
  title,
  subtitle,
  updatedMs: Date.now() - 3 * 86_400_000,
});

const password = crypto.randomUUID();

beforeEach(() => {
  // jsdom has no modal dialogs.
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  vi.clearAllMocks();
  core.state.live = [
    row("a1", "login", "Cloudflare", "ops@example.test"),
    row("b2", "token", "GitHub token", "GitHub - api.github.com"),
    row("c3", "note", "Office Wi-Fi", "Secure note"),
  ];
  core.state.deleted = [row("d4", "login", "Old forum", "me@example.test")];
  core.state.detail = {
    a1: {
      id: "a1",
      kind: "login",
      deleted: false,
      updatedMs: Date.now(),
      fields: {
        title: "Cloudflare",
        username: "ops@example.test",
        "url/x1": "dash.cloudflare.com",
        "tags/dns": "1",
      },
      secrets: ["password"],
      files: [],
    },
    d4: {
      id: "d4",
      kind: "login",
      deleted: true,
      updatedMs: Date.now(),
      fields: { title: "Old forum", username: "me@example.test" },
      secrets: ["password"],
      files: [],
    },
  };
  core.listItems.mockImplementation((q: string) =>
    Promise.resolve(
      q
        ? core.state.live.filter((i) => i.title.toLowerCase().includes(q.toLowerCase()))
        : core.state.live,
    ),
  );
  core.revealField.mockResolvedValue(password);
  core.copyField.mockResolvedValue(30);
  core.deleteItem.mockResolvedValue(undefined);
  core.restoreItem.mockResolvedValue(undefined);
  core.addItem.mockResolvedValue("new1");
  core.updateItem.mockResolvedValue(undefined);
});

const open = () => render(Workspace, { onlock: vi.fn(), onlocked: vi.fn() });

test("the sidebar counts every type and the list shows what each item is", async () => {
  open();
  expect(await screen.findByRole("button", { name: "Cloudflare, ops@example.test" })).toBeTruthy();
  const nav = screen.getByRole("navigation", { name: "Vault" });
  expect(within(nav).getByRole("button", { name: "All items" }).textContent).toContain("3");
  expect(within(nav).getByRole("button", { name: "Logins" }).textContent).toContain("1");
  expect(within(nav).getByRole("button", { name: "Recently deleted" }).textContent).toContain("1");

  await userEvent.click(within(nav).getByRole("button", { name: "API tokens" }));
  expect(
    screen.getByRole("button", { name: "GitHub token, GitHub - api.github.com" }),
  ).toBeTruthy();
  expect(screen.queryByRole("button", { name: /Cloudflare/ })).toBeNull();
});

test("searching asks the app core and says so when nothing matches", async () => {
  const user = userEvent.setup();
  open();
  await screen.findByRole("button", { name: /Cloudflare/ });
  await user.type(screen.getByRole("searchbox", { name: "Search items" }), "cloud");
  await waitFor(() => expect(core.listItems).toHaveBeenCalledWith("cloud"));
  await waitFor(() => expect(screen.queryByRole("button", { name: /Office/ })).toBeNull());
  expect(screen.getByRole("button", { name: /Cloudflare/ })).toBeTruthy();

  await user.clear(screen.getByRole("searchbox", { name: "Search items" }));
  await user.type(screen.getByRole("searchbox", { name: "Search items" }), "zzzz");
  expect(await screen.findByText(/No items match "zzzz"/)).toBeTruthy();
});

test("a secret stays hidden until Show, and Copy never hands the value to the window", async () => {
  const user = userEvent.setup();
  const { container } = open();
  await user.click(await screen.findByRole("button", { name: /Cloudflare/ }));
  expect(await screen.findByRole("heading", { name: "Cloudflare" })).toBeTruthy();
  expect(screen.getByText("ops@example.test")).toBeTruthy();
  expect(container.textContent).not.toContain(password);
  expect(core.revealField).not.toHaveBeenCalled();

  await user.click(screen.getByRole("button", { name: /Show password/ }));
  expect(await screen.findByText(password)).toBeTruthy();
  expect(core.revealField).toHaveBeenCalledWith("a1", "password");
  expect(screen.getByText(/Hidden again in \d+s/)).toBeTruthy();

  await user.click(screen.getByRole("button", { name: /Hide password/ }));
  expect(container.textContent).not.toContain(password);

  await user.click(screen.getByRole("button", { name: /Copy password/ }));
  expect(core.copyField).toHaveBeenCalledWith("a1", "password");
  expect(await screen.findByText(/Clipboard clears in \d+s/)).toBeTruthy();
  expect(container.textContent).not.toContain(password);
});

test("deleting asks first, then moves the item to Recently deleted", async () => {
  const user = userEvent.setup();
  open();
  await user.click(await screen.findByRole("button", { name: /Cloudflare/ }));
  await user.click(await screen.findByRole("button", { name: "Delete" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText("Delete Cloudflare?")).toBeTruthy();
  await user.click(within(dialog).getByRole("button", { name: "Delete" }));
  await waitFor(() => expect(core.deleteItem).toHaveBeenCalledWith("a1"));
  expect(await screen.findByText("Deleted Cloudflare")).toBeTruthy();
  expect(screen.getByRole("navigation", { name: "Vault" })).toBeTruthy();
});

test("a deleted item can be restored but not edited", async () => {
  const user = userEvent.setup();
  open();
  await user.click(await screen.findByRole("button", { name: "Recently deleted" }));
  await user.click(await screen.findByRole("button", { name: /Old forum/ }));
  expect(await screen.findByText("This item is deleted")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Edit" })).toBeNull();
  await user.click(screen.getByRole("button", { name: "Restore" }));
  await waitFor(() => expect(core.restoreItem).toHaveBeenCalledWith("d4"));
  expect(await screen.findByText("Restored Old forum")).toBeTruthy();
});

test("a new login is validated, saved with schema field ids, and shown", async () => {
  const user = userEvent.setup();
  open();
  await user.click(await screen.findByRole("button", { name: /New item/ }));
  await user.click(await screen.findByRole("button", { name: /^Login\./ }));

  await user.click(screen.getByRole("button", { name: "Add item" }));
  expect(await screen.findByText("Add a title so you can find this item later.")).toBeTruthy();
  expect(screen.getByText("Add a password. Login items need one.")).toBeTruthy();
  expect(core.addItem).not.toHaveBeenCalled();

  await user.type(screen.getByLabelText(/^Title/), "Linear");
  await user.type(screen.getByLabelText("Username"), "mira@example.test");
  await user.type(screen.getByLabelText(/^Password/), password);
  await user.type(screen.getByLabelText("Website 1"), "linear.app");
  await user.type(screen.getByLabelText("Tags"), "tracker,");
  core.state.detail.new1 = { ...core.state.detail.a1, id: "new1", fields: { title: "Linear" } };
  await user.click(screen.getByRole("button", { name: "Add item" }));

  await waitFor(() => expect(core.addItem).toHaveBeenCalledTimes(1));
  const [kind, draft] = core.addItem.mock.calls[0];
  expect(kind).toBe("login");
  expect(draft.fields).toMatchObject({
    title: "Linear",
    username: "mira@example.test",
    password,
    "tags/tracker": "1",
  });
  expect(Object.keys(draft.fields).some((f) => f.startsWith("url/"))).toBe(true);
  expect(await screen.findByRole("heading", { name: "Linear" })).toBeTruthy();
});

test("leaving an edit with unsaved changes asks before dropping them", async () => {
  const user = userEvent.setup();
  open();
  await user.click(await screen.findByRole("button", { name: /New item/ }));
  await user.click(await screen.findByRole("button", { name: /^Login\./ }));
  await user.type(screen.getByLabelText(/^Title/), "Half done");

  await user.click(screen.getByRole("button", { name: "Back" }));
  const dialog = await screen.findByRole("dialog");
  await user.click(within(dialog).getByRole("button", { name: "Keep editing" }));
  expect((screen.getByLabelText(/^Title/) as HTMLInputElement).value).toBe("Half done");

  await user.click(screen.getByRole("button", { name: "Back" }));
  await user.click(
    within(await screen.findByRole("dialog")).getByRole("button", { name: "Discard changes" }),
  );
  expect(await screen.findByRole("heading", { name: "New item" })).toBeTruthy();
  expect(core.clearStaged).toHaveBeenCalled();
});

test("editing a login saves the changed fields", async () => {
  const user = userEvent.setup();
  core.itemForEdit.mockResolvedValue({
    id: "a1",
    kind: "login",
    fields: { title: "Cloudflare", username: "ops@example.test", password },
    files: [],
  });
  open();
  await user.click(await screen.findByRole("button", { name: /Cloudflare/ }));
  await user.click(await screen.findByRole("button", { name: "Edit" }));
  const title = await screen.findByLabelText(/^Title/);
  await user.clear(title);
  await user.type(title, "Cloudflare DNS");
  await user.click(screen.getByRole("button", { name: "Save changes" }));
  await waitFor(() => expect(core.updateItem).toHaveBeenCalledTimes(1));
  expect(core.updateItem.mock.calls[0][0]).toBe("a1");
  expect(core.updateItem.mock.calls[0][1].fields).toMatchObject({
    title: "Cloudflare DNS",
    password,
  });
});

test("a document takes files from the picker and refuses a ninth", async () => {
  const user = userEvent.setup();
  const file = (n: number) => ({ token: `t${n}`, name: `f${n}.pdf`, size: 2048 });
  core.chooseFiles.mockResolvedValueOnce({
    files: [file(1), file(2)],
    refused: [{ name: "big.bin", message: "big.bin must be between 1 byte and 768 KiB" }],
  });
  open();
  await user.click(await screen.findByRole("button", { name: /New item/ }));
  await user.click(await screen.findByRole("button", { name: /^Document\./ }));

  await user.click(screen.getByRole("button", { name: "Add item" }));
  expect(await screen.findByText("Add at least one file.")).toBeTruthy();

  await user.click(screen.getByRole("button", { name: "Choose files" }));
  expect(await screen.findByText("f1.pdf")).toBeTruthy();
  expect(screen.getByText("f2.pdf")).toBeTruthy();
  expect(screen.getByText("2 of 8 files")).toBeTruthy();
  expect(screen.getByText(/big.bin must be between/)).toBeTruthy();
  expect(screen.queryByText("Add at least one file.")).toBeNull();

  await user.type(screen.getByLabelText(/^Title/), "Lease");
  await user.click(screen.getByRole("button", { name: "Add item" }));
  await waitFor(() => expect(core.addItem).toHaveBeenCalledTimes(1));
  expect(core.addItem.mock.calls[0][1].addFiles).toEqual(["t1", "t2"]);
});

test("a vault with nothing in it offers to add the first item", async () => {
  core.state.live = [];
  core.state.deleted = [];
  core.listItems.mockResolvedValue([]);
  open();
  expect(await screen.findByText("Your vault is empty")).toBeTruthy();
});
