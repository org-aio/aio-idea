# 宿主界面重构验证记录

日期：2026-09-14。环境：macOS、本地 Chrome、发布模式 Web 构建、隔离 PostgreSQL。本文记录本地证据，不代表正式站点已部署。

## 代码归属与提交状态

| 仓库 | 改动 | 已推送提交 |
| --- | --- | --- |
| dioxus-admin-workbench | 无 UI 依赖的外观模型、主题与密度、共享设置与空状态、导航路径、主题对比度 | `cc676107118a3fa83ac891dd97e35279ab9bebbd`，`codex/admin-console` |
| aio-plugin-settings | 账户与工作区 / 外观 / 关于、技术详情折叠、删除市场源管理 | `9bcedcca63e27ac0c4f198b51c1f5f80f3cd329a`，`codex/ui-refactor` |
| aio-plugin-marketplace | 全部 / 已安装、中文发布状态、管理操作、空状态、刷新保留内容 | `ed60101f7de32419dbb9df244ec932937ce9e07f`，`codex/scene-root-navigation` |
| aio-plugin-identity | 登录与个人资料接入共享工作台视觉 | `8acbefd58423bb01c065b3477dc06305a11df62d`，`codex/registration` |
| aio-platform | 删除第三方市场索引 CLI、索引目录和生成 workflow，更新中文开发指南 | `de7bfb9f7acf74a1cfdd79435137f9d8eafa0bbe`，`main` |

宿主运行时已在工作区抽取到 `aio-platform/lib/plugin/host`，该抽取与 `aio plugin dev` 仍有未提交改动。本次在新模块内完成官方市场查询、旧源迁移、接口移除、外观作用域与测试接入，并保留并行代码。

`aio-idea` 本地已锁定上述插件提交并完成构建。身份客户端升级，身份服务端保持已有锁定版本，避免和其他系统插件的 Dill 服务类型重复。共享宿主仍使用抽取任务的本地路径依赖：待其独立提交、推送后，再锁定宿主 Git 提交并提交产品接入；当前不能仅靠上表提交重现完整宿主改动。

## 已通过的检查

| 检查 | 结果与范围 |
| --- | --- |
| 共享工作台 Rust 测试 | 47 项通过：无头模型 9、应用壳 17、共享组件 21 |
| 共享宿主 Rust 测试 | 56 项通过；5 项需要数据库等额外环境，单独执行相关集成检查如下 |
| CLI 测试 | 34 项单元测试、2 项集成测试通过；初始化、打包、发布保留 |
| 市场层级模型 | 3 项测试通过，包括父插件搜索、缺失父节点和循环边界 |
| 编译 | 设置、市场、身份客户端独立 Wasm 检查及宿主发布模式 Web 构建通过；插件依赖取自已推送的 Git 提交 |
| 宿主页面 | 390、768、1440px 导航、父级路径、抽屉关闭、账户页返回、主题、密度、系统主题变化、不同用户偏好隔离通过，无页面横向溢出或控制台异常 |
| 市场交互 | 搜索、已安装筛选、版本详情、日志折叠、安装、启用、停用、卸载取消通过；15 秒后台刷新保留阅读位置 |
| 页面状态 | 登录、骨架屏、空市场、无匹配结果、失败重试和无权限均检查实际截图；市场源请求计数为 0 |
| 真实后台浏览器流程 | 真实身份与 PostgreSQL 下，账户、资料、工作区创建/重命名/切换、密码修改、旧会话撤销、文件上传下载/删除、字典和 RBAC 通过；对话框焦点、200% 缩放、浅深色对比度检查通过 |
| 真实发布 HTTP 流程 | CLI 打包/发布/下载、首次安装、失败保留、多租户升级、停用/卸载、回退与新提交、过期构建拒绝、无变化轮询通过 |
| 真实前端 HTTP 流程 | 已验证资产挂载、缓存复用、权限和票据撤销通过；删除的 `/api/runtime/registries` 返回 404 |
| PostgreSQL 迁移 | 在独立临时实例中创建旧源与安装数据夹具，验证先归档、事务回滚、重复执行、保留官方条目/安装绑定/业务哨兵数据；未使用生产数据库副本 |

## 插件缓存

使用真实 Compose 构建产物验证桌面与移动端切换。热返回分别为 **28.83ms / 30.56ms**，返回时 iframe 新挂载、卸载和资产请求均为 **0**；页面状态、工作区返回、登录隔离、版本失效、LRU 淘汰和撤权检查通过。

这证明缓存链路继续生效。当前记录没有相同环境下旧版与新版首次打开的成对耗时，不能据此断言正式网络环境首开性能不变；正式发布前补做首开对照。

## 复现入口

以下命令在已完成宿主抽取的工作区执行。浏览器用例需要本机 Chrome，以及 Node 可解析的 `playwright`；缓存用例还需要 `parse5` 和 `pngjs`。

```sh
cd aio-idea
dx build --platform web --release --debug-symbols false
node tests/browser/workbench.cjs
node tests/browser/marketplace.cjs
AIO_TEST_KMP_FRONTEND=../aio-plugin-kmp-example/dist/frontend node tests/browser/keepalive.cjs
```

真实后台测试使用独立 PostgreSQL，`AIO_TEST_DATABASE_URL` 与 `AIO_DATABASE_URL` 必须指向同一测试库；设置测试启动账号和密码，禁止将这些用例指向正式业务库。发布测试额外设置 `AIO_TEST_CLI`、`AIO_PLUGIN_PUBLISH_ACCOUNTS` 和测试 `AIO_DELIVERY_TOKEN`。

```sh
cargo test --no-default-features --features server system_management_browser_workflows -- --ignored --nocapture
cargo test --no-default-features --features server binary_cli_publishes_downloads_recovers_and_rolls_back_over_http -- --ignored --nocapture
cargo test --no-default-features --features server frontend_http_mounts_verified_assets_and_revokes_live_access -- --ignored --nocapture

cd ../aio-platform
cargo test -p az-plugin-host --features server archives_sources_and_preserves_official_packages_and_bindings -- --ignored --nocapture
```

## 正式发布前剩余项

1. 衔接运行时抽取提交，固定共享宿主版本并推送产品集成及测试修改，确保独立检出可构建。
2. 对生产数据库完整备份，在恢复出的副本执行迁移，检查包、发布历史、安装绑定和实际插件业务数据；当前隔离夹具演练不能替代这一步。
3. 使用正式账号检查官方自动发现、构建和上架链路，记录 390/768/1440px 的正式服务截图及新旧版本首开/热返回耗时，再正式发布。
