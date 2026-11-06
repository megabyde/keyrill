#!/bin/sh
# Builds the macOS input method shim with Command Line Tools clang only (no Xcode, no XCTest)
#
#   build.sh check     format check, plist lint, icon check, static analysis, app executable,
#                      diagnostic tools and controlled tests (release lib)
#   build.sh sanitize  icon check and controlled tests under ASan/UBSan (debug lib)
#   build.sh bundle    assemble KeyrillAlpha.app in the output directory; never installs it
#   build.sh status    read current and Keyrill input-source identity/flags; never selects
#   build.sh source-plan  inspect the exact Keyrill parent; no settings changes
#
# CXX, CLANG_FORMAT  compilers/tools resolved from PATH unless set
# KEYRILL_RUST_LIB   prebuilt keyrill-ffi static library; otherwise cargo builds it
# KEYRILL_NATIVE_OUT output directory (default: <cargo target dir>/native-macos)
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
target=${CARGO_TARGET_DIR:-$root/target}
case "$target" in
/*) ;;
*) target=$root/$target ;;
esac
out=${KEYRILL_NATIVE_OUT:-$target/native-macos}
cxx=${CXX:-clang++}
clang_format=${CLANG_FORMAT:-clang-format}

sdk=$(xcrun --show-sdk-path)
# Info.plist is the one source of the bundle identifier; input mode IDs derive from it
bundle_id=$(plutil -extract CFBundleIdentifier raw "$here/Info.plist")
compile() {
  "$cxx" -std=c++20 -fobjc-arc -fobjc-exceptions -isysroot "$sdk" \
    -mmacosx-version-min=13.0 -g -Wall -Wextra -Wpedantic -Werror \
    -Wno-gnu-zero-variadic-macro-arguments -DKR_BUNDLE_IDENTIFIER="\"$bundle_id\"" \
    -I"$root/include" "$@"
}

rust_lib() {
  if [ -n "${KEYRILL_RUST_LIB:-}" ]; then
    lib=$KEYRILL_RUST_LIB
  elif [ "$1" = release ]; then
    (cd "$root" && cargo build --locked --release -p keyrill-ffi)
    lib=$target/release/libkeyrill_ffi.a
  else
    (cd "$root" && cargo build --locked -p keyrill-ffi)
    lib=$target/debug/libkeyrill_ffi.a
  fi
  if [ ! -f "$lib" ]; then
    echo "build.sh: missing Rust library $lib" >&2
    exit 2
  fi
}

app() {
  compile -O2 "$here/src/KRSession.mm" "$here/src/KRKeys.mm" "$here/src/KRRuntime.mm" \
    "$here/src/KRCompatibility.mm" \
    "$here/src/KRController.mm" "$here/src/KRPreferences.mm" \
    "$here/src/main.mm" "$lib" -framework Cocoa -framework Carbon -framework InputMethodKit \
    -o "$out/KeyrillAlpha"
}

icons() {
  compile "$@" "$here/tools/input_icons.mm" -framework Cocoa -o "$out/input_icons"
  "$out/input_icons" --check "$here/resources/icons" "$here/Info.plist"
}

tests() {
  compile "$@" "$here/src/KRSession.mm" "$here/src/KRKeys.mm" "$here/src/KRRuntime.mm" \
    "$here/src/KRCompatibility.mm" "$here/src/KRController.mm" \
    "$here/tests/session_tests.mm" "$lib" -framework Cocoa -framework Carbon \
    -framework InputMethodKit -o "$out/session_tests"
  "$out/session_tests"
  compile "$@" "$here/src/KRSession.mm" "$here/src/KRRuntime.mm" \
    "$here/tests/runtime_tests.mm" "$lib" -framework Cocoa -framework Carbon \
    -framework InputMethodKit -o "$out/runtime_tests"
  "$out/runtime_tests"
  compile "$@" "$here/src/KRPreferences.mm" "$here/tests/preferences_tests.mm" \
    -framework Cocoa -o "$out/preferences_tests"
  "$out/preferences_tests"
}

case "${1:-}" in
check)
  "$clang_format" --dry-run --Werror "$here"/src/*.h "$here"/src/*.mm "$here"/tests/*.mm "$here"/tools/*.mm
  plutil -lint "$here/Info.plist" >/dev/null
  plutil -lint "$here/resources/en.lproj/InfoPlist.strings" >/dev/null
  rust_lib release
  mkdir -p "$out"
  icons -O2
  for source in "$here"/src/*.mm "$here"/tools/*.mm; do
    compile --analyze -Xanalyzer -analyzer-werror "$source" \
      -o "$out/$(basename "$source" .mm).analysis.plist"
  done
  app
  compile -O2 "$here/tools/source_status.mm" -framework Foundation -framework Carbon \
    -o "$out/source_status"
  compile -O2 "$here/tools/source_enable.mm" -framework Foundation -framework Carbon \
    -o "$out/source_enable"
  tests -O1
  ;;
sanitize)
  rust_lib debug
  mkdir -p "$out"
  icons -O1 -fsanitize=address,undefined -fno-sanitize-recover=undefined -fno-omit-frame-pointer
  tests -O1 -fsanitize=address,undefined -fno-sanitize-recover=undefined -fno-omit-frame-pointer
  ;;
bundle)
  rust_lib release
  mkdir -p "$out"
  icons -O2
  app
  rm -rf "$out/KeyrillAlpha.app"
  contents="$out/KeyrillAlpha.app/Contents"
  mkdir -p "$contents/MacOS"
  cp "$here/Info.plist" "$contents/Info.plist"
  cp "$out/KeyrillAlpha" "$contents/MacOS/KeyrillAlpha"
  mkdir -p "$contents/Resources/en.lproj"
  cp "$here/resources/en.lproj/InfoPlist.strings" "$contents/Resources/en.lproj/"
  cp "$here"/resources/icons/*.tif "$contents/Resources/"
  cp "$root/assets/model-notice.txt" "$root/assets/wordfreq-code-license.txt" \
    "$root/assets/model-provenance.json" "$contents/Resources/"
  cp "$root/LICENSE" "$contents/Resources/Keyrill-LICENSE.txt"
  "$out/input_icons" --check "$contents/Resources" "$contents/Info.plist"
  echo "$out/KeyrillAlpha.app"
  ;;
status)
  mkdir -p "$out"
  compile -O2 "$here/tools/source_status.mm" -framework Foundation -framework Carbon \
    -o "$out/source_status"
  "$out/source_status"
  ;;
source-plan)
  mkdir -p "$out"
  compile -O2 "$here/tools/source_enable.mm" -framework Foundation -framework Carbon \
    -o "$out/source_enable"
  "$out/source_enable" --plan
  ;;
*)
  echo "usage: build.sh check|sanitize|bundle|status|source-plan" >&2
  exit 2
  ;;
esac
