#!/usr/bin/env python3
"""Generate the SwiftUI theme constants from design/tokens.css (FIG-158).

    python3 design/gen-swift-theme.py            write apps/ios/Shared/Sources/Shared/Theme/Tokens.generated.swift
    python3 design/gen-swift-theme.py --check    exit 1 when that file is not what tokens.css produces

The token names are the contract (DESIGN.md 10): `--color-primary` becomes `FLColor.primary`, `--space-4`
becomes `FLSpace.s4`, `--radius-md` becomes `FLRadius.md`. `rem` is 16 pt. Only the semantic tokens are
emitted; the raw palette is never used by a component, and `--z-*` and `--touch-target-android` have no iOS
meaning. Anything the generator cannot read is an error, so a token change can never silently drift.
"""
import argparse
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
DEFAULT_TOKENS = HERE / "tokens.css"
DEFAULT_OUT = HERE.parent / "apps/ios/Shared/Sources/Shared/Theme/Tokens.generated.swift"

COLOR_NAMES = [
    "bg", "surface", "surface-sunken", "surface-raised", "text", "text-secondary", "text-tertiary",
    "text-on-primary", "text-link", "border", "border-strong", "primary", "primary-hover", "primary-subtle",
    "agent", "agent-subtle", "success", "success-subtle", "warning", "warning-subtle", "danger",
    "danger-subtle", "focus", "overlay",
]
SHADOW_NAMES = ["sm", "md", "lg"]
SPACE_NAMES = ["0", "1", "2", "3", "4", "5", "6", "8", "10", "12", "16"]
RADIUS_NAMES = ["xs", "sm", "md", "lg", "xl", "full"]
TEXT_NAMES = ["xs", "sm", "base", "md", "lg", "xl", "2xl", "3xl", "4xl"]
LEADING_NAMES = ["tight", "snug", "normal"]
WEIGHT_NAMES = ["regular", "medium", "semibold", "bold"]
TRACKING_NAMES = ["tight", "normal", "wide"]
DURATION_NAMES = ["instant", "fast", "base", "slow"]
EASE_NAMES = ["standard", "enter", "exit"]
SIZE_NAMES = ["control-height-sm", "control-height-md", "control-height-lg", "focus-ring-width", "focus-ring-offset"]
SWIFT_WEIGHT = {"regular": "regular", "medium": "medium", "semibold": "semibold", "bold": "bold"}


class TokenError(Exception):
    pass


def block(css: str, opener: str) -> str:
    """The body of the first rule whose selector line is exactly `opener` (to its closing brace)."""
    m = re.search(re.escape(opener) + r"\s*\{(.*?)\n\s*\}", css, re.S)
    if not m:
        raise TokenError(f"rule not found: {opener}")
    return m.group(1)


def decls(body: str) -> dict:
    return {k: v.strip() for k, v in re.findall(r"--([a-z0-9-]+):\s*([^;]+?);", re.sub(r"/\*.*?\*/", "", body, flags=re.S))}


def number(value: str, what: str) -> float:
    m = re.fullmatch(r"(-?[0-9]*\.?[0-9]+)(rem|px|em|ms)?", value.strip())
    if not m:
        raise TokenError(f"{what}: cannot read {value!r}")
    n, unit = float(m.group(1)), m.group(2)
    return n * 16 if unit == "rem" else n


def fmt(n: float) -> str:
    s = f"{n:.4f}".rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def rgba(value: str, what: str) -> tuple:
    m = re.fullmatch(r"#([0-9a-fA-F]{6})", value)
    if m:
        v = int(m.group(1), 16)
        return (v >> 16, (v >> 8) & 255, v & 255, 1.0)
    m = re.fullmatch(r"rgb\(\s*(\d+)\s+(\d+)\s+(\d+)\s*/\s*([0-9.]+)\s*\)", value)
    if m:
        return (int(m.group(1)), int(m.group(2)), int(m.group(3)), float(m.group(4)))
    raise TokenError(f"{what}: cannot read color {value!r}")


def swift_rgba(c: tuple) -> str:
    return f"FLRGBA(red: {c[0]}, green: {c[1]}, blue: {c[2]}, alpha: {fmt(c[3])})"


def shadow(value: str, what: str) -> tuple:
    m = re.fullmatch(r"(\S+)\s+(\S+)\s+(\S+)\s+(rgb\(.*\))", value)
    if not m:
        raise TokenError(f"{what}: cannot read shadow {value!r}")
    return (number(m.group(1), what), number(m.group(2), what), number(m.group(3), what), rgba(m.group(4), what))


def camel(name: str) -> str:
    head, *rest = name.split("-")
    return head + "".join(p.capitalize() for p in rest)


def need(table: dict, names: list, prefix: str) -> None:
    missing = [f"--{prefix}{n}" for n in names if f"{prefix}{n}" not in table]
    if missing:
        raise TokenError("missing tokens: " + ", ".join(missing))


def parse(css: str) -> dict:
    root = decls(block(css, ":root"))
    light = decls(block(css, ':root,\n:root[data-theme="light"]'))
    dark = decls(block(css, ':root[data-theme="dark"]'))
    media = decls(block(css, '@media (prefers-color-scheme: dark) {\n  :root:not([data-theme="light"])'))
    if media != dark:
        differing = sorted(k for k in media.keys() | dark.keys() if media.get(k) != dark.get(k))
        raise TokenError("the @media dark block and :root[data-theme=\"dark\"] differ: " + ", ".join(differing))
    reduced = decls(block(css, "@media (prefers-reduced-motion: reduce) {\n  :root"))
    for n in DURATION_NAMES:
        if f"duration-{n}" not in reduced or number(reduced[f"duration-{n}"], "reduced motion") != 0:
            raise TokenError(f"reduced motion must set --duration-{n} to 0 (the Swift theme drops the duration to zero)")
    for table, prefix, names in (
        (light, "color-", COLOR_NAMES), (dark, "color-", COLOR_NAMES), (light, "shadow-", SHADOW_NAMES),
        (dark, "shadow-", SHADOW_NAMES), (root, "space-", SPACE_NAMES), (root, "radius-", RADIUS_NAMES),
        (root, "text-", TEXT_NAMES), (root, "leading-", LEADING_NAMES), (root, "weight-", WEIGHT_NAMES),
        (root, "tracking-", TRACKING_NAMES), (root, "duration-", DURATION_NAMES), (root, "ease-", EASE_NAMES),
        (root, "", SIZE_NAMES),
    ):
        need(table, names, prefix)
    return {"root": root, "light": light, "dark": dark}


def generate(css: str) -> str:
    t = parse(css)
    root, light, dark = t["root"], t["light"], t["dark"]
    out = [
        "// Generated by design/gen-swift-theme.py from design/tokens.css (FIG-158). Do not edit: change the",
        "// token, run `python3 design/gen-swift-theme.py`, and commit both. CI fails when they differ.",
        "// swift-format-ignore-file",
        "",
        "import CoreGraphics",
        "import Foundation",
        "import SwiftUI",
        "",
        "/// Semantic colors, light and dark (`--color-*`). They follow the system appearance.",
        "public enum FLColor {",
    ]
    for n in COLOR_NAMES:
        l, d = rgba(light[f"color-{n}"], n), rgba(dark[f"color-{n}"], n)
        out.append(f"    public static let {camel(n)} = flDynamicColor(")
        out.append(f"        light: {swift_rgba(l)},")
        out.append(f"        dark: {swift_rgba(d)})")
    out += ["}", "", "/// Shadows (`--shadow-*`): offset and blur in points, color light and dark.", "public enum FLShadow {"]
    for n in SHADOW_NAMES:
        l, d = shadow(light[f"shadow-{n}"], n), shadow(dark[f"shadow-{n}"], n)
        if l[:3] != d[:3]:
            raise TokenError(f"--shadow-{n} has different geometry in light and dark")
        out.append(f"    public static let {n} = FLShadowSpec(")
        out.append(f"        offsetX: {fmt(l[0])}, offsetY: {fmt(l[1])}, blur: {fmt(l[2])},")
        out.append(f"        light: {swift_rgba(l[3])},")
        out.append(f"        dark: {swift_rgba(d[3])})")
    out += ["}", "", "/// Spacing on the 4 pt grid (`--space-*`, 1 rem = 16 pt).", "public enum FLSpace {"]
    for n in SPACE_NAMES:
        out.append(f"    public static let s{n}: CGFloat = {fmt(number(root[f'space-{n}'], n))}")
    out += ["}", "", "/// Corner radii (`--radius-*`).", "public enum FLRadius {"]
    for n in RADIUS_NAMES:
        out.append(f"    public static let {n}: CGFloat = {fmt(number(root[f'radius-{n}'], n))}")
    out += ["}", "", "/// Type sizes in points (`--text-*`, 1 rem = 16 pt). Scale them with `FLFont`.", "public enum FLText {"]
    for n in TEXT_NAMES:
        out.append(f"    public static let {re.sub(r'^(\d+)(.*)$', r'\2\1', n)}: CGFloat = {fmt(number(root[f'text-{n}'], n))}")
    out += ["}", "", "/// Line height multipliers (`--leading-*`).", "public enum FLLeading {"]
    for n in LEADING_NAMES:
        out.append(f"    public static let {n}: CGFloat = {fmt(number(root[f'leading-{n}'], n))}")
    out += ["}", "", "/// Font weights (`--weight-*`).", "public enum FLWeight {"]
    for n in WEIGHT_NAMES:
        w = int(number(root[f"weight-{n}"], n))
        if w not in (400, 500, 600, 700):
            raise TokenError(f"--weight-{n}: {w} has no SwiftUI weight")
        out.append(f"    public static let {n}: Font.Weight = .{SWIFT_WEIGHT[n]}")
    out += ["}", "", "/// Letter spacing in em (`--tracking-*`); multiply by the point size.", "public enum FLTracking {"]
    for n in TRACKING_NAMES:
        out.append(f"    public static let {n}: CGFloat = {fmt(number(root[f'tracking-{n}'].replace('em', ''), n))}")
    out += ["}", "", "/// Sizes in points (`--control-height-*`, `--focus-ring-*`).", "public enum FLSize {"]
    for n in SIZE_NAMES:
        out.append(f"    public static let {camel(n)}: CGFloat = {fmt(number(root[n], n))}")
    out += ["}", "", "/// Durations in seconds (`--duration-*`). Use `FLMotion` so reduced motion is honoured.", "public enum FLDuration {"]
    for n in DURATION_NAMES:
        out.append(f"    public static let {n}: TimeInterval = {fmt(number(root[f'duration-{n}'], n) / 1000)}")
    out += ["}", "", "/// Easing curves (`--ease-*`) as cubic Bezier control points.", "public enum FLEase {"]
    for n in EASE_NAMES:
        m = re.fullmatch(r"cubic-bezier\(([^)]*)\)", root[f"ease-{n}"])
        if not m:
            raise TokenError(f"--ease-{n}: cannot read {root[f'ease-{n}']!r}")
        p = [number(x, n) for x in m.group(1).split(",")]
        if len(p) != 4:
            raise TokenError(f"--ease-{n}: expected four control points")
        out.append(f"    public static let {n} = FLCubicBezier({', '.join(fmt(x) for x in p)})")
    out += ["}", ""]
    return "\n".join(out)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="fail when the generated file is out of date")
    ap.add_argument("--tokens", type=pathlib.Path, default=DEFAULT_TOKENS)
    ap.add_argument("--out", type=pathlib.Path, default=DEFAULT_OUT)
    args = ap.parse_args()
    try:
        text = generate(args.tokens.read_text())
    except TokenError as e:
        print(f"gen-swift-theme: {e}", file=sys.stderr)
        return 2
    if args.check:
        if not args.out.exists() or args.out.read_text() != text:
            print(f"gen-swift-theme: {args.out} is out of date with {args.tokens}; run python3 design/gen-swift-theme.py", file=sys.stderr)
            return 1
        return 0
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
