import { render, screen, waitFor } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

/** A stand-in for the app core's Secret Key commands. */
const core = vi.hoisted(() => ({
  vaultState: vi.fn(),
  showSecretKey: vi.fn(),
  forgetSecretKey: vi.fn(),
  syncStatus: vi.fn(),
  onSynced: vi.fn(),
  listItems: vi.fn(),
  listDevices: vi.fn(),
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
  lock: () => Promise.resolve(),
  vaultState: core.vaultState,
  showSecretKey: core.showSecretKey,
  forgetSecretKey: core.forgetSecretKey,
}));

import Workspace from "./Workspace.svelte";

const KEY = "FL1-ABCD-EFGH-JKMN-PQRS-TVWX-YZ23-4567-89AB";
const notKept = { secretKeyKept: false, secretKeyProtection: null, touchAvailable: true };
const kept = { secretKeyKept: true, secretKeyProtection: "secure-enclave" };

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  vi.clearAllMocks();
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
  core.vaultState.mockResolvedValue(kept);
});

afterEach(() => vi.useRealTimers());

async function toSettings() {
  const user = userEvent.setup();
  render(Workspace, { onlock: vi.fn(), onlocked: vi.fn() });
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await screen.findByText("Secret Key");
  return user;
}

test("a kept key says where it is protected, and showing it needs the prompt", async () => {
  core.showSecretKey.mockResolvedValue(KEY);
  const user = await toSettings();
  expect((await screen.findByTestId("key-where")).textContent).toContain("Secure Enclave");
  expect(screen.queryByTestId("shown-key")).toBeNull();
  await user.click(screen.getByRole("button", { name: "Show Secret Key" }));
  expect((await screen.findByTestId("shown-key")).textContent).toBe(KEY);
  await user.click(screen.getByRole("button", { name: "Hide" }));
  expect(screen.queryByTestId("shown-key")).toBeNull();
});

test("a declined prompt shows no key and says why", async () => {
  core.showSecretKey.mockRejectedValue({
    code: "not_confirmed",
    message:
      "The Secret Key was not shown because Touch ID or the device password was not confirmed.",
  });
  const user = await toSettings();
  await user.click(await screen.findByRole("button", { name: "Show Secret Key" }));
  expect(await screen.findByText(/was not confirmed/)).toBeTruthy();
  expect(screen.queryByTestId("shown-key")).toBeNull();
});

test("a shown key hides itself after 30 seconds", async () => {
  core.showSecretKey.mockResolvedValue(KEY);
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
  render(Workspace, { onlock: vi.fn(), onlocked: vi.fn() });
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await user.click(await screen.findByRole("button", { name: "Show Secret Key" }));
  await screen.findByTestId("shown-key");
  await vi.advanceTimersByTimeAsync(30_001);
  await waitFor(() => expect(screen.queryByTestId("shown-key")).toBeNull());
});

test("stopping the key being kept asks first, then says the key is typed from now on", async () => {
  core.forgetSecretKey.mockResolvedValue({ ...notKept, quickUnlock: true, secretKeyHeld: true });
  const user = await toSettings();
  await user.click(await screen.findByRole("button", { name: "Stop keeping it on this Mac" }));
  expect(core.forgetSecretKey).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Stop keeping it" }));
  expect(await screen.findByText("Not kept on this Mac")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Show Secret Key" })).toBeNull();
});

test("a Mac that keeps no key offers no way to show one", async () => {
  core.vaultState.mockResolvedValue({ ...notKept, quickUnlock: false });
  await toSettings();
  expect(await screen.findByText("Not kept on this Mac")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Show Secret Key" })).toBeNull();
});

test("with Touch ID off the copy says why, and that the session holds the key until lock", async () => {
  core.vaultState.mockResolvedValue({ ...notKept, quickUnlock: false, secretKeyHeld: true });
  await toSettings();
  const text = (await screen.findByTestId("key-why")).textContent ?? "";
  expect(text).toContain("Touch ID is off");
  expect(text).toContain("each time you unlock");
  expect(text).toContain("holds it until you lock");
  expect(text).not.toContain("kept again");
});

test("a Mac with no secure store says so, and holds nothing it does not hold", async () => {
  core.vaultState.mockResolvedValue({
    ...notKept,
    touchAvailable: false,
    quickUnlock: false,
    secretKeyHeld: false,
  });
  await toSettings();
  const text = (await screen.findByTestId("key-why")).textContent ?? "";
  expect(text).toContain("no secure place");
  expect(text).not.toContain("holds it");
});

test("after stopping the key being kept, the copy says the person did", async () => {
  core.vaultState.mockResolvedValue({ ...notKept, quickUnlock: true, secretKeyHeld: true });
  await toSettings();
  expect((await screen.findByTestId("key-why")).textContent).toContain("You stopped keeping it");
});
