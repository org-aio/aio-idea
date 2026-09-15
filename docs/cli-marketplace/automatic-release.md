# CLI 自动发布验收

## 使用方式

新建 CLI：

```sh
npx -y @zjarlin/aio@2026.9.17 plugin init my-cli --kind cli
```

接入现有 npm CLI，在其目录执行：

```sh
npx -y @zjarlin/aio@2026.9.17 plugin init . --kind cli --adopt --name my-cli
```

现有 CLI 接入只生成 `aio-cli.json`、`AIO.md` 和 `.github/workflows/aio-cli.yml`，已有文件会被保护。推送到 GitHub 后执行一次 `aio tool release setup`，从 origin 确定发布仓库，创建 npm 包（如果尚不存在）并配置 Trusted Publisher。npm 的首次身份配置需要账号验证；之后由 GitHub Actions 的短期 OIDC 身份发布。

## 分发与更新

默认分支推送通过 Linux 和 Windows 测试后，生成下一补丁版本的开发版，先公开发布 npm `next`，再同步市场条目和 README。匹配 `package.json` 的 `vX.Y.Z` 标签发布 npm `latest`。市场显示最高 SemVer，安装固定版本；本机安装不会被推送悄悄替换。

平台从 npm 官方源读取确切版本和包摘要，并验证 GitHub JWT 的签名、有效期、audience、仓库 owner、工作流路径和源码提交。来源绑定后其他仓库不能覆盖同一工具 ID。同一版本内容不可修改，工作流重试可继续市场同步；较旧版本不覆盖新 README。发布接口只处理元数据，不在宿主运行仓库构建脚本。

## 本机验证

- CLI 初始化、现有项目接入保护及工具安装相关 Rust 测试通过。
- 新模板在独立临时目录完成 npm ci、构建、测试、开发版本生成、打包和包内命令版本检查。
- 发布辅助工具的 4 项 Node 测试通过，包含 npm 元数据延迟超过两分钟后自动完成同步的回归测试。
- 3 项宿主发布测试通过，包含真实隔离 PostgreSQL 的幂等、来源隔离和旧版本顺序验证。宿主回归 59 项通过、9 项默认跳过。
- codex-model-sync 原有测试 57 项通过，1 项跳过；npm 入口测试 7 项通过。
- GitHub CI 34952329181 的全仓测试、运行时发布集成测试、npm 入口、schema 一致性检查通过。原发布提交的 Clippy 步骤遇到既有告警；后续独立清理了多余借用、条件合并及测试模块位置，本机全仓 Clippy 和相关回归测试均已通过。

## 生产接口

宿主提交 `fb12e2517b6449f7c1a658e635ad0d5ecbadc743` 已部署，共享平台提交 `d9b52234621e0818545b052003d8413087969a13`。上线前完成宿主数据库、插件数据库与密钥备份。发布器服务端测试、Web 检查、glibc 2.17 构建、Web 构建和公网健康检查通过，运行进程与发布目录一致。

公开 `POST /api/runtime/tools/publish` 已上线；缺少 GitHub 发布身份的请求返回 401。codex-model-sync 的 npm Trusted Publisher 已绑定到其 `aio-cli.yml`，允许该工作流发布。

## npm 与本机 CLI

AIO 发布工作流 [34956303353](https://github.com/zjarlin/aio-platform/actions/runs/34956303353) 成功，公开 npm 发布 `@zjarlin/aio@2026.9.17` 及五个平台包，带 GitHub provenance。本机全局 AIO 已升级到 `2026.9.17`，实际执行 `aio --version` 通过，并用公开安装的 CLI 成功初始化含 TypeScript 入口、锁文件和固定 `2026.9.17` 自动发布工作流的临时项目。

平台后续 Clippy 清理提交为 `edd59b177e23448a060084644f54301e15aeb720`，GitHub CI [34953988870](https://github.com/zjarlin/aio-platform/actions/runs/34953988870) 全部通过；相关本机回归 122 项通过、9 项默认跳过。

## Windows 发布前回归

验证发现原有 Windows 工作流长期停在 Router 测试：桥接直接执行 JS 文件，且测试清理等待已发生的 exit 事件。现已让命令执行、桥接和透明转发共用 Node 入口解析；清理识别已退出进程并限定等待，hook 安装测试按现有 POSIX 支持边界断言。原有工作流 [34955427991](https://github.com/zjarlin/codex-model-sync/actions/runs/34955427991) 在 Windows 和 Linux 均通过，覆盖实际计划任务安装与卸载、打包和包内 CLI 运行。

## 实际自动发布

[CLI 发布工作流 34955428104](https://github.com/zjarlin/codex-model-sync/actions/runs/34955428104) 将源码 `ed2e5eb03fcb51baa119dbaeb58f7c9b64560f7a` 发布为 `codex-model-sync@0.4.2-dev.2.ged2e5eb03fcb`。npm 元数据首次传播超过两分钟，第一次市场同步超时；重跑同一次工作流后确认包摘要和源码完全一致，跳过重复 npm 发布，并成功同步市场。这个实际延迟促使 AIO 修正版把自动等待窗口扩大到约十分钟。

真实登录后的桌面和手机页面均验证市场返回该版本、README 可读、精确安装 URI 正确，无页面脚本错误和横向溢出。点击安装被测试拦截，只验证 URI，未触发本机安装或宿主插件安装。通过公开 npm 包执行 `--version` 也返回同一版本。

### 修正版一次自动完成

AIO `2026.9.17` 的 [完整 CI](https://github.com/zjarlin/aio-platform/actions/runs/34956274050) 和 [五平台 npm 发布](https://github.com/zjarlin/aio-platform/actions/runs/34956303353) 均通过。修正版把 npm 元数据等待窗口扩大到约十分钟，并增加延迟回归测试。新项目的发布工作流固定使用这一版本。

`codex-model-sync` 提交 `da1121712ffd886734af808dfb9d6416868bb039` 的 [CLI release](https://github.com/zjarlin/codex-model-sync/actions/runs/34958860751) 第一次运行即成功：Linux、Windows 测试通过，10:37:41 UTC 发布 npm，自动等待约 105 秒后于 10:39:27 UTC 完成市场同步，无手动重跑或再次账户验证。[原有 Check](https://github.com/zjarlin/codex-model-sync/actions/runs/34958860747) 同样通过。

公开 npm 与线上市场的版本均为 `0.4.2-dev.3.gda1121712ffd`，npm `next` 指向该版本，`latest` 保持 `0.4.1`。npm 元数据确认发布身份为 GitHub Actions Trusted Publisher，并记录相同仓库、提交和 SHA-512 摘要；公开 npx 执行 `--version` 返回该精确版本。

最终桌面与手机浏览器验证通过，证据见 [JSON 报告](automatic-report.json)、[桌面截图](automatic-desktop.png) 和 [手机截图](automatic-mobile.png)。验证读取真实线上版本、README 与安装 URI，未触发实际安装。
