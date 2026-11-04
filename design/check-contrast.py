#!/usr/bin/env python3
"""Check WCAG contrast of the semantic color pairs in design/tokens.css (FIG-22).

Reads the light block (:root) and the explicit dark block (:root[data-theme="dark"]) and fails if any
pair is below its threshold or if the @media (prefers-color-scheme: dark) copy of the dark palette differs. Usage: python3 design/check-contrast.py
"""
import pathlib
import re
import sys

CSS = pathlib.Path(__file__).with_name("tokens.css").read_text()

# (foreground token, background token, minimum ratio)
PAIRS = [
    ("text", "bg", 4.5), ("text", "surface", 4.5), ("text-secondary", "bg", 4.5),
    ("text-secondary", "surface", 4.5), ("text-tertiary", "bg", 4.5), ("text-link", "bg", 4.5),
    ("primary", "bg", 4.5), ("text-on-primary", "primary", 4.5),
    ("agent", "bg", 4.5), ("success", "bg", 4.5), ("warning", "bg", 4.5), ("danger", "bg", 4.5),
    ("success", "success-subtle", 4.5), ("warning", "warning-subtle", 4.5),
    ("danger", "danger-subtle", 4.5), ("agent", "agent-subtle", 4.5), ("primary", "primary-subtle", 4.5),
    ("border-strong", "bg", 3.0), ("focus", "bg", 3.0),
]


def block(selector: str) -> dict:
    m = re.search(re.escape(selector) + r"\s*\{(.*?)\n\}", CSS, re.S)
    if not m:
        sys.exit(f"selector not found: {selector}")
    return dict(re.findall(r"--color-([a-z-]+):\s*(#[0-9a-fA-F]{6})", m.group(1)))


def lum(hexcolor: str) -> float:
    r, g, b = (int(hexcolor[i:i + 2], 16) / 255 for i in (1, 3, 5))
    f = lambda c: c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)


def ratio(a: str, b: str) -> float:
    hi, lo = sorted((lum(a), lum(b)), reverse=True)
    return (hi + 0.05) / (lo + 0.05)


failed = False
media = re.search(
    r'@media \(prefers-color-scheme: dark\)\s*\{\s*:root:not\(\[data-theme="light"\]\)\s*\{(.*?)\n  \}\n\}', CSS, re.S
)
if not media:
    sys.exit("dark block inside @media (prefers-color-scheme: dark) not found")
media_colors = dict(re.findall(r"--color-([a-z-]+):\s*(#[0-9a-fA-F]{6})", media.group(1)))
explicit_dark = block(':root[data-theme="dark"]')
if media_colors != explicit_dark:
    differing = sorted(k for k in media_colors.keys() | explicit_dark.keys() if media_colors.get(k) != explicit_dark.get(k))
    print(f"FAIL dark palette differs between @media block and :root[data-theme=\"dark\"]: {', '.join(differing)}")
    failed = True
for theme, selector in (("light", ':root[data-theme="light"]'), ("dark", ':root[data-theme="dark"]')):
    colors = block(selector) if theme == "dark" else block(":root,\n" + selector)
    for fg, bg, minimum in PAIRS:
        r = ratio(colors[fg], colors[bg])
        ok = r >= minimum
        failed |= not ok
        print(f"{'ok  ' if ok else 'FAIL'} {theme:5} {fg:>16} on {bg:<16} {r:5.2f} (min {minimum})")
sys.exit(1 if failed else 0)
