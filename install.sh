#!/bin/sh
# Installs the latest Helm release into /Applications (specs/update.md §2).
# curl sets no quarantine attribute ⇒ no Gatekeeper prompt despite the ad-hoc
# signature. One-liner:
#   curl -fsSL https://raw.githubusercontent.com/davidbonan/Helm/main/install.sh | sh
set -eu

api="https://api.github.com/repos/davidbonan/Helm/releases/latest"
url="$(curl -fsSL "$api" | grep -o '"browser_download_url": *"[^"]*helm-macos\.zip"' | head -1 | sed -E 's/.*"(https[^"]*)"/\1/')"
[ -n "$url" ] || { echo "error: no helm-macos.zip asset in the latest release" >&2; exit 1; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
echo "Downloading $url"
curl -fSL "$url" -o "$tmp/helm-macos.zip"
rm -rf /Applications/helm.app
ditto -x -k "$tmp/helm-macos.zip" /Applications
echo "Installed /Applications/helm.app"
open /Applications/helm.app
