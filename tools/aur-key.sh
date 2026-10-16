#!/usr/bin/env bash
# Gives the release its own key for the AUR: made here, kept only as the secret
# AUR_SSH_KEY of the environment aur, which main alone may use. Prints the public
# half for the AUR account's SSH Public Key field, as one more line there.
# Needs gh with admin rights on the repository.
#
#   tools/aur-key.sh
set -euo pipefail

repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
ssh-keygen -q -t ed25519 -N '' -C "${repo} release" -f "${work}/key"

gh api --method PUT "repos/${repo}/environments/aur" --input - >/dev/null <<'JSON'
{"deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true}}
JSON
# One that exists already answers 303, and gh follows it there.
gh api --method POST "repos/${repo}/environments/aur/deployment-branch-policies" \
    -f name=main -f type=branch >/dev/null
gh secret set AUR_SSH_KEY --env aur --repo "$repo" <"${work}/key"

echo "Add this line at https://aur.archlinux.org/account/ under SSH Public Key:"
cat "${work}/key.pub"
