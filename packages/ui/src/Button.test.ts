import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { createRawSnippet } from "svelte";
import { expect, test, vi } from "vitest";

import Button from "./Button.svelte";

const label = createRawSnippet(() => ({ render: () => "<span>Unlock</span>" }));

test("a button runs its handler on click and is not clickable when disabled", async () => {
  const onclick = vi.fn();
  const user = userEvent.setup();
  const { rerender } = render(Button, { onclick, children: label });

  await user.click(screen.getByRole("button", { name: "Unlock" }));
  expect(onclick).toHaveBeenCalledOnce();

  await rerender({ disabled: true });
  await user.click(screen.getByRole("button", { name: "Unlock" }));
  expect(onclick).toHaveBeenCalledOnce();
});
