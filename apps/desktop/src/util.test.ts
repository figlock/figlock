import { expect, test } from "vitest";

import { ageText, dayDiff, expiryState, fmtSize, newSuffix, suffixed } from "./util";

test("sizes read in B, KB and MB", () => {
  expect(fmtSize(900)).toBe("900 B");
  expect(fmtSize(652_288)).toBe("637 KB");
  expect(fmtSize(1_572_864)).toBe("1.5 MB");
});

test("ages are in plain words", () => {
  const now = Date.UTC(2026, 10, 11);
  const ago = (days: number) => now - days * 86_400_000;
  expect(ageText(ago(0), now)).toBe("today");
  expect(ageText(ago(1), now)).toBe("yesterday");
  expect(ageText(ago(41), now)).toBe("41 days ago");
  expect(ageText(ago(200), now)).toBe("7 months ago");
  expect(ageText(ago(800), now)).toBe("2 years ago");
});

test("an expiry within 30 days is soon and a past one is expired", () => {
  const now = new Date(2026, 10, 11, 15);
  expect(dayDiff("2026-11-11", now)).toBe(0);
  expect(expiryState("2026-11-10", now)).toBe("expired");
  expect(expiryState("2026-12-11", now)).toBe("soon");
  expect(expiryState("2026-12-12", now)).toBe("");
  expect(expiryState("", now)).toBe("");
});

test("suffixed fields come out in id order, and new suffixes fit the schema", () => {
  expect(suffixed({ "url/b": "2", "url/a": "1", title: "t" }, "url/")).toEqual([
    ["a", "1"],
    ["b", "2"],
  ]);
  for (let i = 0; i < 50; i++) expect(newSuffix()).toMatch(/^[a-z0-9]{6}$/);
});
