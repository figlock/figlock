import { render, screen, waitFor } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";

import type { RecoveryMaterial } from "./vault";

/** The Unlock screen's way back in for a Mac that already has the vault (FIG-139). */
const core = vi.hoisted(() => ({
  forgotFind: vi.fn(),
  forgotPrepare: vi.fn(),
  forgotCommit: vi.fn(),
  forgotCancel: vi.fn(),
  checkKitKey: vi.fn(),
  recoveryPrepare: vi.fn(),
  recoveryCheck: vi.fn(),
  recoveryCancel: vi.fn(),
  saveRecoveryKit: vi.fn(),
  forgetLockedVault: vi.fn(),
  saveEmergencyKit: vi.fn(),
}));

vi.mock("./vault", () => ({
  vaultState: () =>
    Promise.resolve({
      phase: "locked",
      secretKeyKept: true,
      secretKeyStale: false,
      folderMissing: null,
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
const material: RecoveryMaterial = { code: CODE };

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function () {
    this.removeAttribute("open");
  };
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

async function toHelp() {
  const user = userEvent.setup();
  render(App);
  await screen.findByRole("heading", { name: "Unlock your vault" });
  await user.click(screen.getByRole("button", { name: "I forgot my password or Secret Key" }));
  await screen.findByRole("heading", { name: "Can't unlock?" });
  return user;
}

test("the help screen lists the cases and says plainly that no copy means no way back", async () => {
  await toHelp();
  const text = document.body.textContent ?? "";
  expect(text).toContain("Another device is unlocked");
  expect(text).toContain("You have a Recovery Key");
  expect(text).toContain("the vault cannot be opened");
  expect(text).toContain("no way to reset it, by design");
  expect(screen.getByRole("heading", { name: "I lost my Secret Key" })).toBeTruthy();
});

test("a Recovery Key opens this Mac's own vault with no storage to pick, and a new password follows", async () => {
  const user = await toHelp();
  await user.click(screen.getByRole("button", { name: "Use my Recovery Key" }));
  await screen.findByRole("heading", { name: "I forgot my password" });
  expect(screen.queryByRole("button", { name: "Choose..." })).toBeNull();
  await user.type(screen.getByLabelText("Recovery Key"), CODE);
  await user.click(screen.getByRole("button", { name: "Continue" }));
  await screen.findByText("Found your vault");
  expect(core.forgotFind).toHaveBeenCalledWith({}, CODE);
  await user.type(screen.getByLabelText("New password"), PASSWORD);
  await user.type(screen.getByLabelText("Repeat new password"), PASSWORD);
  await user.click(screen.getByRole("button", { name: "Continue" }));
  await screen.findByText("Make a new Recovery Key?");
});

test("removing the vault from this Mac asks first and deletes nothing when cancelled", async () => {
  const user = await toHelp();
  core.forgetLockedVault.mockResolvedValue(undefined);
  await user.click(screen.getByRole("button", { name: "Remove this vault from this Mac" }));
  await user.click(await screen.findByRole("button", { name: "Cancel" }));
  expect(core.forgetLockedVault).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Remove this vault from this Mac" }));
  await user.click(await screen.findByRole("button", { name: "Remove from this Mac" }));
  await waitFor(() => expect(core.forgetLockedVault).toHaveBeenCalledWith(true));
});

test("back returns to the Unlock screen", async () => {
  const user = await toHelp();
  await user.click(screen.getByRole("button", { name: "Back to unlock" }));
  await screen.findByRole("heading", { name: "Unlock your vault" });
});
