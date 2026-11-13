import { render, screen, waitFor, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";

import type { RecoveryMaterial } from "./vault";

/** A stand-in for the app core's setup and Recovery Key commands. */
const core = vi.hoisted(() => ({
  createVault: vi.fn(),
  checkKitKey: vi.fn(),
  finishCreate: vi.fn(),
  cancelCreate: vi.fn(),
  recoveryPrepare: vi.fn(),
  recoveryCheck: vi.fn(),
  saveRecoveryKit: vi.fn(),
  recoveryCancel: vi.fn(),
  recoverySkip: vi.fn(),
  recoveryUseDraft: vi.fn(),
  recoveryEnable: vi.fn(),
  recoveryDisable: vi.fn(),
  vaultState: vi.fn(),
  lock: vi.fn(),
  syncStatus: vi.fn(),
  onSynced: vi.fn(),
  listItems: vi.fn(),
  listDevices: vi.fn(),
}));

vi.mock("./vault", () => ({
  vaultState: core.vaultState,
  onAutoLock: () => Promise.resolve(() => {}),
  activity: () => Promise.resolve(),
  lock: core.lock,
  suggestedFolder: () => Promise.resolve("/Users/test/FigLock"),
  chooseFolder: () => Promise.resolve(null),
  driveStatus: () => Promise.resolve({ canSignIn: false, signedIn: false }),
  driveSignIn: vi.fn(),
  driveSignOut: vi.fn(),
  errorOf: (e: unknown) => e,
  createVault: core.createVault,
  saveEmergencyKit: () => Promise.resolve(true),
  checkKitKey: core.checkKitKey,
  finishCreate: core.finishCreate,
  cancelCreate: core.cancelCreate,
  recoveryPrepare: core.recoveryPrepare,
  recoveryCheck: core.recoveryCheck,
  saveRecoveryKit: core.saveRecoveryKit,
  recoveryCancel: core.recoveryCancel,
  recoverySkip: core.recoverySkip,
  recoveryUseDraft: core.recoveryUseDraft,
  recoveryEnable: core.recoveryEnable,
  recoveryDisable: core.recoveryDisable,
}));
vi.mock("./credentials", () => ({ endRotation: vi.fn() }));
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

import App from "./App.svelte";
import Workspace from "./Workspace.svelte";

const KEY = "FL1-ABCD-EFGH-JKMN-PQRS-TVWX-YZ23-4567-89AB";
const CODE = "FLR1-5EV5-92S1-ZY3E-13NH-6AWA-FE6K-ZBQ1-VK55-EA1P-MHKR-J3GY-ZTAF-1V86-DRWY";

const code: RecoveryMaterial = {
  mode: "code",
  code: CODE,
  cards: [],
  threshold: 0,
  count: 0,
  challenge: [],
};
const words = (n: number) => Array.from({ length: 33 }, (_, i) => `w${n}x${i + 1}`);
const shares: RecoveryMaterial = {
  mode: "shares",
  code: null,
  cards: [words(1), words(2), words(3)],
  threshold: 2,
  count: 3,
  challenge: [4, 9, 20],
};

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  vi.clearAllMocks();
  core.vaultState.mockResolvedValue({
    phase: "none",
    version: "9.9.9",
    quickUnlock: false,
    touchAvailable: false,
    autolockSecs: 300,
    recoveryEnabled: false,
  });
  core.createVault.mockResolvedValue({ secretKey: KEY });
  core.checkKitKey.mockResolvedValue(true);
  core.finishCreate.mockResolvedValue({
    phase: "unlocked",
    version: "9.9.9",
    quickUnlock: false,
    touchAvailable: false,
    autolockSecs: 300,
    recoveryEnabled: true,
  });
  core.cancelCreate.mockResolvedValue(undefined);
  core.recoveryCancel.mockResolvedValue(undefined);
  core.recoverySkip.mockResolvedValue(undefined);
  core.recoveryUseDraft.mockResolvedValue(undefined);
  core.saveRecoveryKit.mockResolvedValue(true);
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
  });
});

/** Create a vault through the first two steps and stop at the recovery choice. */
async function toRecoveryStep() {
  const user = userEvent.setup();
  render(App);
  await user.click(await screen.findByRole("button", { name: "Create a vault" }));
  await screen.findByDisplayValue("/Users/test/FigLock");
  await user.type(screen.getByLabelText("Master password"), "correct horse battery 42");
  await user.type(screen.getByLabelText("Repeat master password"), "correct horse battery 42");
  await user.click(screen.getByRole("button", { name: "Continue" }));
  await user.type(await screen.findByLabelText("Confirm you saved it"), "89AB-4567");
  await user.click(screen.getByRole("button", { name: "Continue" }));
  await screen.findByText("Add a Recovery Key?");
  return user;
}

test("the vault is not created until a recovery choice is made", async () => {
  await toRecoveryStep();
  // Both ways forward are offered, and neither has happened yet.
  expect(screen.getByRole("button", { name: "Add a Recovery Key" })).toBeTruthy();
  const without = screen.getByRole("button", { name: "Create vault without a Recovery Key" });
  expect((without as HTMLButtonElement).disabled).toBe(true);
  expect(core.finishCreate).not.toHaveBeenCalled();
});

test("going without needs the acknowledgement, then creates the vault", async () => {
  const user = await toRecoveryStep();
  const without = screen.getByRole("button", { name: "Create vault without a Recovery Key" });
  await user.click(screen.getByRole("checkbox", { name: /cannot be recovered/ }));
  expect((without as HTMLButtonElement).disabled).toBe(false);
  await user.click(without);
  await waitFor(() => expect(core.finishCreate).toHaveBeenCalledTimes(1));
  expect(core.recoverySkip).toHaveBeenCalledWith(true);
  expect(core.recoveryUseDraft).not.toHaveBeenCalled();
});

test("a Recovery Key code is shown, checked by typing its end back, and then the vault is created", async () => {
  core.recoveryPrepare.mockResolvedValue(code);
  const user = await toRecoveryStep();
  await user.click(screen.getByRole("button", { name: "Add a Recovery Key" }));
  await user.click(screen.getByRole("button", { name: "Make Recovery Key" }));
  expect(core.recoveryPrepare).toHaveBeenCalledWith(false, 2, 3);
  expect((await screen.findByTestId("recovery-code")).textContent?.trim()).toBe(CODE);
  await user.click(screen.getByRole("button", { name: "Save as PDF" }));
  expect(core.saveRecoveryKit).toHaveBeenCalledTimes(1);
  await user.click(screen.getByRole("button", { name: "I have saved it" }));

  // A wrong answer does not confirm and nothing is used.
  core.recoveryCheck.mockResolvedValueOnce(false);
  await user.type(screen.getByLabelText("Last two groups"), "AAAA-AAAA");
  await user.click(screen.getByRole("button", { name: "Confirm" }));
  expect(await screen.findByText(/do not match the end of the Recovery Key/)).toBeTruthy();
  expect(core.recoveryUseDraft).not.toHaveBeenCalled();

  core.recoveryCheck.mockResolvedValueOnce(true);
  const field = screen.getByLabelText("Last two groups");
  await user.clear(field);
  await user.type(field, "drwy-zt1f");
  await user.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() => expect(core.recoveryUseDraft).toHaveBeenCalledTimes(1));
  expect(core.recoveryCheck).toHaveBeenLastCalledWith(["drwy-zt1f"]);
  await user.click(await screen.findByRole("button", { name: "Create vault" }));
  await waitFor(() => expect(core.finishCreate).toHaveBeenCalledTimes(1));
  expect(core.recoverySkip).not.toHaveBeenCalled();
});

test("cards show every word and ask for one word from each", async () => {
  core.recoveryPrepare.mockResolvedValue(shares);
  const user = await toRecoveryStep();
  await user.click(screen.getByRole("button", { name: "Add a Recovery Key" }));
  await user.click(screen.getByRole("radio", { name: /Cards/ }));
  await user.click(screen.getByRole("button", { name: "Make Recovery Key" }));
  expect(core.recoveryPrepare).toHaveBeenCalledWith(true, 2, 3);
  const cards = await screen.findAllByTestId("recovery-card");
  expect(cards).toHaveLength(3);
  expect(within(cards[1]).getByText("Card 2 of 3")).toBeTruthy();
  expect(within(cards[1]).getByText("w2x33")).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "I have saved it" }));
  // Which word of which card is asked for comes from the app core.
  const asked = ["Card 1, word 4", "Card 2, word 9", "Card 3, word 20"];
  for (const [i, label] of asked.entries()) {
    await user.type(screen.getByLabelText(label), `w${i + 1}x${[4, 9, 20][i]}`);
  }
  core.recoveryCheck.mockResolvedValue(true);
  await user.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() => expect(core.recoveryUseDraft).toHaveBeenCalled());
  expect(core.recoveryCheck).toHaveBeenCalledWith(["w1x4", "w2x9", "w3x20"]);
});

test("an empty answer is not sent to be checked", async () => {
  core.recoveryPrepare.mockResolvedValue(code);
  const user = await toRecoveryStep();
  await user.click(screen.getByRole("button", { name: "Add a Recovery Key" }));
  await user.click(screen.getByRole("button", { name: "Make Recovery Key" }));
  await user.click(await screen.findByRole("button", { name: "I have saved it" }));
  await user.click(screen.getByRole("button", { name: "Confirm" }));
  expect(await screen.findByText("Fill in every box.")).toBeTruthy();
  expect(core.recoveryCheck).not.toHaveBeenCalled();
});

async function toSettings(enabled: boolean, keyKept = true) {
  core.vaultState.mockResolvedValue({
    phase: "unlocked",
    version: "9.9.9",
    secretKeyKept: keyKept,
    recoveryEnabled: enabled,
  });
  const user = userEvent.setup();
  const onlocked = vi.fn();
  render(Workspace, { onlock: vi.fn(), onlocked });
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await screen.findByRole("heading", { name: "Recovery Key" });
  return Object.assign(user, { onlocked });
}

test("Settings shows when there is no Recovery Key and sets one up with the credentials", async () => {
  core.recoveryPrepare.mockResolvedValue(code);
  core.recoveryCheck.mockResolvedValue(true);
  core.recoveryEnable.mockResolvedValue(undefined);
  const user = await toSettings(false);
  expect(await screen.findByText(/your data cannot be recovered/)).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "Set up" }));
  const dialog = await screen.findByRole("dialog");
  await user.click(within(dialog).getByRole("button", { name: "Make Recovery Key" }));
  await user.click(await within(dialog).findByRole("button", { name: "I have saved it" }));
  await user.type(within(dialog).getByLabelText("Last two groups"), "drwy-zt1f");
  await user.click(within(dialog).getByRole("button", { name: "Confirm" }));
  // This Mac keeps the Secret Key, so only the master password is asked for.
  await user.type(await within(dialog).findByLabelText("Master password"), "correct horse 42");
  expect(within(dialog).queryByLabelText("Secret Key group 1 of 8")).toBeNull();
  core.vaultState.mockResolvedValue({
    phase: "unlocked",
    secretKeyKept: true,
    recoveryEnabled: true,
  });
  await user.click(within(dialog).getByRole("button", { name: "Save Recovery Key" }));
  await waitFor(() => expect(core.recoveryEnable).toHaveBeenCalledWith("correct horse 42", ""));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(screen.getByText(/The Recovery Key is set up/)).toBeTruthy();
  expect(screen.getByRole("button", { name: "Replace" })).toBeTruthy();
});

test("a Mac that keeps no Secret Key asks for it, and a wrong pair stays in the dialog", async () => {
  core.recoveryPrepare.mockResolvedValue(code);
  core.recoveryCheck.mockResolvedValue(true);
  core.recoveryEnable.mockRejectedValue({
    code: "wrong_credentials",
    message: "That master password and Secret Key do not match this vault.",
  });
  const user = await toSettings(false, false);
  await user.click(screen.getByRole("button", { name: "Set up" }));
  const dialog = await screen.findByRole("dialog");
  await user.click(within(dialog).getByRole("button", { name: "Make Recovery Key" }));
  await user.click(await within(dialog).findByRole("button", { name: "I have saved it" }));
  await user.type(within(dialog).getByLabelText("Last two groups"), "drwy-zt1f");
  await user.click(within(dialog).getByRole("button", { name: "Confirm" }));
  await user.type(await within(dialog).findByLabelText("Master password"), "wrong password");
  await user.click(within(dialog).getByLabelText("Secret Key group 1 of 8"));
  await user.paste(KEY);
  await user.click(within(dialog).getByRole("button", { name: "Save Recovery Key" }));
  expect(await within(dialog).findByText(/do not match this vault/)).toBeTruthy();
  expect(core.recoveryEnable).toHaveBeenCalledWith("wrong password", KEY);
  expect(screen.getByRole("dialog")).toBeTruthy();
});

test("turning it off warns, asks for the master password and then says so", async () => {
  core.recoveryDisable.mockResolvedValue(undefined);
  const user = await toSettings(true);
  await user.click(screen.getByRole("button", { name: "Turn off" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText(/Any code or cards you made stop working/)).toBeTruthy();
  await user.type(within(dialog).getByLabelText("Master password"), "correct horse 42");
  core.vaultState.mockResolvedValue({
    phase: "unlocked",
    secretKeyKept: true,
    recoveryEnabled: false,
  });
  await user.click(within(dialog).getByRole("button", { name: "Turn off" }));
  await waitFor(() => expect(core.recoveryDisable).toHaveBeenCalledWith("correct horse 42", ""));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(screen.getByText("The Recovery Key is turned off.")).toBeTruthy();
});

test("cancelling the setup dialog drops the key that was made", async () => {
  core.recoveryPrepare.mockResolvedValue(code);
  const user = await toSettings(false);
  await user.click(screen.getByRole("button", { name: "Set up" }));
  const dialog = await screen.findByRole("dialog");
  await user.click(within(dialog).getByRole("button", { name: "Make Recovery Key" }));
  await within(dialog).findByTestId("recovery-code");
  await user.click(within(dialog).getByRole("button", { name: "Start over" }));
  expect(core.recoveryCancel).toHaveBeenCalled();
});
