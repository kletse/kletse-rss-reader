#!/bin/bash
# Fails unless every file that carries the app version agrees on it.
# A release bumps Cargo.toml (and Cargo.lock, which cargo updates) and packaging/PKGBUILD.
#
# Usage: packaging/check-version.sh   (prints the version when they all match)
set -euo pipefail
cd "$(dirname "$0")/.."

files=(
  Cargo.toml
  Cargo.lock
  packaging/PKGBUILD
)
versions=(
  "$(awk -F'"' '/^\[/ { pkg = ($0 == "[package]") } pkg && /^version *=/ { print $2; exit }' Cargo.toml)"
  "$(awk -F'"' '/^name = "kletse-rss"$/ { found = 1; next } found && /^version = / { print $2; exit }' Cargo.lock)"
  "$(sed -n 's/^pkgver=//p' packaging/PKGBUILD)"
)

expected=${versions[0]}
for version in "${versions[@]}"; do
  if [ -z "$version" ] || [ "$version" != "$expected" ]; then
    echo "The version differs between files:" >&2
    for i in "${!files[@]}"; do
      printf '  %-20s %s\n' "${files[$i]}" "${versions[$i]:-(none found)}" >&2
    done
    exit 1
  fi
done
echo "$expected"
