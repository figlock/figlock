import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";

/** A stand-in for the app core's command line tool commands. */
const core = vi.hoisted(() => ({
  status: vi.fn(),
  install: vi.fn(),
  remove: vi.fn(),
}));

vi.mock("./cliTool", () => ({
  cliToolStatus: core.status,
  installCliTool: core.install,
  removeCliTool: core.remove,
}));

import CommandLinePanel from "./CommandLinePanel.svelte";

const view = (state: string) => ({ state, link: "~/.local/bin/figlock", dir: "~/.local/bin" });

beforeEach(() => vi.clearAllMocks());

test("a build without the tool says so and offers nothing to install", async () => {
  core.status.mockResolvedValue(view("unavailable"));
  render(CommandLinePanel);
  expect(await screen.findByTestId("cli-unavailable")).toBeTruthy();
  expect(screen.queryByRole("button")).toBeNull();
});

test("nothing is installed until the person asks, and the copy says PATH is left alone", async () => {
  core.status.mockResolvedValue(view("not_installed"));
  core.install.mockResolvedValue(view("installed"));
  const user = userEvent.setup();
  render(CommandLinePanel);

  expect(await screen.findByText(/It does not change your PATH or shell settings/)).toBeTruthy();
  expect(core.install).not.toHaveBeenCalled();

  await user.click(screen.getByRole("button", { name: "Install command line tool" }));
  expect(core.install).toHaveBeenCalledTimes(1);
  expect((await screen.findByTestId("cli-where")).textContent).toContain("~/.local/bin/figlock");
  expect(screen.getByText(/FigLock never edits it for you/)).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Install command line tool" })).toBeNull();
});

test("an installed link can be removed again", async () => {
  core.status.mockResolvedValue(view("installed"));
  core.remove.mockResolvedValue(view("not_installed"));
  const user = userEvent.setup();
  render(CommandLinePanel);
  await user.click(await screen.findByRole("button", { name: "Remove the link" }));
  expect(core.remove).toHaveBeenCalledTimes(1);
  expect(await screen.findByRole("button", { name: "Install command line tool" })).toBeTruthy();
});

test("a link to a moved app is repaired by installing again", async () => {
  core.status.mockResolvedValue(view("moved"));
  core.install.mockResolvedValue(view("installed"));
  const user = userEvent.setup();
  render(CommandLinePanel);
  expect(await screen.findByText("Needs repair")).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "Repair link" }));
  expect(await screen.findByText("Installed")).toBeTruthy();
});

test("something else at the link's path is called out and never offered for replacement", async () => {
  core.status.mockResolvedValue(view("conflict"));
  render(CommandLinePanel);
  expect(await screen.findByRole("alert")).toBeTruthy();
  expect(screen.getByText(/left it alone/)).toBeTruthy();
  expect(screen.queryByRole("button")).toBeNull();
});

test("a failed install shows the reason and leaves the install button available", async () => {
  core.status.mockResolvedValue(view("not_installed"));
  core.install.mockRejectedValue({
    code: "failed",
    message: "The command line tool could not be changed: denied.",
  });
  const user = userEvent.setup();
  render(CommandLinePanel);
  await user.click(await screen.findByRole("button", { name: "Install command line tool" }));
  expect(await screen.findByText(/could not be changed: denied/)).toBeTruthy();
  expect(screen.getByRole("button", { name: "Install command line tool" })).toBeTruthy();
});
