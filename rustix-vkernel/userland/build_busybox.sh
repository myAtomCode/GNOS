#!/bin/bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="$ROOT_DIR/build/userland"
OUTPUT="$OUTPUT_DIR/busybox"
MUSL_VERSION=1.2.5
BUSYBOX_VERSION=1_36_1
MUSL_SHA256=a9a118bbe84d8764da0ea0d28b3ab3fae8477fc7e4085d90102b8596fc7c75e4
BUSYBOX_SHA256=ea5494846c51d946e5e801d1c099b438f683af8147e0be24ca4001073143110f

if [ -x "$OUTPUT" ]; then
    exit 0
fi

for tool in curl tar sha256sum gcc make ar ranlib strip; do
    command -v "$tool" >/dev/null || { echo "Error: missing userland tool: $tool"; exit 1; }
done

mkdir -p "$OUTPUT_DIR/cache" "$OUTPUT_DIR/src" "$OUTPUT_DIR/musl"
MUSL_ARCHIVE="$OUTPUT_DIR/cache/musl-$MUSL_VERSION.tar.gz"
BUSYBOX_ARCHIVE="$OUTPUT_DIR/cache/busybox-$BUSYBOX_VERSION.tar.gz"
fetch_archive() {
    local url="$1" out="$2" sum="$3"
    if [ -f "$out" ] && echo "$sum  $out" | sha256sum -c --status 2>/dev/null; then
        return
    fi
    rm -f "$out"
    curl -L --fail --retry 3 --http1.1 -o "$out" "$url"
    echo "$sum  $out" | sha256sum -c -
}
fetch_archive "https://musl.libc.org/releases/musl-$MUSL_VERSION.tar.gz" "$MUSL_ARCHIVE" "$MUSL_SHA256"
fetch_archive "https://github.com/mirror/busybox/archive/refs/tags/$BUSYBOX_VERSION.tar.gz" "$BUSYBOX_ARCHIVE" "$BUSYBOX_SHA256"

MUSL_SOURCE="$OUTPUT_DIR/src/musl-$MUSL_VERSION"
BUSYBOX_SOURCE="$OUTPUT_DIR/src/busybox-$BUSYBOX_VERSION"
rm -rf "$MUSL_SOURCE" "$BUSYBOX_SOURCE"
mkdir -p "$MUSL_SOURCE" "$BUSYBOX_SOURCE"
tar -xzf "$MUSL_ARCHIVE" -C "$MUSL_SOURCE" --strip-components=1
tar -xzf "$BUSYBOX_ARCHIVE" -C "$BUSYBOX_SOURCE" --strip-components=1

(cd "$MUSL_SOURCE" && CC=gcc AR=ar RANLIB=ranlib ./configure \
    --prefix="$OUTPUT_DIR/musl" --target=x86_64 --disable-shared >/dev/null)
make -C "$MUSL_SOURCE" -j2 >/dev/null
make -C "$MUSL_SOURCE" install >/dev/null

make -C "$BUSYBOX_SOURCE" allnoconfig >/dev/null
for key in STATIC ASH SH_IS_ASH ASH_ECHO ASH_PRINTF ASH_TEST ECHO PRINTF TEST TRUE FALSE CAT \
    FEATURE_SH_STANDALONE FEATURE_PREFER_APPLETS FEATURE_SH_NOFORK FEATURE_SH_MATH \
    FEATURE_SH_MATH_64 FEATURE_SH_EXTRA_QUIET; do
    sed -i -E "s/^# CONFIG_${key} is not set/CONFIG_${key}=y/" "$BUSYBOX_SOURCE/.config"
done
make -C "$BUSYBOX_SOURCE" oldconfig < /dev/null > /dev/null
make -C "$BUSYBOX_SOURCE" -j2 CC="$OUTPUT_DIR/musl/bin/musl-gcc" SKIP_STRIP=y busybox >/dev/null
strip -s "$BUSYBOX_SOURCE/busybox"
install -m 755 "$BUSYBOX_SOURCE/busybox" "$OUTPUT"
echo "Built static musl BusyBox: $OUTPUT ($(stat -c%s "$OUTPUT") bytes)"
