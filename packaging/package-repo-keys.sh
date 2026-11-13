#!/usr/bin/env bash
# Gives package-repos.yml what it needs to write the Homebrew tap and the Scoop bucket, and
# switches those updates on. For each of jprodgers/homebrew-jetcarp and jprodgers/scoop-jetcarp it
# makes a new SSH key pair, adds the public half to that repository as a deploy key with write
# access (replacing an earlier one made by this script), and stores the private half as the secret
# DEPLOY_KEY in the environment `homebrew` or `scoop` of jprodgers/jetcarp. The private keys exist
# only in a temporary folder, which is deleted at the end; nothing is printed but what was done.
# Deploy keys don't expire; run this again to replace both.
#
#   packaging/package-repo-keys.sh      # needs gh, logged in as the repositories' owner, and ssh-keygen
set -euo pipefail

owner=jprodgers
main=$owner/jetcarp
title="jetcarp release workflow (package-repos.yml)"

umask 077
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

for kind in homebrew scoop; do
    repo=$owner/$kind-jetcarp
    ssh-keygen -q -t ed25519 -N "" -C "$title, $repo" -f "$tmp/$kind"
    gh api -X PUT "repos/$main/environments/$kind" --silent
    for id in $(gh api "repos/$repo/keys" --jq ".[] | select(.title == \"$title\") | .id"); do
        gh api -X DELETE "repos/$repo/keys/$id" --silent
    done
    gh api "repos/$repo/keys" -f title="$title" -f key="$(cat "$tmp/$kind.pub")" -F read_only=false --silent
    gh secret set DEPLOY_KEY --env "$kind" --repo "$main" < "$tmp/$kind"
    echo "$kind: deploy key added to $repo, secret DEPLOY_KEY set in the environment $kind"
done

gh variable set UPDATE_HOMEBREW --body true --repo "$main"
gh variable set UPDATE_SCOOP --body true --repo "$main"
echo "UPDATE_HOMEBREW and UPDATE_SCOOP are on: each release now updates the tap and the bucket."
