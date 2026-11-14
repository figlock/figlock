import { render, screen, waitFor, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";

import type { HealthReport } from "./health";
import type { ItemSummary } from "./items";
import type { SyncView } from "./sync";

/** A stand-in for the app core's health command and item list. */
const core = vi.hoisted(() => ({
  healthReport: vi.fn(),
  syncStatus: vi.fn(),
  onSynced: vi.fn(),
  listItems: vi.fn(),
  getItem: vi.fn(),
}));

vi.mock("./health", () => ({ healthReport: core.healthReport }));
vi.mock("./sync", async (original) => ({
  ...(await original<typeof import("./sync")>()),
  syncStatus: core.syncStatus,
  onSynced: core.onSynced,
}));
vi.mock("./items", async (original) => ({
  ...(await original<typeof import("./items")>()),
  listItems: core.listItems,
  listDeleted: () => Promise.resolve([]),
  itemDetail: core.getItem,
}));
vi.mock("./vault", () => ({
  errorOf: (e: unknown) => e,
  lock: vi.fn(),
  vaultState: vi.fn(),
}));

import Workspace from "./Workspace.svelte";

const row = (id: string, title: string): ItemSummary => ({
  id,
  kind: "login",
  title,
  subtitle: "",
  updatedMs: 0,
});

const local: HealthReport = {
  checked: 4,
  rows: [
    { id: "a1", weakBits: 30, reusedWith: 0, oldDays: null, breached: null },
    { id: "b2", weakBits: null, reusedWith: 1, oldDays: 800, breached: null },
  ],
  breachChecked: false,
  breachFailed: false,
};

beforeEach(() => {
  vi.clearAllMocks();
  core.listItems.mockResolvedValue([row("a1", "Forum"), row("b2", "Bank")]);
  core.onSynced.mockResolvedValue(() => {});
  core.syncStatus.mockResolvedValue({
    enabled: false,
    state: "off",
    signedIn: false,
    signedInSecsAgo: null,
    canSignIn: true,
    lastSyncSecsAgo: null,
    message: null,
    fork: false,
    needsUnlock: false,
    localFolder: null,
  } satisfies SyncView);
  core.healthReport.mockResolvedValue(local);
});

async function toHealth() {
  const user = userEvent.setup();
  render(Workspace, { onlock: vi.fn(), onlocked: vi.fn() });
  await user.click(await screen.findByRole("button", { name: "Password health" }));
  await screen.findByRole("list", { name: "Items that need attention" });
  return user;
}

test("opening the page runs only the local check, and lists what is wrong", async () => {
  await toHealth();
  expect(core.healthReport).toHaveBeenCalledTimes(1);
  expect(core.healthReport).toHaveBeenCalledWith(false);
  const list = screen.getByRole("list", { name: "Items that need attention" });
  expect(within(list).getByText("Forum")).toBeTruthy();
  expect(within(list).getByText("Weak")).toBeTruthy();
  expect(within(list).getByText("Reused with 1 other item")).toBeTruthy();
  expect(within(list).getByText("Unchanged for 2 years")).toBeTruthy();
});

test("the breach check only runs when asked, and says what it sends", async () => {
  const user = await toHealth();
  expect(screen.getByText(/first 5 characters/)).toBeTruthy();
  core.healthReport.mockResolvedValue({
    ...local,
    rows: [
      { id: "a1", weakBits: null, reusedWith: 0, oldDays: null, breached: 52579 },
      ...local.rows.slice(1),
    ],
    breachChecked: true,
  });
  await user.click(screen.getByRole("button", { name: "Check for breaches" }));
  await waitFor(() => expect(core.healthReport).toHaveBeenLastCalledWith(true));
  expect(await screen.findByText("In 52579 known breaches")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Check again" })).toBeTruthy();
});

test("an unreachable breach service is said so, and the local results stay", async () => {
  const user = await toHealth();
  core.healthReport.mockResolvedValue({ ...local, breachFailed: true });
  await user.click(screen.getByRole("button", { name: "Check for breaches" }));
  expect(await screen.findByText(/Could not reach the breach service/)).toBeTruthy();
  expect(screen.getByText("Forum")).toBeTruthy();
});

test("a clean vault says so", async () => {
  core.healthReport.mockResolvedValue({ ...local, rows: [] });
  const user = userEvent.setup();
  render(Workspace, { onlock: vi.fn(), onlocked: vi.fn() });
  await user.click(await screen.findByRole("button", { name: "Password health" }));
  expect(await screen.findByText("No problems found")).toBeTruthy();
});
