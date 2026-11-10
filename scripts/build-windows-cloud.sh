#!/usr/bin/env bash
set -euo pipefail
# Linux cloud helper. Install the toolchain described in docs/WINDOWS_BUILD.md first.
source /workspace/.toolchains/activate-windows.sh
ulimit -c 0
cd "$(dirname "$0")/.."
pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --no-bundle -- --locked
# The Linux Tauri CLI probes tray libraries even when bundling a Windows target.
export PATH="/workspace/.toolchains/bin:$PATH"
export PKG_CONFIG_SYSROOT_DIR=/workspace/.toolchains/linux-desktop
export PKG_CONFIG_PATH=/workspace/.toolchains/linux-desktop/usr/lib/x86_64-linux-gnu/pkgconfig:/workspace/.toolchains/linux-desktop/usr/share/pkgconfig:/workspace/.toolchains/linux-desktop/usr/lib/pkgconfig
pnpm tauri bundle --target x86_64-pc-windows-msvc --bundles nsis
