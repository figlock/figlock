import { render, screen, waitFor, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";

import type { SyncView } from "./sync";

/** A stand-in for the app core's sync commands. */
const core = vi.hoisted(() => ({
  syncStatus: vi.fn(),
  syncNow: vi.fn(),
  syncEnable: vi.fn(),
  syncDisable: vi.fn(),
  onSynced: vi.fn(),
  driveSignIn: vi.fn(),
  lock: vi.fn(),
  listItems: vi.fn(),
}));

vi.mock("./sync", async (original) => ({
  ...(await original<typeof import("./sync")>()),
  syncStatus: core.syncStatus,
  syncNow: core.syncNow,
  syncEnable: core.syncEnable,
  syncDisable: core.syncDisable,
  onSynced: core.onSynced,
}));
vi.mock("./items", async (original) => ({
  ...(await original<typeof import("./items")>()),
  listItems: core.listItems,
  listDeleted: () => Promise.resolve([]),
}));
vi.mock("./vault", () => ({
  errorOf: (e: unknown) => e,
  driveSignIn: core.driveSignIn,
  lock: core.lock,
}));

import Workspace from "./Workspace.svelte";

const view = (over: Partial<SyncView> = {}): SyncView => ({
  enabled: true,
  state: "idle",
  signedIn: true,
  canSignIn: true,
  lastSyncSecsAgo: 120,
  message: null,
  fork: false,
  needsUnlock: false,
  localFolder: "/Users/test/FigLock",
  ...over,
});

let onlocked = vi.fn();
let emit: () => void = () => {};

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  vi.clearAllMocks();
  onlocked = vi.fn();
  core.listItems.mockResolvedValue([]);
  core.onSynced.mockImplementation((h: () => void) => {
    emit = h;
    return Promise.resolve(() => {});
  });
  core.syncStatus.mockResolvedValue(view());
  core.lock.mockResolvedValue(undefined);
});

async function toSettings(over?: Partial<SyncView>) {
  if (over) core.syncStatus.mockResolvedValue(view(over));
  const user = userEvent.setup();
  render(Workspace, { onlock: vi.fn(), onlocked });
  await user.click(await screen.findByRole("button", { name: "Settings" }));
  await screen.findByText("Google Drive sync");
  return user;
}

test("the sidebar says how sync is going, and when it needs the person", async () => {
  render(Workspace, { onlock: vi.fn(), onlocked });
  expect((await screen.findByTestId("sync-badge")).textContent).toBe("Synced 2 minutes ago");
});

test("with sync off there is no badge, and turning it on needs a sign-in first", async () => {
  core.syncStatus.mockResolvedValue(view({ enabled: false, state: "off", signedIn: false }));
  const user = await toSettings();
  expect(screen.queryByTestId("sync-badge")).toBeNull();
  expect(screen.getByText("Off")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Turn on sync" })).toBeNull();

  core.driveSignIn.mockResolvedValue({ canSignIn: true, signedIn: true });
  core.syncNow.mockResolvedValue(view());
  core.syncStatus.mockResolvedValue(view({ enabled: false, state: "off", signedIn: true }));
  await user.click(screen.getByRole("button", { name: "Sign in with Google" }));
  await waitFor(() => expect(core.driveSignIn).toHaveBeenCalledTimes(1));
});

test("turning sync on uploads, and a failure is shown with sync left off", async () => {
  core.syncStatus.mockResolvedValue(view({ enabled: false, state: "off" }));
  const user = await toSettings();
  core.syncEnable.mockRejectedValueOnce({
    code: "sync_failed",
    message:
      "Could not turn on sync, so nothing was changed. Your Google Drive already holds a different vault.",
  });
  await user.click(screen.getByRole("button", { name: "Turn on sync" }));
  expect(await screen.findByText(/already holds a different vault/)).toBeTruthy();
  expect(screen.getByText("Off")).toBeTruthy();

  core.syncEnable.mockResolvedValueOnce(view());
  await user.click(screen.getByRole("button", { name: "Turn on sync" }));
  expect(await screen.findByText("On")).toBeTruthy();
  expect(core.syncEnable).toHaveBeenCalledTimes(2);
});

test("sync now syncs and shows the result", async () => {
  const user = await toSettings();
  expect(screen.getByTestId("last-sync").textContent).toContain("2 minutes ago");
  core.syncNow.mockResolvedValue(view({ lastSyncSecsAgo: 1 }));
  await user.click(screen.getByRole("button", { name: /Sync now/ }));
  await waitFor(() => expect(screen.getByTestId("last-sync").textContent).toContain("just now"));
  expect(core.syncNow).toHaveBeenCalledWith(false);
});

test("offline is calm: the vault keeps working and sync retries by itself", async () => {
  await toSettings({
    state: "offline",
    message:
      "Can't reach Google Drive. The vault works here as normal and will sync when you're back online.",
  });
  expect(screen.getByText(/vault works here as normal/)).toBeTruthy();
  expect((screen.getByTestId("sync-badge") as HTMLElement).textContent).toBe(
    "Offline, will sync later",
  );
});

test("an expired Google sign-in asks to sign in again, then syncs", async () => {
  const user = await toSettings({
    state: "sign_in",
    message: "Your Google sign-in has expired. Sign in again to keep syncing.",
  });
  core.driveSignIn.mockResolvedValue({ canSignIn: true, signedIn: true });
  core.syncNow.mockResolvedValue(view());
  await user.click(screen.getByRole("button", { name: "Sign in with Google" }));
  await waitFor(() => expect(core.syncNow).toHaveBeenCalledWith(false));
  expect(core.driveSignIn).toHaveBeenCalledTimes(1);
});

test("a vault missing from Drive is only uploaded again after the person confirms", async () => {
  const user = await toSettings({
    state: "lost",
    message: "The vault is no longer in your Google Drive. You can upload it again from this Mac.",
  });
  expect(screen.getByTestId("last-sync").textContent).toContain("Sync is paused");
  await user.click(screen.getByRole("button", { name: /Upload from this Mac/ }));
  const dialog = await screen.findByRole("dialog");
  expect(core.syncNow).not.toHaveBeenCalled();

  await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(core.syncNow).not.toHaveBeenCalled();

  core.syncNow.mockResolvedValue(view());
  await user.click(screen.getByRole("button", { name: /Upload from this Mac/ }));
  await user.click(
    within(await screen.findByRole("dialog")).getByRole("button", { name: "Upload again" }),
  );
  await waitFor(() => expect(core.syncNow).toHaveBeenCalledWith(true));
});

test("a different vault in Drive pauses sync and says nothing was changed", async () => {
  await toSettings({
    state: "foreign",
    message: "Your Google Drive holds a different vault, so sync is paused. Nothing was changed.",
  });
  expect(screen.getByText(/different vault, so sync is paused/)).toBeTruthy();
  expect(screen.queryByRole("button", { name: /Upload from this Mac/ })).toBeNull();
});

test("settings changed on another device ask for a lock and unlock", async () => {
  const user = await toSettings({ needsUnlock: true });
  await user.click(screen.getByRole("button", { name: /Lock and unlock/ }));
  await waitFor(() => expect(core.lock).toHaveBeenCalledTimes(1));
  expect(onlocked).toHaveBeenCalledWith(
    "Unlock to see the latest changes from your other devices.",
  );
});

test("a settings fork is explained", async () => {
  await toSettings({ fork: true });
  expect(screen.getByText(/changed the vault's settings .* at the same time/)).toBeTruthy();
});

test("turning sync off asks first and leaves the vault alone", async () => {
  const user = await toSettings();
  await user.click(screen.getByRole("button", { name: "Turn off sync" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText(/stays on this Mac/)).toBeTruthy();
  expect(core.syncDisable).not.toHaveBeenCalled();
  core.syncDisable.mockResolvedValue(view({ enabled: false, state: "off" }));
  await user.click(within(dialog).getByRole("button", { name: "Turn off sync" }));
  await waitFor(() => expect(core.syncDisable).toHaveBeenCalledTimes(1));
  expect(await screen.findByText("Off")).toBeTruthy();
});

test("items another device added appear when the app core reports a sync", async () => {
  render(Workspace, { onlock: vi.fn(), onlocked });
  await screen.findByTestId("sync-badge");
  const before = core.listItems.mock.calls.length;
  core.listItems.mockResolvedValue([
    { id: "x", kind: "login", title: "From my phone", subtitle: "", updatedMs: Date.now() },
  ]);
  emit();
  expect(await screen.findByText("From my phone")).toBeTruthy();
  expect(core.listItems.mock.calls.length).toBeGreaterThan(before);
});
