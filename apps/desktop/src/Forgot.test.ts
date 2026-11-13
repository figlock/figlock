import { render, screen, waitFor } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";

import type { RecoveryMaterial } from "./vault";

/** A stand-in for the app core's "I forgot my password" commands. */
const core = vi.hoisted(() => ({
  forgotCardInfo: vi.fn(),
  forgotFind: vi.fn(),
  forgotPrepare: vi.fn(),
  forgotCommit: vi.fn(),
  forgotCancel: vi.fn(),
  checkKitKey: vi.fn(),
  recoveryPrepare: vi.fn(),
  recoveryCheck: vi.fn(),
  recoveryCancel: vi.fn(),
  saveRecoveryKit: vi.fn(),
  saveEmergencyKit: vi.fn(),
}));

vi.mock("./vault", () => ({
  vaultState: () =>
    Promise.resolve({
      phase: "none",
      version: "9.9.9",
      quickUnlock: false,
      touchAvailable: false,
      autolockSecs: 300,
    }),
  onAutoLock: () => Promise.resolve(() => {}),
  activity: () => Promise.resolve(),
  lock: () => Promise.resolve(),
  suggestedFolder: () => Promise.resolve("/Users/test/FigLock"),
  chooseFolder: () => Promise.resolve("/Users/test/Vault"),
  driveStatus: () => Promise.resolve({ canSignIn: false, signedIn: false }),
  driveSignIn: vi.fn(),
  driveSignOut: vi.fn(),
  errorOf: (e: unknown) => e,
  ...core,
}));
vi.mock("./credentials", () => ({ endRotation: vi.fn() }));
vi.mock("./items", async (original) => ({
  ...(await original<typeof import("./items")>()),
  listItems: () => Promise.resolve([]),
  listDeleted: () => Promise.resolve([]),
}));

import App from "./App.svelte";

const CODE = "FLR1-5EV5-92S1-ZY3E-13NH-6AWA-FE6K-ZBQ1-VK55-EA1P-MHKR-J3GY-ZTAF-1V86-DRWY";
const NEW_KEY = "FL1-ABCD-EFGH-JKMN-PQRS-TVWX-YZ23-4567-89AB";
const PASSWORD = "a brand new passphrase 42";
const found = {
  name: "Personal",
  items: 12,
  lastChangeMs: Date.now() - 3 * 86_400_000,
  devices: 2,
};
const material: RecoveryMaterial = {
  mode: "code",
  code: CODE,
  cards: [],
  threshold: 0,
  count: 0,
  challenge: [],
};

beforeEach(() => {
  vi.clearAllMocks();
  core.forgotFind.mockResolvedValue(found);
  core.forgotPrepare.mockResolvedValue({ secretKey: NEW_KEY });
  core.forgotCommit.mockResolvedValue({ phase: "unlocked", version: "9.9.9" });
  core.forgotCancel.mockResolvedValue(undefined);
  core.checkKitKey.mockResolvedValue(true);
  core.recoveryPrepare.mockResolvedValue(material);
  core.recoveryCheck.mockResolvedValue(true);
  core.recoveryCancel.mockResolvedValue(undefined);
  core.saveEmergencyKit.mockResolvedValue(true);
  core.saveRecoveryKit.mockResolvedValue(true);
});

async function toForgot() {
  const user = userEvent.setup();
  render(App);
  await user.click(await screen.findByRole("button", { name: "I forgot my password" }));
  await screen.findByRole("heading", { name: "I forgot my password" });
  await user.click(screen.getByRole("button", { name: "Choose..." }));
  await screen.findByDisplayValue("/Users/test/Vault");
  return user;
}

async function openWithCode(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByLabelText("Recovery Key"), CODE);
  await user.click(screen.getByRole("button", { name: "Continue" }));
  await screen.findByText("Found your vault");
}

async function newPassword(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByLabelText("New master password"), PASSWORD);
  await user.type(screen.getByLabelText("Repeat new master password"), PASSWORD);
  await user.click(screen.getByRole("button", { name: "Continue" }));
  await screen.findByText("Make a new Recovery Key?");
}

test("a code opens the vault, shows what was found and asks for a new password", async () => {
  const user = await toForgot();
  await openWithCode(user);
  expect(core.forgotFind).toHaveBeenCalledWith({ path: "/Users/test/Vault" }, [CODE], false);
  const summary = screen.getByTestId("found").textContent ?? "";
  expect(summary).toContain("Personal");
  expect(summary).toContain("12 items");
  expect(summary).toContain("last changed 3 days ago");
  expect(summary).toContain("2 other devices");

  // Mismatched and short passwords go nowhere.
  await user.type(screen.getByLabelText("New master password"), "short");
  await user.type(screen.getByLabelText("Repeat new master password"), "short");
  await user.click(screen.getByRole("button", { name: "Continue" }));
  expect(await screen.findByText("Use at least 8 characters.")).toBeTruthy();
  expect(core.forgotPrepare).not.toHaveBeenCalled();
});

test("keeping the Recovery Key makes the new credentials, confirms the kit and opens the vault", async () => {
  const user = await toForgot();
  await openWithCode(user);
  await newPassword(user);
  await user.click(screen.getByRole("radio", { name: /Keep the one I just used/ }));
  await user.click(screen.getByRole("button", { name: "Continue" }));
  await screen.findByText("Save your new Secret Key");
  expect(core.forgotPrepare).toHaveBeenCalledWith(PASSWORD, false);
  expect(screen.getByText(/revoke any device you do not trust/)).toBeTruthy();

  // The key is hidden until shown, and the vault only opens once the kit is confirmed.
  expect(screen.getByTestId("secret-key").textContent).not.toContain("ABCD");
  await user.click(screen.getByRole("button", { name: "Save Emergency Kit" }));
  expect(core.saveEmergencyKit).toHaveBeenCalled();
  core.checkKitKey.mockResolvedValueOnce(false);
  await user.type(screen.getByLabelText("Confirm you saved it"), "0000-0000");
  await user.click(screen.getByRole("button", { name: "Open my vault" }));
  expect(await screen.findByText(/do not match the end of your Secret Key/)).toBeTruthy();
  expect(core.forgotCommit).not.toHaveBeenCalled();

  const field = screen.getByLabelText("Confirm you saved it");
  await user.clear(field);
  await user.type(field, "89AB-4567");
  await user.click(screen.getByRole("button", { name: "Open my vault" }));
  await waitFor(() => expect(core.forgotCommit).toHaveBeenCalledWith(false));
});

test("a new Recovery Key is shown and confirmed before it is used", async () => {
  const user = await toForgot();
  await openWithCode(user);
  await newPassword(user);
  // The default is a new key, and Continue is not offered until it is made.
  expect(screen.queryByRole("button", { name: "Continue" })).toBeNull();
  await user.click(screen.getByRole("button", { name: "Make a Recovery Key" }));
  await user.click(await screen.findByRole("button", { name: "Make Recovery Key" }));
  await user.click(await screen.findByRole("button", { name: "I have saved it" }));
  await user.type(screen.getByLabelText("Last two groups"), "drwy-zt1f");
  await user.click(screen.getByRole("button", { name: "Confirm" }));
  expect(await screen.findByText("Your new Recovery Key is ready.")).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "Continue" }));
  await screen.findByText("Save your new Secret Key");
  expect(core.forgotPrepare).toHaveBeenCalledWith(PASSWORD, true);
});

test("cards are read as they are typed: how many are needed, a typo, and then all of them", async () => {
  core.forgotCardInfo.mockImplementation((text: string) =>
    text.startsWith("bad")
      ? Promise.reject({ code: "bad_card", message: "This card has a typo or is incomplete." })
      : Promise.resolve({ needed: 2, number: text.startsWith("one") ? 1 : 2 }),
  );
  const user = await toForgot();
  await user.click(screen.getByRole("radio", { name: "Cards" }));
  const first = screen.getByLabelText("Card 1");
  await user.type(first, "bad words here");
  await user.tab();
  expect(await screen.findByText("This card has a typo or is incomplete.")).toBeTruthy();

  await user.clear(first);
  await user.type(first, "one two three");
  await user.tab();
  // Learning that two are needed makes room for the second.
  expect(await screen.findByText("2 cards are needed to open this vault.")).toBeTruthy();
  const second = await screen.findByLabelText("Card 2 of 2");
  await user.type(second, "two cards words");
  await user.tab();
  await user.click(screen.getByRole("button", { name: "Continue" }));
  await screen.findByText("Found your vault");
  expect(core.forgotFind).toHaveBeenCalledWith(
    { path: "/Users/test/Vault" },
    ["one two three", "two cards words"],
    true,
  );
});

test("one card of two is not enough to try", async () => {
  core.forgotCardInfo.mockResolvedValue({ needed: 2, number: 1 });
  const user = await toForgot();
  await user.click(screen.getByRole("radio", { name: "Cards" }));
  await user.type(screen.getByLabelText("Card 1"), "one two three");
  await user.tab();
  await user.click(screen.getByRole("button", { name: "Continue" }));
  expect(await screen.findByText("Enter every card that is needed.")).toBeTruthy();
  expect(core.forgotFind).not.toHaveBeenCalled();
});

test("a key that does not open the vault is explained and the person can try again", async () => {
  core.forgotFind.mockRejectedValueOnce({
    code: "recovery_failed",
    message: "This Recovery Key does not open that vault.",
  });
  const user = await toForgot();
  await user.type(screen.getByLabelText("Recovery Key"), CODE);
  await user.click(screen.getByRole("button", { name: "Continue" }));
  expect(await screen.findByText("This Recovery Key does not open that vault.")).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "Continue" }));
  await screen.findByText("Found your vault");
  expect(core.forgotFind).toHaveBeenCalledTimes(2);
});

test("going back drops the attempt", async () => {
  const user = await toForgot();
  await openWithCode(user);
  await user.click(screen.getByRole("button", { name: "Back" }));
  await waitFor(() => expect(core.forgotCancel).toHaveBeenCalled());
  expect(await screen.findByRole("button", { name: "Create a vault" })).toBeTruthy();
});
