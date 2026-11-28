import { afterEach, expect, test } from "vitest";

import { applyAppearance, savedAppearance, setAppearance } from "./appearance";

afterEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});

test("starts on System, with no theme forced", () => {
  expect(savedAppearance()).toBe("system");
  applyAppearance(savedAppearance());
  expect(document.documentElement.hasAttribute("data-theme")).toBe(false);
});

test("Light and Dark set the theme and are remembered", () => {
  setAppearance("dark");
  expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  expect(savedAppearance()).toBe("dark");
  setAppearance("light");
  expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  expect(savedAppearance()).toBe("light");
});

test("going back to System removes the forced theme", () => {
  setAppearance("dark");
  setAppearance("system");
  expect(document.documentElement.hasAttribute("data-theme")).toBe(false);
  expect(savedAppearance()).toBe("system");
});

test("an unknown saved value is ignored", () => {
  localStorage.setItem("figlock.appearance", "neon");
  expect(savedAppearance()).toBe("system");
});
