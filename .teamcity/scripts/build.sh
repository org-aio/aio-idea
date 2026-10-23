#!/usr/bin/env bash
# 验证与构建：测试 -> 检查 -> 服务端/启动器 -> Web 资产。
set -euo pipefail

: "${TEAMCITY_TOOLCHAIN:?缺少 TEAMCITY_TOOLCHAIN}"
: "${TEAMCITY_TARGET:?缺少 TEAMCITY_TARGET}"

export PATH="$TEAMCITY_TOOLCHAIN/bin:$PATH"
export RUSTUP_HOME="$TEAMCITY_TOOLCHAIN/rustup"
export CARGO_HOME="$TEAMCITY_TOOLCHAIN/cargo"
export CARGO_NET_GIT_FETCH_WITH_CLI=true
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
export CARGO_INCREMENTAL=0

readonly target="$TEAMCITY_TARGET"
readonly target_dir="${target%%.*}"

echo "== 服务端测试"
cargo test --no-default-features --features server

echo "== Web 客户端检查"
cargo check --no-default-features --features web

echo "== 服务端（glibc 2.17）"
cargo zigbuild --release --target "$target" --no-default-features --features server

echo "== 固定启动器"
cargo zigbuild --release --target "$target" --no-default-features --features server --bin aio-launcher

echo "== Web 资产"
cargo clean -p az-ui-components --profile wasm-release --target wasm32-unknown-unknown \
    --config 'profile.wasm-release.inherits="release"' || true
rm -rf "$CARGO_TARGET_DIR/dx/aio-idea/release/web/public"
dx build --platform web --release --debug-symbols false

echo "== 构建完成：$CARGO_TARGET_DIR/$target_dir/release"
ls -l "$CARGO_TARGET_DIR/$target_dir/release/aio-idea" "$CARGO_TARGET_DIR/$target_dir/release/aio-launcher"
