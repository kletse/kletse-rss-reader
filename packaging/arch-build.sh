#!/bin/bash
# Builds the Arch package and the standalone Linux binary inside an x86_64 archlinux container.
# Used by .github/workflows/release.yml (native runner) and build-linux-docker.sh (local Docker).
#
# Usage: arch-build.sh WORK_DIR
#   WORK_DIR/src.tar  the source, as made by `git archive --prefix=kletse-rss-reader/`
#   WORK_DIR/out/     receives kletse-rss-<version>-1-x86_64.pkg.tar.zst and kletse-rss-<version>-x86_64
set -euo pipefail

work=${1:?usage: arch-build.sh WORK_DIR}
mkdir -p "$work/out"

# pacman's download sandbox needs seccomp, which Docker on Apple Silicon (x86_64 under emulation)
# can't set up. The container is thrown away after the build, so the sandbox is turned off everywhere.
sed -i '/^\[options\]/a DisableSandbox' /etc/pacman.conf
pacman -Syu --noconfirm --needed base-devel rust npm webkit2gtk-4.1 gtk3 >/dev/null

# makepkg refuses to run as root.
useradd -m builder
tar -xf "$work/src.tar" -C /home/builder
chown -R builder /home/builder/kletse-rss-reader
su builder -c 'cd ~/kletse-rss-reader/packaging && makepkg -f --noconfirm'

root=/home/builder/kletse-rss-reader
version=$(sed -n 's/^pkgver=//p' "$root/packaging/PKGBUILD")
cp "$root"/packaging/*.pkg.tar.zst "$work/out/"
cp "$root/src-tauri/target/release/kletse-rss" "$work/out/kletse-rss-$version-x86_64"
