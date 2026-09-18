#!/usr/bin/env bash
# MergeSort Pro — Linux/macOS build (cross-platform rilis).
# Usage: ./build.sh            (native release)
#        ./build.sh linux       (x86_64-unknown-linux-gnu, needs target installed)
#        ./build.sh mac-intel   (x86_64-apple-darwin, on macOS)
#        ./build.sh mac-arm     (aarch64-apple-darwin, on macOS)
set -euo pipefail
TARGET="${1:-}"
if [ -n "$TARGET" ]; then
  case "$TARGET" in
    linux)   T=x86_64-unknown-linux-gnu ;;
    mac-intel) T=x86_64-apple-darwin ;;
    mac-arm) T=aarch64-apple-darwin ;;
    *) echo "target: linux|mac-intel|mac-arm"; exit 2 ;;
  esac
  rustup target add "$T"
  cargo build --release --target "$T"
  echo "OK: target/$T/release/mergesort"
else
  cargo build --release
  echo "OK: target/release/mergesort"
fi
./target/release/mergesort --version 2>/dev/null || ./target/*/release/mergesort --version || true
