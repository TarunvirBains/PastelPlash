#!/usr/bin/env bash
# Builds PastelPlash mods from an OoT Reloaded .o2r pack and installs one into Ship of Harkinian.
# Works from Git Bash on Windows and from bash in WSL. Needs only pastelplash.exe, coreutils and
# (optionally, for VRAM numbers) nvidia-smi.
#
#   scripts/make-mod.sh                 # test mod: Kokiri Forest + Link, all styles
#   FULL=1 scripts/make-mod.sh          # every texture in the pack
#   STYLES=impressionist scripts/make-mod.sh
#   NO_INSTALL=1 scripts/make-mod.sh    # build only
#
# Settings (environment variables):
#   SOH_DIR        Ship of Harkinian folder (default: autodetected, see below)
#   PACK           source pack (default: $SOH_DIR/mods/OoT_Reloaded_v11.0.0_4K.o2r)
#   STYLES         styles to build, space-separated (default: all three presets)
#   INSTALL_STYLE  style installed into $SOH_DIR/mods (default: skyward-watercolor); the others
#                  go to $SOH_DIR/pastelplash-variants/ for swapping in
#   INCLUDE        entry globs, space-separated (default: the Kokiri Forest + Link test set)
#   FULL=1         no INCLUDE filter: restyle the whole pack
#   JOBS           worker threads (default: all cores)
#   OUT_DIR        where built mods and logs go (default: <repo>/target/mods)
#   NO_INSTALL=1   don't copy anything into the Ship of Harkinian folder
set -euo pipefail

REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
EXE="$REPO/target/x86_64-pc-windows-gnu/release/pastelplash.exe"

# Windows path for the .exe (it doesn't understand /mnt/z or /z paths).
winpath() {
    if command -v wslpath >/dev/null 2>&1; then
        wslpath -w "$1"
    elif command -v cygpath >/dev/null 2>&1; then
        cygpath -w "$1"
    else
        printf '%s\n' "$1"
    fi
}

now() { date +%s.%N; }
elapsed() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.1f", b - a }'; }

if [[ -z "${SOH_DIR:-}" ]]; then
    for d in "/mnt/z/Games/Ocarina of Time/SoH-9.2.3-celshade0.11-Win64" \
             "/z/Games/Ocarina of Time/SoH-9.2.3-celshade0.11-Win64"; do
        [[ -d "$d" ]] && SOH_DIR=$d && break
    done
fi
[[ -n "${SOH_DIR:-}" && -d "$SOH_DIR" ]] || { echo "set SOH_DIR to your Ship of Harkinian folder" >&2; exit 1; }
PACK=${PACK:-"$SOH_DIR/mods/OoT_Reloaded_v11.0.0_4K.o2r"}
[[ -f "$PACK" ]] || { echo "pack not found: $PACK" >&2; exit 1; }
STYLES=${STYLES:-"skyward-watercolor ss-baseline impressionist"}
INSTALL_STYLE=${INSTALL_STYLE:-skyward-watercolor}
OUT_DIR=${OUT_DIR:-"$REPO/target/mods"}
mkdir -p "$OUT_DIR"
LOG="$OUT_DIR/make-mod.log"

if [[ "${FULL:-0}" == 1 ]]; then
    INCLUDE=""
    SCOPE=full
else
    INCLUDE=${INCLUDE:-"alt/scenes/*/spot04_scene/** alt/objects/object_spot04_objects/** alt/objects/object_link_boy/** alt/objects/object_link_child/** alt/objects/gameplay_field_keep/**"}
    SCOPE=test
fi
include_args=()
for g in $INCLUDE; do include_args+=(--include "$g"); done

# Build when a Rust toolchain is available (WSL); otherwise use the existing binary.
if command -v cargo >/dev/null 2>&1; then
    echo "building release binary..."
    (cd "$REPO" && cargo build --release --quiet)
fi
[[ -f "$EXE" ]] || { echo "missing $EXE (build it in WSL with: cargo build --release)" >&2; exit 1; }

SMI=$(command -v nvidia-smi 2>/dev/null || command -v nvidia-smi.exe 2>/dev/null || true)
[[ -z "$SMI" && -x /usr/lib/wsl/lib/nvidia-smi ]] && SMI=/usr/lib/wsl/lib/nvidia-smi

# Samples used VRAM (MiB) every 0.25 s into $1 until stopped.
vram_start() {
    [[ -n "$SMI" ]] || return 0
    (while :; do "$SMI" --query-gpu=memory.used --format=csv,noheader,nounits 2>/dev/null | head -1; sleep 0.25; done) >"$1" &
    VRAM_PID=$!
}
vram_stop() {
    [[ -n "${VRAM_PID:-}" ]] || return 0
    kill "$VRAM_PID" 2>/dev/null || true
    wait "$VRAM_PID" 2>/dev/null || true
    VRAM_PID=
    sort -n "$1" | awk 'NR == 1 { lo = $1 } { hi = $1 } END { if (NR) printf "VRAM used: baseline %d MiB, peak %d MiB (+%d MiB)\n", lo, hi, hi - lo }'
}

{
    echo "== $(date) scope=$SCOPE pack=$PACK"
    echo "styles: $STYLES"
    [[ -n "$INCLUDE" ]] && echo "include: $INCLUDE"
    [[ -n "$SMI" ]] || echo "(nvidia-smi not found: no VRAM numbers)"
} | tee -a "$LOG"

total_start=$(now)
for style in $STYLES; do
    out="$OUT_DIR/PastelPlash-$style-$SCOPE.o2r"
    echo "--- $style -> $out" | tee -a "$LOG"
    start=$(now)
    vram_start "$OUT_DIR/vram-$style.txt"
    "$EXE" o2r "$(winpath "$PACK")" "$(winpath "$out")" \
        --style "$(winpath "$REPO/styles/$style.toml")" \
        --target "$(winpath "$REPO/targets/soh-celshade.toml")" \
        --pack "$(winpath "$REPO/packs/oot-reloaded.toml")" \
        ${JOBS:+--jobs "$JOBS"} "${include_args[@]}" 2>&1 \
        | tee "$OUT_DIR/$style-$SCOPE.log" | grep -v '^  ' | tee -a "$LOG"
    status=${PIPESTATUS[0]}
    vram_stop "$OUT_DIR/vram-$style.txt" | tee -a "$LOG"
    echo "$style: $(elapsed "$start" "$(now)") s, $(du -h "$out" 2>/dev/null | cut -f1) (per-texture lines: $OUT_DIR/$style-$SCOPE.log)" | tee -a "$LOG"
    [[ $status == 0 ]] || { echo "$style failed" | tee -a "$LOG"; exit "$status"; }
done
echo "all styles: $(elapsed "$total_start" "$(now)") s" | tee -a "$LOG"

if [[ "${NO_INSTALL:-0}" != 1 ]]; then
    mkdir -p "$SOH_DIR/pastelplash-variants"
    for style in $STYLES; do
        src="$OUT_DIR/PastelPlash-$style-$SCOPE.o2r"
        if [[ "$style" == "$INSTALL_STYLE" ]]; then
            cp "$src" "$SOH_DIR/mods/"
            echo "installed $(basename "$src") into $SOH_DIR/mods" | tee -a "$LOG"
        else
            cp "$src" "$SOH_DIR/pastelplash-variants/"
        fi
    done
    echo "variants: $SOH_DIR/pastelplash-variants (move one into mods/ to switch)" | tee -a "$LOG"
    echo "In SoH: enable it in the Mod Menu and order it after OoT Reloaded." | tee -a "$LOG"
fi
