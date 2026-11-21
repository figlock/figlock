import { render, screen, waitFor } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";

const core = vi.hoisted(() => ({ brokerStatus: vi.fn(), brokerTakeOver: vi.fn() }));
vi.mock("./vault", () => ({
  errorOf: (e: unknown) => e,
  brokerStatus: core.brokerStatus,
  brokerTakeOver: core.brokerTakeOver,
}));

import BrokerBanner from "./BrokerBanner.svelte";

beforeEach(() => vi.resetAllMocks());

test("says nothing when this window answers agents or nobody does", async () => {
  core.brokerStatus.mockResolvedValue("hosting");
  render(BrokerBanner);
  await waitFor(() => expect(core.brokerStatus).toHaveBeenCalled());
  expect(screen.queryByTestId("broker-banner")).toBeNull();
});

test("offers to take over from a broker started in a terminal, and goes away once it has", async () => {
  core.brokerStatus.mockResolvedValue("cli");
  core.brokerTakeOver.mockResolvedValue("hosting");
  render(BrokerBanner);
  const button = await screen.findByRole("button", { name: "Use FigLock for agents" });
  expect(screen.getByTestId("broker-banner").textContent).toMatch(/Approvals already given/);
  await userEvent.click(button);
  expect(core.brokerTakeOver).toHaveBeenCalledTimes(1);
  await waitFor(() => expect(screen.queryByTestId("broker-banner")).toBeNull());
});

test("tells the person when the terminal broker will not stop, and keeps the button", async () => {
  core.brokerStatus.mockResolvedValue("cli");
  core.brokerTakeOver.mockRejectedValue({
    code: "broker_busy",
    message: "The broker running in a terminal did not stop. Stop it there and try again.",
  });
  render(BrokerBanner);
  await userEvent.click(await screen.findByRole("button", { name: "Use FigLock for agents" }));
  expect(await screen.findByText(/did not stop/)).toBeTruthy();
  expect(screen.getByRole("button", { name: "Use FigLock for agents" })).toBeTruthy();
});
