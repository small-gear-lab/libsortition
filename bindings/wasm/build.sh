#!/bin/sh
# core を wasm32-unknown-unknown でビルドして bindings/wasm/sortition.wasm に置く
# 前提: rustup target add wasm32-unknown-unknown（乱数は .cargo/config.toml の custom backend 経由で JS に委ねる）
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
cargo build --release --target wasm32-unknown-unknown -p sortition --manifest-path "$root/Cargo.toml"
cp "$root/target/wasm32-unknown-unknown/release/sortition.wasm" "$root/bindings/wasm/sortition.wasm"
ls -l "$root/bindings/wasm/sortition.wasm"
