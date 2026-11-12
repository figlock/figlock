import { render, screen, waitFor } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";

const state = vi.hoisted(() => ({
  current: {
    phase: "none",
    version: "9.9.9",
    quickUnlock: false,
    touchAvailable: false,
    autolockSecs: 300,
  } as Record<string, unknown>,
  unlock: vi.fn(),
  quickUnlock: vi.fn(),
  endRotation: vi.fn(),
  checkKitKey: vi.fn(),
}));

vi.mock("./vault", () => ({
  vaultState: () => Promise.resolve(state.current),
  onAutoLock: () => Promise.resolve(() => {}),
  activity: () => Promise.resolve(),
  lock: () => Promise.resolve(),
  unlock: state.unlock,
  quickUnlock: state.quickUnlock,
  suggestedFolder: () => Promise.resolve("/Users/test/FigLock"),
  chooseFolder: () => Promise.resolve(null),
  errorOf: (e: unknown) => e,
  saveEmergencyKit: () => Promise.resolve(true),
  checkKitKey: state.checkKitKey,
}));

vi.mock("./credentials", () => ({ endRotation: state.endRotation }));

vi.mock("./items", async (original) => ({
  ...(await original<typeof import("./items")>()),
  listItems: () => Promise.resolve([]),
  listDeleted: () => Promise.resolve([]),
}));

import App from "./App.svelte";

const KEY = "FL1-ABCD-EFGH-JKMN-PQRS-TVWX-YZ23-4567-89AB";

beforeEach(() => {
  state.unlock.mockReset();
  state.quickUnlock.mockReset();
  state.current = {
    phase: "none",
    version: "9.9.9",
    quickUnlock: false,
    touchAvailable: false,
    autolockSecs: 300,
  };
});

test("a device with no vault is offered the three ways to start", async () => {
  render(App);
  expect(await screen.findByRole("button", { name: "Create a vault" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Open an existing vault" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Add this Mac with an invite code" })).toBeTruthy();
});

test("create asks for a folder and matching passwords before it goes on", async () => {
  const user = userEvent.setup();
  render(App);
  await user.click(await screen.findByRole("button", { name: "Create a vault" }));
  expect(await screen.findByDisplayValue("/Users/test/FigLock")).toBeTruthy();

  await user.type(screen.getByLabelText("Master password"), "short");
  await user.type(screen.getByLabelText("Repeat master password"), "short");
  await user.click(screen.getByRole("button", { name: "Continue" }));
  expect(await screen.findByText("Use at least 8 characters.")).toBeTruthy();
});

test("a locked vault shows the unlock form and checks the Secret Key is complete", async () => {
  state.current = { ...state.current, phase: "locked" };
  const user = userEvent.setup();
  render(App);
  await user.type(await screen.findByLabelText("Master password"), "correct horse");
  await user.click(screen.getByRole("button", { name: "Unlock" }));
  expect(await screen.findByText(/This Secret Key looks incomplete/)).toBeTruthy();
  expect(state.unlock).not.toHaveBeenCalled();
  expect(screen.getByText(/locks itself after 5 minutes/)).toBeTruthy();
});

test("a pasted Secret Key fills every group, and a wrong password shows the error", async () => {
  state.current = { ...state.current, phase: "locked" };
  state.unlock.mockRejectedValue({
    code: "wrong_credentials",
    message: "That master password and Secret Key do not match this vault.",
  });
  const user = userEvent.setup();
  render(App);
  await user.type(await screen.findByLabelText("Master password"), "wrong password");
  await user.click(screen.getByLabelText("Secret Key group 1 of 8"));
  await user.paste(KEY);
  const groups = KEY.split("-").slice(1);
  groups.forEach((g, i) =>
    expect(
      (screen.getByLabelText(`Secret Key group ${i + 1} of 8`) as HTMLInputElement).value,
    ).toBe(g),
  );

  await user.click(screen.getByRole("button", { name: "Unlock" }));
  expect(await screen.findByText(/do not match this vault/)).toBeTruthy();
  expect(state.unlock).toHaveBeenCalledWith("wrong password", KEY);
});

test("a Mac that keeps the Secret Key asks for the master password only", async () => {
  state.current = { ...state.current, phase: "locked", secretKeyKept: true };
  state.unlock.mockResolvedValue({ ...state.current, phase: "unlocked" });
  const user = userEvent.setup();
  render(App);
  await user.type(await screen.findByLabelText("Master password"), "correct horse battery 42");
  expect(screen.queryByLabelText("Secret Key group 1 of 8")).toBeNull();
  expect(screen.getByText(/keeps your Secret Key/)).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "Unlock" }));
  await waitFor(() =>
    expect(state.unlock).toHaveBeenCalledWith("correct horse battery 42", undefined),
  );
});

test("when the kept key does not open the vault the Secret Key is asked for, and a retry sends it", async () => {
  state.current = { ...state.current, phase: "locked", secretKeyKept: true };
  state.unlock.mockRejectedValueOnce({
    code: "needs_secret_key",
    message:
      "That did not open the vault. If your master password is right, the Secret Key was changed.",
  });
  const user = userEvent.setup();
  render(App);
  await user.type(await screen.findByLabelText("Master password"), "correct horse battery 42");
  await user.click(screen.getByRole("button", { name: "Unlock" }));
  expect(await screen.findByText(/the Secret Key was changed/)).toBeTruthy();
  await user.click(screen.getByLabelText("Secret Key group 1 of 8"));
  await user.paste(KEY);
  state.unlock.mockResolvedValueOnce({ ...state.current, phase: "unlocked" });
  await user.click(screen.getByRole("button", { name: "Unlock" }));
  await waitFor(() =>
    expect(state.unlock).toHaveBeenLastCalledWith("correct horse battery 42", KEY),
  );
});

test("Touch ID is only offered when quick unlock is on, and its failure asks for the credentials", async () => {
  state.current = { ...state.current, phase: "locked" };
  const user = userEvent.setup();
  const { unmount } = render(App);
  await screen.findByLabelText("Master password");
  expect(screen.queryByRole("button", { name: "Use Touch ID" })).toBeNull();
  unmount();

  state.current = { ...state.current, quickUnlock: true };
  state.quickUnlock.mockRejectedValue({
    code: "quick_unavailable",
    message: "Touch ID did not unlock the vault. Type your master password and Secret Key.",
  });
  render(App);
  await user.click(await screen.findByRole("button", { name: "Use Touch ID" }));
  expect(await screen.findByText(/Type your master password and Secret Key/)).toBeTruthy();
});

test("unlocking moves to the vault and locking comes back to the unlock form", async () => {
  state.current = { ...state.current, phase: "locked" };
  state.unlock.mockImplementation(() => {
    state.current = { ...state.current, phase: "unlocked" };
    return Promise.resolve(state.current);
  });
  const user = userEvent.setup();
  render(App);
  await user.type(await screen.findByLabelText("Master password"), "correct horse");
  await user.click(screen.getByLabelText("Secret Key group 1 of 8"));
  await user.paste(KEY);
  await user.click(screen.getByRole("button", { name: "Unlock" }));
  expect(await screen.findByText("Your vault is empty")).toBeTruthy();

  state.current = { ...state.current, phase: "locked" };
  await user.click(screen.getByRole("button", { name: "Lock" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Unlock" })).toBeTruthy());
});

test("typing the key one character at a time, prefix included, lands the groups in order", async () => {
  state.current = { ...state.current, phase: "locked" };
  const user = userEvent.setup();
  render(App);
  await user.click(await screen.findByLabelText("Secret Key group 1 of 8"));
  await user.keyboard("FL1-ABCD-EFGH");
  const value = (i: number) =>
    (screen.getByLabelText(`Secret Key group ${i} of 8`) as HTMLInputElement).value;
  expect([value(1), value(2), value(3)]).toEqual(["ABCD", "EFGH", ""]);
});

test("unlocking with a rotation's kit step unfinished shows it again until it is confirmed", async () => {
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  const NEW_KEY = "FL1-2222-3333-4444-5555-6666-7777-8888-9999";
  state.current = { ...state.current, phase: "locked", secretKeyKept: true };
  state.unlock.mockResolvedValue({ ...state.current, phase: "unlocked", pendingKit: NEW_KEY });
  state.checkKitKey.mockResolvedValue(true);
  state.endRotation.mockResolvedValue(undefined);
  const user = userEvent.setup();
  render(App);
  await user.type(await screen.findByLabelText("Master password"), "correct horse");
  await user.click(screen.getByRole("button", { name: "Unlock" }));
  const dialog = await screen.findByRole("dialog", { name: "Save your new Secret Key" });
  expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
  await user.type(screen.getByLabelText("Confirm you saved it"), "8888-9999");
  await user.click(screen.getByRole("button", { name: "Done" }));
  await waitFor(() => expect(dialog.isConnected).toBe(false));
  expect(state.endRotation).toHaveBeenCalledTimes(1);
});
