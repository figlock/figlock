import { render, screen } from "@testing-library/svelte";
import { expect, test, vi } from "vitest";

vi.mock("./vault", () => ({
  vaultState: () => Promise.resolve({ initialised: true, version: "9.9.9" }),
}));

import App from "./App.svelte";

test("shows the locked state and version reported by the core", async () => {
  render(App);
  expect(await screen.findByText("Your vault is locked.")).toBeTruthy();
  expect(screen.getByText("Version 9.9.9")).toBeTruthy();
});
