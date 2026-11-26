#!/usr/bin/env bash
# Tests for scripts/ios-version.sh (FIG-175): the tag must match the workspace version, a pre-release suffix is
# dropped, and a bad version or build number fails for its own reason.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
script="$here/ios-version.sh"
work=$(mktemp -d)
[ -n "$work" ] && [ -d "$work" ] || exit 2
trap 'rm -rf -- "${work:?}"' EXIT

fail=0
cargo_for() { printf '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "%s"\nedition = "2024"\n' "$1" > "$work/Cargo.toml"; }

expect_ok() { # <description> <expected output> <version> <tag> <build>
  local desc=$1 want=$2 out
  cargo_for "$3"
  if out=$(CARGO_TOML="$work/Cargo.toml" "$script" "$4" "$5" 2> "$work/err") && [ "$out" = "$want" ]; then
    echo "ok   $desc"
  else
    echo "FAIL $desc: got '$out' $(cat "$work/err")" >&2
    fail=1
  fi
}
expect_fail() { # <description> <message fragment> <version> <tag> <build>
  local desc=$1 want=$2
  cargo_for "$3"
  if CARGO_TOML="$work/Cargo.toml" "$script" "$4" "$5" > /dev/null 2> "$work/err"; then
    echo "FAIL $desc: expected a failure" >&2
    fail=1
  elif grep -q -- "$want" "$work/err"; then
    echo "ok   $desc"
  else
    echo "FAIL $desc: wrong error: $(cat "$work/err")" >&2
    fail=1
  fi
}

expect_ok "a release tag" $'MARKETING_VERSION=1.2.3\nCURRENT_PROJECT_VERSION=42' 1.2.3 v1.2.3 42
expect_ok "a pre-release builds as its x.y.z" $'MARKETING_VERSION=0.6.0\nCURRENT_PROJECT_VERSION=7' 0.6.0-rc1 v0.6.0-rc1 7
expect_ok "workspace instead of a tag" $'MARKETING_VERSION=1.2.3\nCURRENT_PROJECT_VERSION=9' 1.2.3 workspace 9
expect_fail "workspace still needs a valid version" "no x.y.z" 1.2 workspace 9
expect_fail "a tag that is not the workspace version" "does not match" 1.2.3 v1.2.4 42
expect_fail "a tag without the v" "does not match" 1.2.3 1.2.3 42
expect_fail "a version without three numbers" "no x.y.z" 1.2 v1.2 42
expect_fail "a build number of zero" "not a positive integer" 1.2.3 v1.2.3 0
expect_fail "a build number that is not a number" "not a positive integer" 1.2.3 v1.2.3 abc

[ "$fail" = 0 ] || exit 1
echo "all ios-version checks passed"
