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
  syncSignOut: vi.fn(),
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
  syncSignOut: core.syncSignOut,
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
  signedInSecsAgo: 3 * 86_400,
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

test("with sync off there is no badge, and one click turns it on from a signed-out Mac", async () => {
  core.syncStatus.mockResolvedValue(view({ enabled: false, state: "off", signedIn: false }));
  const user = await toSettings();
  expect(screen.queryByTestId("sync-badge")).toBeNull();
  expect(screen.getByText("Off")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Sign in with Google" })).toBeNull();

  const order: string[] = [];
  core.driveSignIn.mockImplementation(async () => {
    order.push("sign-in");
    core.syncStatus.mockResolvedValue(view({ enabled: false, state: "off", signedIn: true }));
    return { canSignIn: true, signedIn: true };
  });
  core.syncEnable.mockImplementation(async () => {
    order.push("enable");
    core.syncStatus.mockResolvedValue(view({ lastSyncSecsAgo: 0 }));
    return view({ lastSyncSecsAgo: 0 });
  });
  await user.click(screen.getByRole("button", { name: "Turn on sync" }));
  expect(await screen.findByRole("button", { name: "Turn off sync" })).toBeTruthy();
  expect(order).toEqual(["sign-in", "enable"]);
  expect(screen.getByTestId("last-sync").textContent).toContain("Last synced just now");
  // Other panels may complain about this file's partial mocks; sync itself must not.
  const alerts = screen.queryAllByRole("alert").map((a) => a.textContent ?? "");
  expect(alerts.filter((t) => /google|sync/i.test(t))).toEqual([]);
});

test("progress reads Waiting for Google, then Turning on", async () => {
  core.syncStatus.mockResolvedValue(view({ enabled: false, state: "off", signedIn: false }));
  const user = await toSettings();
  let finishSignIn: () => void = () => {};
  core.driveSignIn.mockReturnValue(new Promise<void>((r) => (finishSignIn = r)));
  let finishEnable: (v: SyncView) => void = () => {};
  core.syncEnable.mockReturnValue(new Promise<SyncView>((r) => (finishEnable = r)));

  await user.click(screen.getByRole("button", { name: "Turn on sync" }));
  expect(await screen.findByRole("button", { name: "Waiting for Google..." })).toBeTruthy();
  core.syncStatus.mockResolvedValue(view({ enabled: false, state: "off", signedIn: true }));
  finishSignIn();
  expect(await screen.findByRole("button", { name: "Turning on..." })).toBeTruthy();
  finishEnable(view({ lastSyncSecsAgo: 0 }));
  expect(await screen.findByRole("button", { name: "Turn off sync" })).toBeTruthy();
});

test("a sign-in that works but a sync that does not says which step failed and keeps the sign-in", async () => {
  core.syncStatus.mockResolvedValue(view({ enabled: false, state: "off", signedIn: false }));
  const user = await toSettings();
  core.driveSignIn.mockImplementation(async () => {
    core.syncStatus.mockResolvedValue(view({ enabled: false, state: "off", signedIn: true }));
    return { canSignIn: true, signedIn: true };
  });
  core.syncEnable.mockRejectedValueOnce({
    code: "sync_failed",
    message:
      "Could not turn on sync, so nothing was changed. Your Google Drive already holds a different vault.",
  });
  await user.click(screen.getByRole("button", { name: "Turn on sync" }));
  expect(
    await screen.findByText(/You are signed in to Google, but sync is not on yet/),
  ).toBeTruthy();
  expect(screen.getByText(/already holds a different vault/)).toBeTruthy();
  expect(screen.getByText("Off")).toBeTruthy();

  // The next click only retries the second step: no second Google sign-in.
  core.syncEnable.mockResolvedValue(view({ lastSyncSecsAgo: 0 }));
  await user.click(screen.getByRole("button", { name: "Turn on sync" }));
  expect(await screen.findByRole("button", { name: "Turn off sync" })).toBeTruthy();
  expect(core.driveSignIn).toHaveBeenCalledTimes(1);
});

test("a cancelled or failed Google sign-in is shown and sync stays off", async () => {
  core.syncStatus.mockResolvedValue(view({ enabled: false, state: "off", signedIn: false }));
  const user = await toSettings();
  core.driveSignIn.mockRejectedValue({ code: "drive_sign_in", message: "Sign-in timed out." });
  await user.click(screen.getByRole("button", { name: "Turn on sync" }));
  expect(await screen.findByText("Sign-in timed out.")).toBeTruthy();
  expect(core.syncEnable).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Turn on sync" })).toBeTruthy();
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

test("Settings says this Mac is signed in to Google and since when", async () => {
  core.syncStatus.mockResolvedValue(view());
  await toSettings();
  expect(screen.getByText("Signed in to Google")).toBeTruthy();
  expect(screen.getByTestId("google-signin").textContent).toContain("Signed in 3 days ago.");
});

test("signing out of Google asks first, then signs out and turns sync off", async () => {
  core.syncStatus.mockResolvedValue(view());
  const user = await toSettings();
  await user.click(screen.getByRole("button", { name: "Sign out of Google" }));
  const dialog = await screen.findByRole("dialog");
  expect(dialog.textContent).toContain("stays where it is");
  // Cancelling signs nobody out.
  await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(core.syncSignOut).not.toHaveBeenCalled();

  core.syncSignOut.mockImplementation(async () => {
    core.syncStatus.mockResolvedValue(view({ enabled: false, state: "off", signedIn: false }));
    return { revoked: true };
  });
  await user.click(screen.getByRole("button", { name: "Sign out of Google" }));
  await user.click(
    within(await screen.findByRole("dialog")).getByRole("button", { name: "Sign out of Google" }),
  );
  await waitFor(() => expect(core.syncSignOut).toHaveBeenCalledTimes(1));
  expect(await screen.findByRole("button", { name: "Turn on sync" })).toBeTruthy();
  expect(screen.queryByText("Signed in to Google")).toBeNull();
});

test("when Google cannot be reached the person is told where to remove the access", async () => {
  core.syncStatus.mockResolvedValue(view());
  const user = await toSettings();
  core.syncSignOut.mockImplementation(async () => {
    core.syncStatus.mockResolvedValue(view({ enabled: false, state: "off", signedIn: false }));
    return { revoked: false };
  });
  await user.click(screen.getByRole("button", { name: "Sign out of Google" }));
  await user.click(
    within(await screen.findByRole("dialog")).getByRole("button", { name: "Sign out of Google" }),
  );
  expect(await screen.findByText(/could not be reached to remove FigLock's access/)).toBeTruthy();
});

test("using a different Google account signs out, then runs Turn on sync against the new sign-in", async () => {
  core.syncStatus.mockResolvedValue(view());
  const user = await toSettings();
  const order: string[] = [];
  core.syncSignOut.mockImplementation(async () => {
    order.push("sign-out");
    core.syncStatus.mockResolvedValue(view({ enabled: false, state: "off", signedIn: false }));
    return { revoked: true };
  });
  core.driveSignIn.mockImplementation(async () => {
    order.push("sign-in");
    core.syncStatus.mockResolvedValue(
      view({ enabled: false, state: "off", signedIn: true, signedInSecsAgo: 0 }),
    );
    return { canSignIn: true, signedIn: true };
  });
  core.syncEnable.mockImplementation(async () => {
    order.push("enable");
    core.syncStatus.mockResolvedValue(view({ signedInSecsAgo: 0, lastSyncSecsAgo: 0 }));
    return view({ signedInSecsAgo: 0, lastSyncSecsAgo: 0 });
  });
  await user.click(screen.getByRole("button", { name: "Use a different Google account" }));
  const dialog = await screen.findByRole("dialog");
  expect(dialog.textContent).toContain("different FigLock vault");
  await user.click(within(dialog).getByRole("button", { name: "Sign out and choose another" }));
  await waitFor(() => expect(order).toEqual(["sign-out", "sign-in", "enable"]));
  expect(await screen.findByRole("button", { name: "Turn off sync" })).toBeTruthy();
});

test("a sign-out that fails is shown and no new sign-in starts", async () => {
  core.syncStatus.mockResolvedValue(view());
  const user = await toSettings();
  core.syncSignOut.mockRejectedValue({ code: "failed", message: "Could not sign out." });
  await user.click(screen.getByRole("button", { name: "Use a different Google account" }));
  await user.click(
    within(await screen.findByRole("dialog")).getByRole("button", {
      name: "Sign out and choose another",
    }),
  );
  expect(await screen.findByText("Could not sign out.")).toBeTruthy();
  expect(core.driveSignIn).not.toHaveBeenCalled();
});
