# AIO IDEA 部署流水线

TeamCity Versioned Settings 读取本目录的 `settings.kts`，`main` 推送时执行四步：

1. **预检**：校验工具链（`$TEAMCITY_TOOLCHAIN`）、子模块、发布物约束，并拒绝退役主机地址；
2. **验证与构建**：`cargo test --features server` → `cargo check --features web` →
   `cargo zigbuild`（glibc 2.17）服务端与启动器 → `dx build` Web 资产；
3. **打包**：`prepare-web.cjs` 整理资产，`release-manifest.cjs` 生成可信清单；
4. **发布**：上传到 `$TEAMCITY_DEPLOY_ROOT/data/releases/<SHA>`，原子切换 `.env` 的
   `AIO_RELEASE`，只 `docker compose up -d --no-deps supervisor aio-idea` 重建应用容器，
   健康检查失败自动回滚。

## 不构建镜像

部署机不在流水线里构建镜像。运行时统一使用已发布的
`mcr.microsoft.com/playwright:v1.62.0-noble`：Web 宿主需要 Chromium 依赖，
自己维护 Dockerfile 只会多一套需要同步的副本。发布物是纯二进制 + 静态资产。

## 运行形态

- `aio-idea-postgres`：独立 PostgreSQL 16，数据在 `data/` 之外的 `postgres/data/`，
  只在本机回环暴露（`$POSTGRES_PORT`）；发布流程不重建它。
- `aio-idea-supervisor`：唯一持有 Docker socket 的容器，root；
- `aio-idea`：以旧部署相同的数字 UID/GID 运行，不持有 Docker socket；
- 宿主只监听 `127.0.0.1:$AIO_WEB_PORT` 与 `127.0.0.1:$AIO_DEVICE_TUNNEL_PORT`，
  内网可达性由 localmesh Caddy 的 `bind` + `remote_ip` 决定。

## 必需的外部条件

- agent `okm182` 有 `/opt/aio-idea-toolchain`（cargo/rustc/cargo-zigbuild/dx/zig）的执行权限；
- 部署根目录 `/root/DockerCompose/aio-idea/` 下有 `.env`、`runtime.env`、`process.env`（均 0600）；
- agent 用户能执行 `docker compose`（已在 `docker` 组）并能写 `data/releases/`；
- localmesh Caddy 已渲染内网站点，`dnsmasq` 已把域名指向入口机。

## 参数

| 参数 | 含义 |
|---|---|
| `env.TEAMCITY_DEPLOY_HOST` | 目标主机名（文档与校验用，唯一默认值） |
| `env.TEAMCITY_DEPLOY_ROOT` | 部署根目录 |
| `env.TEAMCITY_PUBLIC_HOST` | 站点域名 |
| `env.TEAMCITY_TOOLCHAIN` | 工具链根目录 |
| `env.TEAMCITY_TARGET` | 目标三元组 |
| `env.TEAMCITY_KEEP_RELEASES` | 保留的发布数 |

## 安全边界

- 流水线不读、不写数据库；迁移由应用自身的 `runtime migrate` 显式执行；
- 不覆盖 `.env` / `runtime.env` / `process.env` 中的凭据，只改 `AIO_RELEASE`；
- 任何构建或健康检查失败都不会留下半切换状态：`.env` 回滚后重新拉起上一发布。
