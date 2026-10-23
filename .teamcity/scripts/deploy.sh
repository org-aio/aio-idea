#!/usr/bin/env bash
# 发布：上传发布物到部署根目录，原子切换 .env 的 AIO_RELEASE，只重建应用容器。
# PostgreSQL 与数据卷不参与切换，避免每次发布都碰数据库。
set -euo pipefail

: "${TEAMCITY_DEPLOY_ROOT:?缺少 TEAMCITY_DEPLOY_ROOT}"
: "${TEAMCITY_PUBLIC_HOST:?缺少 TEAMCITY_PUBLIC_HOST}"
readonly repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly deploy_root="$TEAMCITY_DEPLOY_ROOT"
readonly revision="${BUILD_VCS_NUMBER:?缺少 BUILD_VCS_NUMBER}"
readonly host_uid="${DEPLOY_APP_UID:-984}"
readonly host_gid="${DEPLOY_APP_GID:-977}"
readonly keep="${TEAMCITY_KEEP_RELEASES:-5}"

[[ "$revision" =~ ^[0-9a-f]{40}$ ]] || { echo "发布版本必须是完整 40 位 Git SHA" >&2; exit 64; }
test -f "$repository/dist/release.tar.gz" || { echo "缺少 dist/release.tar.gz" >&2; exit 66; }
test -d "$deploy_root" || { echo "部署根目录不存在: $deploy_root" >&2; exit 66; }
test -f "$deploy_root/.env" || { echo "缺 $deploy_root/.env" >&2; exit 66; }

readonly incoming="$deploy_root/data/releases/.incoming-$revision"
readonly release="$deploy_root/data/releases/$revision"

echo "== 解包到候选目录"
rm -rf "$incoming"
mkdir -p "$incoming"
tar -C "$incoming" --strip-components=1 -xzf "$repository/dist/release.tar.gz"

echo "== 校验候选发布物"
test -x "$incoming/aio-idea"
test -x "$incoming/aio-launcher"
test -f "$incoming/aio.toml"
test -f "$incoming/web/index.html"
test -f "$incoming/release-manifest.json"
python3 - "$incoming/release-manifest.json" "$revision" <<'PY'
import hashlib, json, pathlib, sys
manifest_path, revision = sys.argv[1], sys.argv[2]
manifest = json.loads(pathlib.Path(manifest_path).read_text())
if manifest.get('revision') != revision:
    raise SystemExit(f"清单 revision 不匹配: {manifest.get('revision')} != {revision}")
root = pathlib.Path(manifest_path).parent
for key, value in (manifest.get('extra_files') or {}).items():
    digest = hashlib.sha256((root / key).read_bytes()).hexdigest()
    if digest != value:
        raise SystemExit(f"{key} 摘要不匹配")
digest = hashlib.sha256((root / manifest['host_binary']).read_bytes()).hexdigest()
if digest != manifest['host_sha256']:
    raise SystemExit("宿主二进制摘要不匹配")
print("清单校验通过")
PY

chown -R "$host_uid:$host_gid" "$incoming"
chmod -R u=rwX,g=rX,o= "$incoming"
mv "$incoming" "$release"

echo "== 同步编排文件"
if ! cmp -s "$repository/compose.yaml" "$deploy_root/compose.yaml"; then
    cp -p "$deploy_root/compose.yaml" "$deploy_root/compose.yaml.previous" 2>/dev/null || true
    install -m 0644 "$repository/compose.yaml" "$deploy_root/compose.yaml"
    echo "已更新 compose.yaml"
else
    echo "compose.yaml 无变化"
fi

echo "== 切换 AIO_RELEASE"
cp -p "$deploy_root/.env" "$deploy_root/.env.previous"
python3 - "$deploy_root/.env" "$revision" <<'PY'
import pathlib, sys
path, revision = pathlib.Path(sys.argv[1]), sys.argv[2]
lines = path.read_text().splitlines()
out, found = [], False
for line in lines:
    if line.startswith('AIO_RELEASE='):
        out.append(f'AIO_RELEASE={revision}')
        found = True
    else:
        out.append(line)
if not found:
    out.append(f'AIO_RELEASE={revision}')
path.write_text('\n'.join(out) + '\n')
path.chmod(0o600)
print(f'AIO_RELEASE={revision}')
PY

restart_app() {
    cd "$deploy_root"
    docker compose up -d --no-deps --force-recreate supervisor aio-idea
}

wait_for_health() {
    local deadline=$((SECONDS + 300))
    while ((SECONDS < deadline)); do
        if curl -fsS --connect-timeout 1 --max-time 2 http://127.0.0.1:3080/health >/dev/null 2>&1; then
            return 0
        fi
        sleep 2
    done
    return 1
}

echo "== 重启应用容器（数据库不动）"
restart_app

if wait_for_health; then
    echo "== 应用健康"
else
    echo "!! 健康检查失败，回滚到上一发布" >&2
    mv "$deploy_root/.env.previous" "$deploy_root/.env"
    chmod 600 "$deploy_root/.env"
    if [ -f "$deploy_root/compose.yaml.previous" ]; then
        mv "$deploy_root/compose.yaml.previous" "$deploy_root/compose.yaml"
    fi
    restart_app || true
    exit 1
fi

echo "== 校验站点入口"
curl -kfsS --resolve "$TEAMCITY_PUBLIC_HOST:443:127.0.0.1" --max-time 10 "https://$TEAMCITY_PUBLIC_HOST/health" >/dev/null \
    || echo "警告: 本机回环未命中原站（内网站点绑定在 LAN 地址，属预期）"

echo "== 清理旧发布（保留最近 $keep 个）"
python3 - "$deploy_root/data/releases" "$keep" <<'PY'
import pathlib, shutil, sys
root, keep = pathlib.Path(sys.argv[1]), int(sys.argv[2])
releases = sorted(
    (p for p in root.iterdir() if p.is_dir() and not p.name.startswith('.')),
    key=lambda p: p.stat().st_mtime,
    reverse=True,
)
for old in releases[keep:]:
    shutil.rmtree(old, ignore_errors=True)
    print(f'删除 {old.name}')
PY

rm -f "$deploy_root/.env.previous" "$deploy_root/compose.yaml.previous"
echo "发布完成: $revision"
