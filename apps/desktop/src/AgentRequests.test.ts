import { render, screen, waitFor } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";

import type { AgentCard } from "./agent";

const core = vi.hoisted(() => ({
  agentRequests: vi.fn(),
  agentDecide: vi.fn(),
  request: null as null | ((c: unknown) => void),
  done: null as null | ((id: number) => void),
}));
vi.mock("./agent", () => ({
  agentRequests: core.agentRequests,
  agentDecide: core.agentDecide,
  onAgentRequest: (h: (c: unknown) => void) => {
    core.request = h;
    return Promise.resolve(() => {});
  },
  onAgentDone: (h: (id: number) => void) => {
    core.done = h;
    return Promise.resolve(() => {});
  },
}));

import AgentRequests from "./AgentRequests.svelte";

const card: AgentCard = {
  id: 7,
  program: "figlock",
  programPath: "/usr/local/bin/figlock",
  programVerified: true,
  startedBy: ["zsh", "claude"],
  says: "figlock http",
  reason: "list open issues",
  wants: 'the token "GitHub" (only for api.github.com)',
  how: "http: figlock makes the request and adds the secret; the program never sees it",
  duration: "15 minutes",
  loud: [],
  seconds: 60,
};

beforeEach(() => {
  vi.resetAllMocks();
  core.agentRequests.mockResolvedValue([]);
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
});

test("shows nothing until a request comes in, then says what was verified and what was only claimed", async () => {
  render(AgentRequests);
  await waitFor(() => expect(core.request).not.toBeNull());
  expect(screen.queryByText(/wants to use a secret/)).toBeNull();
  core.request!(card);
  expect(await screen.findByText("figlock wants to use a secret")).toBeTruthy();
  expect(screen.getByText(/zsh, then claude/)).toBeTruthy();
  expect(screen.getByText(/checked by your Mac/)).toBeTruthy();
  // What the program said about itself is marked as such.
  expect(screen.getAllByText("not checked")).toHaveLength(2);
  expect(screen.getByText(/No answer in 60 seconds counts as no/)).toBeTruthy();
});

test("a card that was waiting before the window opened is shown", async () => {
  core.agentRequests.mockResolvedValue([card]);
  render(AgentRequests);
  expect(await screen.findByText("figlock wants to use a secret")).toBeTruthy();
});

test("Approve asks for Touch ID and disables both buttons while it is open", async () => {
  let finish: (v: string) => void = () => {};
  core.agentDecide.mockReturnValue(new Promise((r) => (finish = r)));
  core.agentRequests.mockResolvedValue([card]);
  render(AgentRequests);
  await userEvent.click(await screen.findByRole("button", { name: "Approve with Touch ID" }));
  expect(core.agentDecide).toHaveBeenCalledWith(7, true);
  expect(screen.getByRole("button", { name: "Waiting for Touch ID..." })).toHaveProperty(
    "disabled",
    true,
  );
  expect(screen.getByRole("button", { name: "Not now" })).toHaveProperty("disabled", true);
  finish("approved");
  await waitFor(() => expect(screen.queryByText(/wants to use a secret/)).toBeNull());
});

test("Not now answers no without asking for Touch ID", async () => {
  core.agentDecide.mockResolvedValue("not_approved");
  core.agentRequests.mockResolvedValue([card]);
  render(AgentRequests);
  await userEvent.click(await screen.findByRole("button", { name: "Not now" }));
  expect(core.agentDecide).toHaveBeenCalledWith(7, false);
  await waitFor(() => expect(screen.queryByText(/wants to use a secret/)).toBeNull());
});

test("a request that timed out is taken off the screen, and the next one is shown", async () => {
  core.agentRequests.mockResolvedValue([card, { ...card, id: 8, program: "other" }]);
  render(AgentRequests);
  await screen.findByText("figlock wants to use a secret");
  core.done!(7);
  expect(await screen.findByText("other wants to use a secret")).toBeTruthy();
});

test("a dangerous request carries its loud warning", async () => {
  core.agentRequests.mockResolvedValue([
    { ...card, loud: ["THE PROGRAM WILL BE GIVEN THE SECRET VALUE ITSELF"] },
  ]);
  render(AgentRequests);
  expect(await screen.findByText(/GIVEN THE SECRET VALUE ITSELF/)).toBeTruthy();
});

test("a program path the system could not give is not called checked", async () => {
  core.agentRequests.mockResolvedValue([
    { ...card, programPath: "an unknown program", programVerified: false },
  ]);
  render(AgentRequests);
  expect(await screen.findByText(/could not be checked/)).toBeTruthy();
  expect(screen.queryByText(/checked by your Mac/)).toBeNull();
});
