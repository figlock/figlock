#!/usr/bin/env bash
# Download every asset of a draft release into a directory (FIG-115).
#
# Usage: scripts/download-draft-assets.sh <tag> <dir>
#
# `gh release download <tag>` cannot see a draft (the tag endpoint skips drafts), so this finds the draft through
# the releases list and fetches each asset by id. Needs GH_TOKEN with contents: write, and GITHUB_REPOSITORY.
# Writes the release id to <dir>/.release-id.
set -euo pipefail

tag=${1:?usage: download-draft-assets.sh <tag> <dir>}
dir=${2:?usage: download-draft-assets.sh <tag> <dir>}
repo=${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is not set}

id=$(gh api "repos/$repo/releases" --paginate --jq ".[] | select(.draft and .tag_name == \"$tag\") | .id" | head -1)
[ -n "$id" ] || {
  echo "no draft release for $tag in $repo" >&2
  exit 1
}
mkdir -p "$dir"
gh api "repos/$repo/releases/$id/assets" --paginate --jq '.[] | "\(.id) \(.name)"' | while read -r asset name; do
  gh api -H "Accept: application/octet-stream" "repos/$repo/releases/assets/$asset" > "$dir/$name"
done
echo "$id" > "$dir/.release-id"
echo "Downloaded $(find "$dir" -type f ! -name .release-id | wc -l | tr -d ' ') assets of draft release $id ($tag)"
