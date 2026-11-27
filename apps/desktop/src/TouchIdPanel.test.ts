import { render, screen, waitFor } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";

const core = vi.hoisted(() => ({
  vaultState: vi.fn(),
  turnTouchIdOff: vi.fn(),
  turnTouchIdOn: vi.fn(),
}));

vi.mock("./vault", () => ({
  errorOf: (e: unknown) => e,
  vaultState: core.vaultState,
  turnTouchIdOff: core.turnTouchIdOff,
  turnTouchIdOn: core.turnTouchIdOn,
}));
vi.mock("./workspace", () => ({
  getWorkspace: () => ({}),
  failure: (_w: unknown, e: { message: string }) => e.message,
}));

import TouchIdPanel from "./TouchIdPanel.svelte";

const on = { quickUnlock: true, touchAvailable: true, secretKeyKept: true, secretKeyHeld: true };

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  vi.clearAllMocks();
});

test("nothing shows on a Mac without Touch ID", async () => {
  core.vaultState.mockResolvedValue({ ...on, touchAvailable: false });
  render(TouchIdPanel);
  await waitFor(() => expect(core.vaultState).toHaveBeenCalled());
  expect(screen.queryByText("Touch ID")).toBeNull();
});

test("turning it off asks first, then sends the command and shows Off", async () => {
  const user = userEvent.setup();
  core.vaultState.mockResolvedValue(on);
  core.turnTouchIdOff.mockResolvedValue({ ...on, quickUnlock: false });
  render(TouchIdPanel);
  await user.click(await screen.findByRole("button", { name: "Turn off Touch ID" }));
  expect(core.turnTouchIdOff).not.toHaveBeenCalled();
  const buttons = screen.getAllByRole("button", { name: "Turn off Touch ID" });
  await user.click(buttons[buttons.length - 1]);
  expect(await screen.findByRole("button", { name: "Turn on Touch ID" })).toBeTruthy();
  expect(core.turnTouchIdOff).toHaveBeenCalledTimes(1);
});

test("turning it on asks for the password only when the key is on hand", async () => {
  const user = userEvent.setup();
  core.vaultState.mockResolvedValue({ ...on, quickUnlock: false });
  core.turnTouchIdOn.mockResolvedValue(on);
  render(TouchIdPanel);
  await user.click(await screen.findByRole("button", { name: "Turn on Touch ID" }));
  expect(screen.queryByLabelText(/Secret Key group 1/)).toBeNull();
  await user.type(screen.getByLabelText("Password"), "pw");
  await user.click(screen.getByRole("button", { name: "Turn on" }));
  await waitFor(() => expect(core.turnTouchIdOn).toHaveBeenCalledWith("pw", ""));
  expect(await screen.findByRole("button", { name: "Turn off Touch ID" })).toBeTruthy();
});

test("a Mac with no key on hand asks for it, and a refusal is said in words", async () => {
  const user = userEvent.setup();
  core.vaultState.mockResolvedValue({
    ...on,
    quickUnlock: false,
    secretKeyKept: false,
    secretKeyHeld: false,
  });
  render(TouchIdPanel);
  await user.click(await screen.findByRole("button", { name: "Turn on Touch ID" }));
  expect(screen.getByLabelText("Secret Key group 1 of 8")).toBeTruthy();
});
