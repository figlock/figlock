import { render, screen, waitFor, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import type { Device, Invite, PairingStatus } from "./devices";
import type { SyncView } from "./sync";

/** A stand-in for the app core's pairing commands. */
const core = vi.hoisted(() => ({
  listDevices: vi.fn(),
  revokeDevice: vi.fn(),
  addDeviceStart: vi.fn(),
  addDeviceStatus: vi.fn(),
  addDeviceApprove: vi.fn(),
  addDeviceCancel: vi.fn(),
  syncStatus: vi.fn(),
  onSynced: vi.fn(),
  listItems: vi.fn(),
  lock: vi.fn(),
  vaultState: vi.fn(),
}));

vi.mock("./devices", () => ({
  listDevices: core.listDevices,
  revokeDevice: core.revokeDevice,
  addDeviceStart: core.addDeviceStart,
  addDeviceStatus: core.addDeviceStatus,
  addDeviceApprove: core.addDeviceApprove,
  addDeviceCancel: core.addDeviceCancel,
}));
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
vi.mock("./vault", () => ({
  errorOf: (e: unknown) => e,
  lock: core.lock,
  vaultState: core.vaultState,
}));

import Workspace from "./Workspace.svelte";

const KEY = "FL1-ABCD-EFGH-JKMN-PQRS-TVWX-YZ23-4567-89AB";
const mac: Device = {
  id: "aa",
  name: "Work Mac",
  fingerprint: "1A2B-3C4D",
  thisDevice: true,
  revoked: false,
};
const laptop: Device = {
  id: "cc",
  name: "Home Laptop",
  fingerprint: "9Z8Y-7X6W",
  thisDevice: false,
  revoked: false,
};
const invite: Invite = {
  code: "ABCD-EFGH-JKMN-PQRS-TVWX",
  qr: ["1110111", "1010101", "1110111", "0000000", "1110111", "1010101", "1110111"],
  expiresAtMs: Date.now() + 600_000,
};
const waiting: PairingStatus = {
  state: "waiting",
  expiresAtMs: invite.expiresAtMs,
  deviceName: null,
  fingerprint: null,
};
const asked: PairingStatus = {
  state: "request",
  expiresAtMs: invite.expiresAtMs,
  deviceName: "Home Laptop",
  fingerprint: "9Z8Y-7X6W-5V4U-3T2S",
};

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  vi.useFakeTimers({ shouldAdvanceTime: true });
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
  core.listDevices.mockResolvedValue([mac]);
  core.vaultState.mockResolvedValue({ secretKeyKept: true, secretKeyProtection: "keychain" });
  core.addDeviceStart.mockResolvedValue(invite);
  core.addDeviceStatus.mockResolvedValue(waiting);
  core.addDeviceCancel.mockResolvedValue(undefined);
});

afterEach(() => vi.useRealTimers());

async function openDialog() {
  const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
  const onlocked = vi.fn();
  render(Workspace, { onlock: vi.fn(), onlocked });
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await user.click(await screen.findByRole("button", { name: "Add a device" }));
  await screen.findByTestId("invite-code");
  return Object.assign(user, { onlocked });
}

test("Add a device shows the code and QR and waits for the new device", async () => {
  await openDialog();
  expect(screen.getByTestId("invite-code").textContent?.trim()).toBe(invite.code);
  expect(screen.getByRole("img", { name: "QR code of the invite code" })).toBeTruthy();
  expect(screen.getByText(/Waiting for the new device/)).toBeTruthy();
  expect(core.addDeviceStart).toHaveBeenCalledTimes(1);
});

test("the new device's name and fingerprint are shown, and approving adds it", async () => {
  const user = await openDialog();
  core.addDeviceStatus.mockResolvedValue(asked);
  await vi.advanceTimersByTimeAsync(3100);
  const dialog = await screen.findByRole("dialog");
  expect(await within(dialog).findByText('Add "Home Laptop"?')).toBeTruthy();
  expect(within(dialog).getByTestId("fingerprint").textContent?.trim()).toBe(asked.fingerprint);
  core.addDeviceApprove.mockResolvedValue([mac, laptop]);
  await user.click(within(dialog).getByRole("button", { name: "Approve with Touch ID" }));
  // Touch ID holds the key: nothing is typed or sent but the request to approve.
  expect(core.addDeviceApprove).toHaveBeenCalledWith("", "");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(screen.getByText(/Home Laptop is added/)).toBeTruthy();
  expect(screen.getByText("Home Laptop", { selector: "p" })).toBeTruthy();
});

test("a Mac that keeps no Secret Key asks for the password and key to approve", async () => {
  core.vaultState.mockResolvedValue({ secretKeyKept: false, secretKeyProtection: null });
  const user = await openDialog();
  core.addDeviceStatus.mockResolvedValue(asked);
  await vi.advanceTimersByTimeAsync(3100);
  const dialog = await screen.findByRole("dialog");
  await within(dialog).findByText('Add "Home Laptop"?');
  core.addDeviceApprove.mockRejectedValueOnce({
    code: "wrong_credentials",
    message: "That master password and Secret Key do not match this vault.",
  });
  await user.type(within(dialog).getByLabelText("Master password"), "correct horse battery 42");
  await user.click(within(dialog).getByLabelText("Secret Key group 1 of 8"));
  await user.paste(KEY);
  await user.click(within(dialog).getByRole("button", { name: "Approve" }));
  expect(await within(dialog).findByText(/do not match this vault/)).toBeTruthy();
  expect(core.addDeviceApprove).toHaveBeenCalledWith("correct horse battery 42", KEY);
  // Still waiting for a right answer: the dialog stays.
  expect(screen.getByRole("dialog")).toBeTruthy();
});

test("a session that holds the Secret Key asks for the master password only, with no key field or Touch ID step", async () => {
  core.vaultState.mockResolvedValue({
    secretKeyKept: false,
    secretKeyHeld: true,
    secretKeyProtection: null,
  });
  const user = await openDialog();
  core.addDeviceStatus.mockResolvedValue(asked);
  await vi.advanceTimersByTimeAsync(3100);
  const dialog = await screen.findByRole("dialog");
  await within(dialog).findByText('Add "Home Laptop"?');
  expect(within(dialog).queryByLabelText("Secret Key group 1 of 8")).toBeNull();
  expect(within(dialog).queryByText(/Touch ID/)).toBeNull();
  core.addDeviceApprove.mockResolvedValue([]);
  await user.click(within(dialog).getByRole("button", { name: "Approve" }));
  expect(await within(dialog).findByText("Enter your master password.")).toBeTruthy();
  expect(core.addDeviceApprove).not.toHaveBeenCalled();
  await user.type(within(dialog).getByLabelText("Master password"), "correct horse battery 42");
  await user.click(within(dialog).getByRole("button", { name: "Approve" }));
  await waitFor(() =>
    expect(core.addDeviceApprove).toHaveBeenCalledWith("correct horse battery 42", ""),
  );
});

test("a code that ran out offers a new one", async () => {
  const user = await openDialog();
  core.addDeviceStatus.mockResolvedValue({ ...waiting, state: "expired" });
  await vi.advanceTimersByTimeAsync(3100);
  expect(await screen.findByText("This code has expired")).toBeTruthy();
  const second = { ...invite, code: "WXYZ-2345-6789-ABCD-EFGH" };
  core.addDeviceStart.mockResolvedValue(second);
  core.addDeviceStatus.mockResolvedValue(waiting);
  await user.click(screen.getByRole("button", { name: "New code" }));
  expect((await screen.findByTestId("invite-code")).textContent?.trim()).toBe(second.code);
});

test("closing the dialog cancels the open invite", async () => {
  const user = await openDialog();
  await user.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(core.addDeviceCancel).toHaveBeenCalledTimes(1);
});

test("an offline start is explained and leaves no code", async () => {
  core.addDeviceStart.mockRejectedValue({
    code: "offline",
    message: "FigLock could not reach Google Drive, so the new device could not find the invite.",
  });
  const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
  render(Workspace, { onlock: vi.fn(), onlocked: vi.fn() });
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await user.click(await screen.findByRole("button", { name: "Add a device" }));
  expect(await screen.findByText(/could not reach Google Drive/)).toBeTruthy();
  expect(screen.queryByTestId("invite-code")).toBeNull();
});
