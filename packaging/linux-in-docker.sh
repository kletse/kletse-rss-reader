#!/bin/bash
# Runs inside an x86_64 archlinux container (see release.sh): builds the Arch package
# and the standalone binary from /work/src.tar and copies them to /work/out.
set -euo pipefail

# Under emulation (Docker on Apple Silicon) pacman's download sandbox can't set up seccomp.
# This container is thrown away after the build, so turn the sandbox off.
sed -i '/^\[options\]/a DisableSandbox' /etc/pacman.conf
pacman -Syu --noconfirm --needed base-devel rust npm webkit2gtk-4.1 gtk3 >/dev/null

# makepkg refuses to run as root.
useradd -m builder
tar -xf /work/src.tar -C /home/builder
chown -R builder /home/builder/kletse-rss-reader
su builder -c 'cd ~/kletse-rss-reader/packaging && makepkg -f --noconfirm'

root=/home/builder/kletse-rss-reader
version=$(sed -n 's/^pkgver=//p' "$root/packaging/PKGBUILD")
cp "$root"/packaging/*.pkg.tar.zst /work/out/
cp "$root/src-tauri/target/release/kletse-rss" "/work/out/kletse-rss-$version-x86_64"
