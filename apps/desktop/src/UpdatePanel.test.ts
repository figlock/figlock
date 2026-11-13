import { render, screen, waitFor } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";

/** A stand-in for the app core's update commands. */
const core = vi.hoisted(() => ({
  check: vi.fn(),
  install: vi.fn(),
  restart: vi.fn(),
  onProgress: vi.fn(),
}));

vi.mock("./updates", () => ({
  updateCheck: core.check,
  updateInstall: core.install,
  restartApp: core.restart,
  onUpdateProgress: core.onProgress,
}));

import UpdatePanel from "./UpdatePanel.svelte";
import type { UpdateState, UpdateView } from "./updates";
import { Updates, updates } from "./updates.svelte";

const view = (
  state: UpdateState,
  version: string | null = null,
  notes: string | null = null,
): UpdateView => ({
  state,
  current: "0.2.0",
  version,
  notes,
});

beforeEach(() => {
  vi.clearAllMocks();
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  updates.view = null;
  updates.error = "";
  updates.progress = null;
  core.onProgress.mockResolvedValue(() => {});
});

test("a build that cannot update says so and offers nothing", async () => {
  updates.view = view("unavailable");
  render(UpdatePanel);
  expect(screen.getByTestId("update-unavailable")).toBeTruthy();
  expect(screen.queryByRole("button")).toBeNull();
});

test("checking by hand says when FigLock is up to date", async () => {
  core.check.mockResolvedValue(view("up_to_date"));
  const user = userEvent.setup();
  render(UpdatePanel);
  await user.click(screen.getByRole("button", { name: "Check for updates" }));
  expect((await screen.findByTestId("update-current")).textContent).toContain("0.2.0");
});

test("an available update shows its version and notes and installs only on a click", async () => {
  core.check.mockResolvedValue(view("available", "0.2.1", "Faster sync.\nA fix."));
  core.install.mockResolvedValue(view("ready", "0.2.1"));
  const user = userEvent.setup();
  render(UpdatePanel);
  await user.click(screen.getByRole("button", { name: "Check for updates" }));

  expect((await screen.findByTestId("update-available")).textContent).toContain("0.2.1");
  expect(screen.getByTestId("update-notes").textContent).toContain("Faster sync.");
  expect(core.install).not.toHaveBeenCalled();

  await user.click(screen.getByRole("button", { name: "Install update" }));
  expect(core.install).toHaveBeenCalledTimes(1);
  expect(await screen.findByText("FigLock 0.2.1 is installed")).toBeTruthy();
  expect(core.restart).not.toHaveBeenCalled();
});

test("restarting asks first, then restarts", async () => {
  updates.view = view("ready", "0.2.1");
  const user = userEvent.setup();
  render(UpdatePanel);
  await user.click(screen.getByRole("button", { name: "Restart FigLock" }));
  expect(core.restart).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Restart" }));
  expect(core.restart).toHaveBeenCalledTimes(1);
});

test("a failed install shows the reason, keeps the update on offer and can be retried", async () => {
  core.check.mockResolvedValue(view("available", "0.2.1"));
  core.install.mockRejectedValue({
    code: "update_unverified",
    message:
      "The update could not be verified, so it was not installed. Your current version is unchanged.",
  });
  const user = userEvent.setup();
  render(UpdatePanel);
  await user.click(screen.getByRole("button", { name: "Check for updates" }));
  await user.click(await screen.findByRole("button", { name: "Install update" }));
  expect(await screen.findByText(/could not be verified/)).toBeTruthy();
  expect(screen.getByRole("button", { name: "Install update" })).toBeTruthy();
});

test("after a failed install the feed is asked again, so a retry uses what is published now", async () => {
  core.check
    .mockResolvedValueOnce(view("available", "0.2.1"))
    .mockResolvedValueOnce(view("available", "0.2.2"));
  core.install.mockRejectedValueOnce({
    code: "update_unverified",
    message: "The update could not be verified, so it was not installed.",
  });
  const user = userEvent.setup();
  render(UpdatePanel);
  await user.click(screen.getByRole("button", { name: "Check for updates" }));
  await user.click(await screen.findByRole("button", { name: "Install update" }));
  expect(await screen.findByText(/could not be verified/)).toBeTruthy();
  expect(core.check).toHaveBeenCalledTimes(2);
  expect((await screen.findByTestId("update-available")).textContent).toContain("0.2.2");
});

test("download progress is shown while installing", async () => {
  core.check.mockResolvedValue(view("available", "0.2.1"));
  let finish: (v: unknown) => void = () => {};
  core.install.mockReturnValue(new Promise((r) => (finish = r)));
  let report: (p: { downloaded: number; total: number | null }) => void = () => {};
  core.onProgress.mockImplementation((h) => {
    report = h;
    return Promise.resolve(() => {});
  });
  const user = userEvent.setup();
  render(UpdatePanel);
  await user.click(screen.getByRole("button", { name: "Check for updates" }));
  await user.click(await screen.findByRole("button", { name: "Install update" }));
  await waitFor(() => expect(core.onProgress).toHaveBeenCalled());
  report({ downloaded: 50, total: 200 });
  expect((await screen.findByTestId("update-progress")).textContent).toContain("25% downloaded");
  finish(view("ready", "0.2.1"));
  expect(await screen.findByText("FigLock 0.2.1 is installed")).toBeTruthy();
});

test("the check at launch is quiet when it fails and when there is nothing new", async () => {
  const quiet = new Updates();
  core.check.mockRejectedValue({
    code: "update_check_failed",
    message: "FigLock could not check.",
  });
  await quiet.check(true);
  expect(quiet.error).toBe("");
  expect(quiet.pending).toBe(false);

  core.check.mockResolvedValue(view("up_to_date"));
  await quiet.check(true);
  expect(quiet.pending).toBe(false);

  core.check.mockResolvedValue(view("available", "0.2.1"));
  await quiet.check(true);
  expect(quiet.pending).toBe(true);
});

test("a check by hand that fails says so", async () => {
  core.check.mockRejectedValue({
    code: "update_check_failed",
    message: "FigLock could not check for updates. Check your internet connection and try again.",
  });
  const user = userEvent.setup();
  render(UpdatePanel);
  await user.click(screen.getByRole("button", { name: "Check for updates" }));
  expect(await screen.findByText(/could not check for updates/)).toBeTruthy();
});
