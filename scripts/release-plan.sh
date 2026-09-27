#!/usr/bin/env bash
# Decides what a release run builds, from the GitHub event. Prints `key=value` lines for
# $GITHUB_OUTPUT: version, channel, tag, publish, prerelease, skip.
#
#   tag push vX.Y.Z          -> stable,  version X.Y.Z,        rows wait for a release manager
#   tag push vX.Y.Z-beta.N   -> beta,    version X.Y.Z-beta.N, rows wait for a release manager
#   schedule / dispatch      -> nightly or weekly, version <next>-<channel>.<YYYYMMDD>.<run>, where
#                               <next> is the workspace version, or its next patch once released,
#                               and <YYYYMMDD> is the day the run was created, so a re-run
#                               rebuilds the same tag; published to the update server with no
#                               confirmation
#
# Inputs (environment): EVENT, REF_NAME, CHANNEL (nightly|weekly for schedule/dispatch),
# RUN_ID, RUN_NUMBER, SHA, REPO, GH_TOKEN. Run from the repository root.
set -euo pipefail

workspace=$(sed -n '/^\[workspace.package\]/,/^\[/ s/^version = "\(.*\)"/\1/p' Cargo.toml)
[[ -n "$workspace" ]] || { echo "no [workspace.package] version in Cargo.toml" >&2; exit 1; }

pubkey=$(sed -n 's/^ *"pubkey": *"\(.*\)",\{0,1\}$/\1/p' packaging/updater.json)
if [[ -z "$pubkey" ]]; then
  echo "packaging/updater.json has no pubkey: released copies could never verify an update." >&2
  echo "Generate one with 'cargo xtask signer generate -w <file>' (or reuse your Tauri key) and commit the public key." >&2
  exit 1
fi

skip=false
case "$EVENT" in
  push)
    version="${REF_NAME#v}"
    # The prerelease is <channel>[.<fields>], because rpm and deb cannot hold a second '-' and
    # pacman only sorts a prerelease below its release when it starts with a letter.
    if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z][0-9A-Za-z.]*)?$ ]]; then
      echo "tag $REF_NAME is not vX.Y.Z or vX.Y.Z-<channel>[.<fields>]" >&2; exit 1
    fi
    if [[ "${version%%-*}" != "$workspace" ]]; then
      echo "tag $REF_NAME does not match the workspace version $workspace; bump Cargo.toml first" >&2; exit 1
    fi
    if [[ "$version" == *-* ]]; then
      prerelease_part="${version#*-}"
      channel="${prerelease_part%%.*}"
    else
      channel=stable
    fi
    if [[ "$channel" == nightly || "$channel" == weekly ]]; then
      echo "tag $REF_NAME: $channel builds come from the schedule or 'Run workflow', not from tags" >&2; exit 1
    fi
    tag="$REF_NAME"
    ;;
  schedule | workflow_dispatch)
    [[ "$CHANNEL" == nightly || "$CHANNEL" == weekly ]] || { echo "unknown channel '$CHANNEL'" >&2; exit 1; }
    channel="$CHANNEL"
    base="$workspace"
    # A prerelease sorts below its release, so once v<workspace> is out, builds from main are
    # prereleases of the next patch; otherwise nightly users would be offered the older stable.
    if gh release view "v$workspace" --repo "$REPO" >/dev/null 2>&1; then
      IFS=. read -r major minor patch <<<"$workspace"
      base="$major.$minor.$((patch + 1))"
    fi
    day=$(gh api "repos/$REPO/actions/runs/$RUN_ID" --jq '.created_at[:10] | gsub("-"; "")')
    version="$base-$channel.$day.$RUN_NUMBER"
    tag="v$version"
    if [[ "$EVENT" == schedule ]]; then
      # An idle repository should not publish a new build every night. A re-run computes the
      # same tag and has to finish that release, not skip it.
      last=$(gh release list --repo "$REPO" --limit 100 --json tagName \
        --jq "[.[].tagName | select(test(\"-$channel\\\\.\"))][0] // empty")
      if [[ -n "$last" && "$last" != "$tag" ]] && [[ "$(gh api "repos/$REPO/commits/$last" --jq .sha)" == "$SHA" ]]; then
        echo "$last already built $SHA; skipping" >&2
        skip=true
      fi
    fi
    ;;
  *)
    echo "unsupported event $EVENT" >&2; exit 1 ;;
esac

case "$channel" in
  nightly | weekly) publish=true ;;
  *) publish=false ;;
esac
prerelease=$([[ "$channel" == stable ]] && echo false || echo true)

printf '%s\n' "version=$version" "channel=$channel" "tag=$tag" "publish=$publish" "prerelease=$prerelease" "skip=$skip"
