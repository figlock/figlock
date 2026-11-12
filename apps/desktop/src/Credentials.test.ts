import { render, screen, waitFor, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";

import type { SyncView } from "./sync";

/** A stand-in for the app core's credential commands. */
const core = vi.hoisted(() => ({
  changeCredentials: vi.fn(),
  endRotation: vi.fn(),
  listDevices: vi.fn(),
  syncStatus: vi.fn(),
  onSynced: vi.fn(),
  listItems: vi.fn(),
  lock: vi.fn(),
  vaultState: vi.fn(),
  saveEmergencyKit: vi.fn(),
  checkKitKey: vi.fn(),
}));

vi.mock("./credentials", () => ({
  changeCredentials: core.changeCredentials,
  endRotation: core.endRotation,
}));
vi.mock("./devices", () => ({ listDevices: core.listDevices, revokeDevice: vi.fn() }));
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
  saveEmergencyKit: core.saveEmergencyKit,
  checkKitKey: core.checkKitKey,
}));

import Workspace from "./Workspace.svelte";

const OLD_KEY = "FL1-ABCD-EFGH-JKMN-PQRS-TVWX-YZ23-4567-89AB";
const NEW_KEY = "FL1-2222-3333-4444-5555-6666-7777-8888-9999";
const PASSWORD = "correct horse battery 42";

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  vi.clearAllMocks();
  core.lock.mockResolvedValue(undefined);
  core.listItems.mockResolvedValue([]);
  core.listDevices.mockResolvedValue([]);
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
  core.vaultState.mockResolvedValue({ secretKeyKept: true, secretKeyProtection: "keychain" });
});

async function toSettings() {
  const user = userEvent.setup();
  const onlocked = vi.fn();
  render(Workspace, { onlock: vi.fn(), onlocked });
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await screen.findByRole("heading", { name: "Password and Secret Key" });
  return Object.assign(user, { onlocked });
}

test("changing the password needs the current one and a matching new one", async () => {
  const user = await toSettings();
  await user.click(screen.getByRole("button", { name: "Change master password" }));
  const dialog = screen.getByRole("dialog");
  // This Mac keeps the Secret Key, so only passwords are asked for.
  expect(within(dialog).queryByLabelText("Secret Key group 1 of 8")).toBeNull();
  await user.type(within(dialog).getByLabelText("Current master password"), PASSWORD);
  await user.type(within(dialog).getByLabelText("New master password"), "brand new password 7");
  await user.type(within(dialog).getByLabelText("Repeat new master password"), "something else 1");
  await user.click(within(dialog).getByRole("button", { name: "Change password" }));
  expect(await screen.findByText("The two passwords do not match.")).toBeTruthy();
  expect(core.changeCredentials).not.toHaveBeenCalled();

  core.changeCredentials.mockResolvedValue({ secretKey: null });
  const repeat = within(dialog).getByLabelText("Repeat new master password");
  await user.clear(repeat);
  await user.type(repeat, "brand new password 7");
  await user.click(within(dialog).getByRole("button", { name: "Change password" }));
  await waitFor(() =>
    expect(core.changeCredentials).toHaveBeenCalledWith(
      PASSWORD,
      "",
      "brand new password 7",
      false,
    ),
  );
  expect(await screen.findByText(/master password is changed/)).toBeTruthy();
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("a wrong current password stays in the dialog", async () => {
  const user = await toSettings();
  await user.click(screen.getByRole("button", { name: "Change master password" }));
  const dialog = screen.getByRole("dialog");
  core.changeCredentials.mockRejectedValue({
    code: "wrong_credentials",
    message: "That master password and Secret Key do not match this vault.",
  });
  await user.type(within(dialog).getByLabelText("Current master password"), PASSWORD);
  await user.type(within(dialog).getByLabelText("New master password"), "brand new password 7");
  await user.type(
    within(dialog).getByLabelText("Repeat new master password"),
    "brand new password 7",
  );
  await user.click(within(dialog).getByRole("button", { name: "Change password" }));
  expect(await screen.findByText(/do not match this vault/)).toBeTruthy();
  expect(screen.getByRole("dialog")).toBeTruthy();
});

test("rotating the key shows the new key and cannot be closed until it is confirmed", async () => {
  const user = await toSettings();
  await user.click(screen.getByRole("button", { name: "Rotate Secret Key" }));
  let dialog = screen.getByRole("dialog");
  core.changeCredentials.mockResolvedValue({ secretKey: NEW_KEY });
  await user.type(within(dialog).getByLabelText("Current master password"), PASSWORD);
  await user.click(within(dialog).getByRole("button", { name: "Rotate key" }));
  await waitFor(() =>
    expect(core.changeCredentials).toHaveBeenCalledWith(PASSWORD, "", null, true),
  );

  dialog = await screen.findByRole("dialog", { name: "Save your new Secret Key" });
  expect(within(dialog).queryByRole("button", { name: "Cancel" })).toBeNull();
  await user.click(within(dialog).getByRole("button", { name: "Show" }));
  expect(within(dialog).getByTestId("new-secret-key").textContent).toContain("8888");

  core.saveEmergencyKit.mockResolvedValue(true);
  await user.click(within(dialog).getByRole("button", { name: "Save Emergency Kit" }));
  expect(await within(dialog).findByRole("button", { name: /Saved/ })).toBeTruthy();

  // A wrong confirmation keeps the step open and does not end the rotation.
  core.checkKitKey.mockResolvedValue(false);
  await user.type(within(dialog).getByLabelText("Confirm you saved it"), "AAAA-BBBB");
  await user.click(within(dialog).getByRole("button", { name: "Done" }));
  expect(await screen.findByText(/do not match the end of your new Secret Key/)).toBeTruthy();
  expect(core.endRotation).not.toHaveBeenCalled();

  core.checkKitKey.mockResolvedValue(true);
  core.endRotation.mockResolvedValue(undefined);
  const typed = within(dialog).getByLabelText("Confirm you saved it");
  await user.clear(typed);
  await user.type(typed, "8888-9999");
  await user.click(within(dialog).getByRole("button", { name: "Done" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(core.endRotation).toHaveBeenCalledTimes(1);
  expect(screen.getByText(/Secret Key is rotated/)).toBeTruthy();
});

test("a Mac that does not keep the key asks for it, and the old key is never shown again", async () => {
  core.vaultState.mockResolvedValue({ secretKeyKept: false, secretKeyProtection: null });
  const user = await toSettings();
  await user.click(screen.getByRole("button", { name: "Rotate Secret Key" }));
  const dialog = screen.getByRole("dialog");
  await user.type(within(dialog).getByLabelText("Current master password"), PASSWORD);
  await user.click(within(dialog).getByRole("button", { name: "Rotate key" }));
  expect(await screen.findByText(/looks incomplete/)).toBeTruthy();
  expect(core.changeCredentials).not.toHaveBeenCalled();

  core.changeCredentials.mockResolvedValue({ secretKey: NEW_KEY });
  await user.click(within(dialog).getByLabelText("Secret Key group 1 of 8"));
  await user.paste(OLD_KEY);
  await user.click(within(dialog).getByRole("button", { name: "Rotate key" }));
  await waitFor(() =>
    expect(core.changeCredentials).toHaveBeenCalledWith(PASSWORD, OLD_KEY, null, true),
  );
});

test("a refusal because settings changed elsewhere locks the vault and relocks", async () => {
  const user = await toSettings();
  await user.click(screen.getByRole("button", { name: "Change master password" }));
  const dialog = screen.getByRole("dialog");
  const message = "Another device changed this vault. Lock and unlock, then try again.";
  core.changeCredentials.mockRejectedValue({ code: "needs_unlock", message });
  await user.type(within(dialog).getByLabelText("Current master password"), PASSWORD);
  await user.type(within(dialog).getByLabelText("New master password"), "brand new password 7");
  await user.type(
    within(dialog).getByLabelText("Repeat new master password"),
    "brand new password 7",
  );
  await user.click(within(dialog).getByRole("button", { name: "Change password" }));
  await waitFor(() => expect(user.onlocked).toHaveBeenCalledWith(message));
  expect(core.lock).toHaveBeenCalledTimes(1);
});
