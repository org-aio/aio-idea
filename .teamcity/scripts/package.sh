#!/usr/bin/env bash
# 打包发布物：二进制 + web + aio.toml + 清单。不生成镜像。
set -euo pipefail

export PATH="${TEAMCITY_TOOLCHAIN:?缺少 TEAMCITY_TOOLCHAIN}/bin:$PATH"
readonly target="${TEAMCITY_TARGET:?缺少 TEAMCITY_TARGET}"
readonly target_dir="${target%%.*}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
readonly revision="${BUILD_VCS_NUMBER:?缺少 BUILD_VCS_NUMBER（TeamCity 的 40 位 Git SHA）}"

echo "== 整理 Web 资产"
npm ci --prefix deploy --ignore-scripts --no-audit --no-fund
node deploy/prepare-web.cjs "$CARGO_TARGET_DIR/dx/aio-idea/release/web/public"

echo "== 组装发布目录"
rm -rf dist
mkdir -p dist/release
cp "$CARGO_TARGET_DIR/$target_dir/release/aio-idea" dist/release/aio-idea
cp "$CARGO_TARGET_DIR/$target_dir/release/aio-launcher" dist/release/aio-launcher
cp -R "$CARGO_TARGET_DIR/dx/aio-idea/release/web/public" dist/release/web
cp aio.toml dist/release/aio.toml
node deploy/release-manifest.cjs dist/release "$revision"

echo "== 打包"
tar -C dist -czf dist/release.tar.gz release
ls -lh dist/release.tar.gz
cat dist/release/release-manifest.json
