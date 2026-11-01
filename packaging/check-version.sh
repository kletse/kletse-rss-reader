#!/bin/bash
# Fails unless every file that carries the app version agrees on it.
# A release bumps package.json (and package-lock.json), src-tauri/Cargo.toml and packaging/PKGBUILD;
# tauri.conf.json has no version of its own, so Tauri takes the one in Cargo.toml.
#
# Usage: packaging/check-version.sh   (prints the version when they all match)
set -euo pipefail
cd "$(dirname "$0")/.."

files=(
  package.json
  "package-lock.json"
  "package-lock.json (root package)"
  src-tauri/Cargo.toml
  packaging/PKGBUILD
)
versions=(
  "$(node -p 'require("./package.json").version')"
  "$(node -p 'require("./package-lock.json").version')"
  "$(node -p 'require("./package-lock.json").packages[""].version')"
  "$(awk -F'"' '/^\[/ { pkg = ($0 == "[package]") } pkg && /^version *=/ { print $2; exit }' src-tauri/Cargo.toml)"
  "$(sed -n 's/^pkgver=//p' packaging/PKGBUILD)"
)

expected=${versions[0]}
for version in "${versions[@]}"; do
  if [ -z "$version" ] || [ "$version" != "$expected" ]; then
    echo "The version differs between files:" >&2
    for i in "${!files[@]}"; do
      printf '  %-34s %s\n' "${files[$i]}" "${versions[$i]:-(none found)}" >&2
    done
    exit 1
  fi
done
echo "$expected"
