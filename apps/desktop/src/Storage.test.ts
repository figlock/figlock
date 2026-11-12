import { render, screen, waitFor } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";

/** A stand-in for the app core's storage and Drive commands. */
const core = vi.hoisted(() => ({
  driveStatus: vi.fn(),
  driveSignIn: vi.fn(),
  driveSignOut: vi.fn(),
  createVault: vi.fn(),
  openExisting: vi.fn(),
  joinStart: vi.fn(),
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
  chooseFolder: () => Promise.resolve(null),
  cancelCreate: () => Promise.resolve(),
  errorOf: (e: unknown) => e,
  ...core,
}));

import App from "./App.svelte";

const KEY = "FL1-ABCD-EFGH-JKMN-PQRS-TVWX-YZ23-4567-89AB";
const syncBox = () => screen.getByRole("checkbox", { name: /Sync with Google Drive/ });

beforeEach(() => {
  vi.clearAllMocks();
  core.driveStatus.mockResolvedValue({ canSignIn: true, signedIn: false });
  core.createVault.mockResolvedValue({ secretKey: KEY });
});

async function toCreate() {
  const user = userEvent.setup();
  render(App);
  await user.click(await screen.findByRole("button", { name: "Create a vault" }));
  await screen.findByRole("checkbox", { name: /Sync with Google Drive/ });
  return user;
}

async function fillPasswords(user: ReturnType<typeof userEvent.setup>) {
  const pw = "correct horse battery 42";
  await user.type(screen.getByLabelText("Master password"), pw);
  await user.type(screen.getByLabelText("Repeat master password"), pw);
}

test("a folder is the default and is sent without Drive", async () => {
  const user = await toCreate();
  expect((syncBox() as HTMLInputElement).checked).toBe(false);
  await fillPasswords(user);
  await user.click(screen.getByRole("button", { name: "Continue" }));
  await waitFor(() => expect(core.createVault).toHaveBeenCalledTimes(1));
  expect(core.createVault.mock.calls[0][0]).toEqual({
    path: "/Users/test/FigLock",
    drive: false,
  });
});

test("syncing with Google Drive needs a sign-in first, and keeps the local folder", async () => {
  const user = await toCreate();
  await user.click(syncBox());
  expect(await screen.findByRole("button", { name: "Sign in with Google" })).toBeTruthy();
  const next = screen.getByRole("button", { name: "Continue" }) as HTMLButtonElement;
  expect(next.disabled).toBe(true);

  core.driveSignIn.mockResolvedValue({ canSignIn: true, signedIn: true });
  await user.click(screen.getByRole("button", { name: "Sign in with Google" }));
  expect(await screen.findByText("Signed in to Google Drive.")).toBeTruthy();
  expect(core.driveSignIn).toHaveBeenCalledTimes(1);
  expect(next.disabled).toBe(false);

  await fillPasswords(user);
  await user.click(next);
  await waitFor(() => expect(core.createVault).toHaveBeenCalledTimes(1));
  expect(core.createVault.mock.calls[0][0]).toEqual({
    path: "/Users/test/FigLock",
    drive: true,
  });
});

test("while the browser sign-in is open the button says so and cannot be pressed twice", async () => {
  const user = await toCreate();
  await user.click(syncBox());
  let finish: (v: unknown) => void = () => {};
  core.driveSignIn.mockReturnValue(new Promise((r) => (finish = r)));
  await user.click(await screen.findByRole("button", { name: "Sign in with Google" }));
  const waiting = (await screen.findByRole("button", {
    name: "Waiting for Google...",
  })) as HTMLButtonElement;
  expect(waiting.disabled).toBe(true);
  expect(screen.getByText("Finish signing in in your browser.")).toBeTruthy();
  finish({ canSignIn: true, signedIn: true });
  expect(await screen.findByText("Signed in to Google Drive.")).toBeTruthy();
});

test("a failed sign-in shows why and leaves the person able to try again", async () => {
  const user = await toCreate();
  await user.click(syncBox());
  core.driveSignIn.mockRejectedValue({
    code: "drive_sign_in",
    message: "The sign-in was not finished in time.",
  });
  await user.click(await screen.findByRole("button", { name: "Sign in with Google" }));
  expect(await screen.findByText("The sign-in was not finished in time.")).toBeTruthy();
  expect(
    (screen.getByRole("button", { name: "Sign in with Google" }) as HTMLButtonElement).disabled,
  ).toBe(false);
});

test("a build without a Google client says so instead of offering a sign-in", async () => {
  core.driveStatus.mockResolvedValue({ canSignIn: false, signedIn: false });
  const user = await toCreate();
  await user.click(syncBox());
  expect(await screen.findByText(/cannot sign in to Google/)).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Sign in with Google" })).toBeNull();
  expect((screen.getByRole("button", { name: "Continue" }) as HTMLButtonElement).disabled).toBe(
    true,
  );
});

test("signing out says when Google could not confirm the access is gone", async () => {
  core.driveStatus.mockResolvedValueOnce({ canSignIn: true, signedIn: true });
  const user = await toCreate();
  await user.click(syncBox());
  expect(await screen.findByText("Signed in to Google Drive.")).toBeTruthy();
  core.driveSignOut.mockResolvedValue({ revoked: false });
  core.driveStatus.mockResolvedValue({ canSignIn: true, signedIn: false });
  await user.click(screen.getByRole("button", { name: "Sign out" }));
  expect(await screen.findByText(/Google could not be reached/)).toBeTruthy();
  expect(await screen.findByRole("button", { name: "Sign in with Google" })).toBeTruthy();
});

test("opening an existing vault can use Google Drive too", async () => {
  core.driveStatus.mockResolvedValue({ canSignIn: true, signedIn: true });
  core.openExisting.mockResolvedValue({ phase: "unlocked" });
  const user = userEvent.setup();
  render(App);
  await user.click(await screen.findByRole("button", { name: "Open an existing vault" }));
  await user.click(await screen.findByRole("radio", { name: /Google Drive/ }));
  await user.type(screen.getByLabelText("Master password"), "correct horse battery 42");
  await user.type(screen.getAllByLabelText(/Secret Key group 1/)[0], KEY);
  await user.click(screen.getByRole("button", { name: "Open vault" }));
  await waitFor(() => expect(core.openExisting).toHaveBeenCalledTimes(1));
  expect(core.openExisting.mock.calls[0][0]).toEqual({ drive: true });
});
