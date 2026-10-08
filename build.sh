#!/usr/bin/env bash
# MergeSort Pro — Linux/macOS build (cross-platform rilis).
# Usage: ./build.sh            (native release)
#        ./build.sh linux       (x86_64-unknown-linux-gnu, needs target installed)
#        ./build.sh mac-intel   (x86_64-apple-darwin, on macOS)
#        ./build.sh mac-arm     (aarch64-apple-darwin, on macOS)
set -euo pipefail
if ! command -v rustup >/dev/null 2>&1 && ! command -v cargo >/dev/null 2>&1; then
  echo "error: rust toolchain not found (install from https://rustup.rs)" >&2
  exit 2
fi
TARGET="${1:-}"
if [ -n "$TARGET" ]; then
  case "$TARGET" in
    linux)   T=x86_64-unknown-linux-gnu ;;
    mac-intel) T=x86_64-apple-darwin ;;
    mac-arm) T=aarch64-apple-darwin ;;
    *) echo "target: linux|mac-intel|mac-arm"; exit 2 ;;
  esac
  if command -v rustup >/dev/null 2>&1; then
    rustup target add "$T"
  fi
  cargo build --locked --release --target "$T"
  cargo build --locked --release --target "$T" --bin gen
  echo "OK: target/$T/release/mergesort"
  "target/$T/release/mergesort" --version
else
  cargo build --locked --release
  cargo build --locked --release --bin gen
  echo "OK: target/release/mergesort"
  ./target/release/mergesort --version
fi
