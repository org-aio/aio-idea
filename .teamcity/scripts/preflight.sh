#!/usr/bin/env bash
# 预检：工具链、子模块与仓库状态。任何缺失都立即失败，绝不静默回退。
set -euo pipefail

: "${TEAMCITY_TOOLCHAIN:?缺少 TEAMCITY_TOOLCHAIN}"
: "${TEAMCITY_TARGET:?缺少 TEAMCITY_TARGET}"

die() { echo "预检失败: $*" >&2; exit 1; }

echo "== 工具链：$TEAMCITY_TOOLCHAIN"
export PATH="$TEAMCITY_TOOLCHAIN/bin:$PATH"
export RUSTUP_HOME="$TEAMCITY_TOOLCHAIN/rustup"
export CARGO_HOME="$TEAMCITY_TOOLCHAIN/cargo"
export CARGO_NET_GIT_FETCH_WITH_CLI=true

for tool in cargo rustc cargo-zigbuild dx zig git node npm; do
    command -v "$tool" >/dev/null || die "找不到 $tool"
done
cargo --version
rustc --version
dx --version
zig version
node --version

echo "== 目标三元组：$TEAMCITY_TARGET"
rustc --print target-list | grep -qx "${TEAMCITY_TARGET%%.*}" \
    || die "rustc 不认识目标 ${TEAMCITY_TARGET}"

echo "== 子模块"
test -f .gitmodules || die "缺少 .gitmodules"
git submodule update --init --recursive
for path in $(git config --file .gitmodules --get-regexp path | awk '{print $2}'); do
    test -e "$path/.git" || die "子模块未检出: $path"
done

echo "== 发布物约束"
test -f aio.toml || die "缺少 aio.toml"
test -f deploy/prepare-web.cjs || die "缺少 deploy/prepare-web.cjs"
test -f deploy/release-manifest.cjs || die "缺少 deploy/release-manifest.cjs"

echo "== 禁止在部署脚本里出现退役主机"
if grep -rn '192\.168\.31\.252' --include='*.sh' --include='*.yaml' --include='*.yml' --include='*.zsh' . ; then
    die "发现已退役主机地址 192.168.31.252"
fi

echo "预检通过"
