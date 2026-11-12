import { render, screen, waitFor, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";

import type { Device } from "./devices";
import type { SyncView } from "./sync";

/** A stand-in for the app core's device commands. */
const core = vi.hoisted(() => ({
  listDevices: vi.fn(),
  revokeDevice: vi.fn(),
  syncStatus: vi.fn(),
  onSynced: vi.fn(),
  listItems: vi.fn(),
  lock: vi.fn(),
}));

vi.mock("./devices", () => ({ listDevices: core.listDevices, revokeDevice: core.revokeDevice }));
vi.mock("./sync", async (original) => ({
  ...(await original<typeof import("./sync")>()),
  syncStatus: core.syncStatus,
  onSynced: core.onSynced,
}));
vi.mock("./items", async (original) => ({
  ...(await original<typeof import("./items")>()),
  listItems: core.listItems,
  listDeleted: () => Promise.resolve([]),
}));
vi.mock("./vault", () => ({ errorOf: (e: unknown) => e, lock: core.lock }));

import Workspace from "./Workspace.svelte";

const KEY = "FL1-ABCD-EFGH-JKMN-PQRS-TVWX-YZ23-4567-89AB";
const mac: Device = {
  id: "aa",
  name: "Work Mac",
  fingerprint: "1A2B-3C4D",
  thisDevice: true,
  revoked: false,
};
const phone: Device = {
  id: "bb",
  name: "Phone",
  fingerprint: "5E6F-7A8B",
  thisDevice: false,
  revoked: false,
};

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  vi.clearAllMocks();
  core.lock.mockResolvedValue(undefined);
  core.listItems.mockResolvedValue([]);
  core.onSynced.mockResolvedValue(() => {});
  core.syncStatus.mockResolvedValue({
    enabled: false,
    state: "off",
    signedIn: false,
    canSignIn: true,
    lastSyncSecsAgo: null,
    message: null,
    fork: false,
    needsUnlock: false,
    localFolder: null,
  } satisfies SyncView);
  core.listDevices.mockResolvedValue([mac, phone]);
});

async function toDevices() {
  const user = userEvent.setup();
  const onlocked = vi.fn();
  render(Workspace, { onlock: vi.fn(), onlocked });
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await screen.findByRole("list", { name: "Devices" });
  return Object.assign(user, { onlocked });
}

async function fill(user: ReturnType<typeof userEvent.setup>) {
  const dialog = screen.getByRole("dialog");
  await user.type(within(dialog).getByLabelText("Master password"), "correct horse battery 42");
  await user.click(within(dialog).getByLabelText("Secret Key group 1 of 8"));
  await user.paste(KEY);
  return dialog;
}

test("this Mac has no revoke button, another device does", async () => {
  await toDevices();
  expect(screen.getByText(/\(this Mac\)/)).toBeTruthy();
  expect(screen.getAllByRole("button", { name: /^Revoke/ })).toHaveLength(1);
  expect(screen.getByRole("button", { name: "Revoke Phone" })).toBeTruthy();
});

test("revoking asks for the credentials and then shows the device as revoked", async () => {
  const user = await toDevices();
  await user.click(screen.getByRole("button", { name: "Revoke Phone" }));
  core.revokeDevice.mockResolvedValue([mac, { ...phone, name: null, revoked: true }]);
  const dialog = await fill(user);
  await user.click(within(dialog).getByRole("button", { name: "Revoke device" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(core.revokeDevice).toHaveBeenCalledWith("bb", "correct horse battery 42", KEY, null);
  expect(screen.getByText("Revoked")).toBeTruthy();
  expect(screen.getByText("Phone is revoked.")).toBeTruthy();
});

test("a wrong password stays in the dialog so the person can try again", async () => {
  const user = await toDevices();
  await user.click(screen.getByRole("button", { name: "Revoke Phone" }));
  core.revokeDevice.mockRejectedValue({
    code: "wrong_credentials",
    message: "That master password and Secret Key do not match this vault.",
  });
  const dialog = await fill(user);
  await user.click(within(dialog).getByRole("button", { name: "Revoke device" }));
  expect(await screen.findByText(/do not match this vault/)).toBeTruthy();
  expect(screen.getByRole("dialog")).toBeTruthy();
});

test("a compromised device also sets a new password, which must match", async () => {
  const user = await toDevices();
  await user.click(screen.getByRole("button", { name: "Revoke Phone" }));
  const dialog = await fill(user);
  await user.click(within(dialog).getByRole("checkbox"));
  await user.type(within(dialog).getByLabelText("New master password"), "brand new password 7");
  await user.type(within(dialog).getByLabelText("Repeat new master password"), "something else 1");
  await user.click(within(dialog).getByRole("button", { name: "Revoke device" }));
  expect(await screen.findByText("The two passwords do not match.")).toBeTruthy();
  expect(core.revokeDevice).not.toHaveBeenCalled();

  core.revokeDevice.mockResolvedValue([mac, { ...phone, name: null, revoked: true }]);
  const repeat = within(dialog).getByLabelText("Repeat new master password");
  await user.clear(repeat);
  await user.type(repeat, "brand new password 7");
  await user.click(within(dialog).getByRole("button", { name: "Revoke device" }));
  await waitFor(() =>
    expect(core.revokeDevice).toHaveBeenCalledWith(
      "bb",
      "correct horse battery 42",
      KEY,
      "brand new password 7",
    ),
  );
  expect(await screen.findByText(/master password is changed/)).toBeTruthy();
});

test("a refusal because settings changed elsewhere locks the vault and relocks", async () => {
  const user = await toDevices();
  await user.click(screen.getByRole("button", { name: "Revoke Phone" }));
  const message = "Vault settings changed on another device. Lock and unlock to continue.";
  core.revokeDevice.mockRejectedValue({ code: "needs_unlock", message });
  const dialog = await fill(user);
  await user.click(within(dialog).getByRole("button", { name: "Revoke device" }));
  await waitFor(() => expect(user.onlocked).toHaveBeenCalledWith(message));
  expect(core.lock).toHaveBeenCalledTimes(1);
});
