#!/usr/bin/env bash
# Local replacement for the GitHub CI and release jobs.
#
#   tools/local-ci.sh check          fmt + clippy + tests for Linux and for
#                                    Windows (cross-compiled, tests under Wine)
#   tools/local-ci.sh dist <outdir>  release artifacts like release.yml:
#                                    Windows .exe, Linux .AppImage (linked by
#                                    zig cc against glibc 2.35, so it runs on
#                                    glibc 2.35+), latest.json and SHA256SUMS
#
# Needs: rustup target x86_64-pc-windows-gnu, mingw64-gcc, wine (check);
# zig and appimagetool in $RC_TOOLS (default: ../../Tools, the workspace's
# Tools folder) for dist.
set -euo pipefail
cd "$(dirname "$0")/.."

export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine
export WINEDEBUG=-all
WIN=x86_64-pc-windows-gnu
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

step() { printf '\n== %s\n' "$*"; }

check() {
    step "fmt"
    cargo fmt --all -- --check
    step "clippy (linux)"
    cargo clippy --workspace --all-targets -- -D warnings
    step "clippy (windows)"
    cargo clippy --workspace --all-targets --target "$WIN" -- -D warnings
    step "test (linux)"
    cargo test --workspace -q
    step "test (windows, wine)"
    cargo test --workspace -q --target "$WIN"
    step "all checks passed"
}

dist() {
    local out=${1:?usage: local-ci.sh dist <outdir>}
    mkdir -p "$out"
    out=$(cd "$out" && pwd)
    step "windows .exe ($VER)"
    RUSTFLAGS="-C target-feature=+crt-static" cargo build --release -p rustcraft --target "$WIN"
    cp "target/$WIN/release/rustcraft.exe" "$out/rustcraft-$VER-windows-x86_64.exe"

    step "linux .AppImage ($VER), glibc 2.35 via zig cc"
    # Linking against the host's glibc would require the newest glibc to run;
    # zig cc links against glibc 2.35 symbols (the minimum platform, ADR-003).
    local tools=${RC_TOOLS:-$PWD/../../Tools}
    local zig
    zig=$(ls -d "$tools"/zig-linux-x86_64-*/zig 2>/dev/null | tail -1)
    [ -x "$zig" ] || { echo "zig not found in $tools (set RC_TOOLS)" >&2; exit 1; }
    local wrap="$PWD/target/zigcc"
    mkdir -p "$wrap"
    # cc-rs passes its own --target=<rust triple>, which zig rejects: drop it.
    cat > "$wrap/cc" <<EOS
#!/bin/sh
for a in "\$@"; do
    shift
    case "\$a" in --target=*) ;; *) set -- "\$@" "\$a" ;; esac
done
exec "$zig" cc -target x86_64-linux-gnu.2.35 "\$@"
EOS
    chmod +x "$wrap/cc"
    CC_x86_64_unknown_linux_gnu="$wrap/cc" \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$wrap/cc" \
        cargo build --release -p rustcraft --target x86_64-unknown-linux-gnu
    local bin=target/x86_64-unknown-linux-gnu/release/rustcraft
    echo "highest glibc symbol: $(objdump -T "$bin" | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -1)"
    local app="$PWD/target/AppDir"
    rm -rf "$app" && mkdir -p "$app/usr/bin"
    cp "$bin" "$app/usr/bin/"
    cp packaging/appimage/rustcraft.desktop packaging/appimage/rustcraft.png "$app/"
    cp packaging/appimage/AppRun "$app/" && chmod +x "$app/AppRun"
    ARCH=x86_64 "$tools/appimagetool-x86_64.AppImage" -n "$app" \
        "$out/rustcraft-$VER-linux-x86_64.AppImage" >/dev/null 2>&1

    step "manifest + checksums"
    cargo run -q -p xtask -- manifest "$VER" "$out"
    (cd "$out" && sha256sum -c SHA256SUMS)
    ls -la "$out"
}

case "${1:-check}" in
    check) check ;;
    dist) dist "${2:-}" ;;
    *) echo "usage: $0 check | dist <outdir>" >&2; exit 2 ;;
esac
