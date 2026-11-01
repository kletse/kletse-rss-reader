#!/bin/bash
# Builds the Linux downloads locally in an x86_64 Arch Linux container, without releasing anything.
# Releases build them on GitHub Actions instead (release.yml), which is much faster than Docker's
# x86_64 emulation on Apple Silicon. Output: packaging/release/.
#
# Usage: packaging/build-linux-docker.sh [git ref]   (default: HEAD; builds what is committed there)
set -euo pipefail

ref=${1:-HEAD}
root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/packaging/release"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

git -C "$root" archive --format=tar --prefix=kletse-rss-reader/ -o "$work/src.tar" "$ref"
cp "$root/packaging/arch-build.sh" "$work/"
docker run --rm --platform linux/amd64 -v "$work:/work" archlinux:latest bash /work/arch-build.sh /work
mkdir -p "$out"
cp "$work"/out/* "$out/"
ls -l "$out"
