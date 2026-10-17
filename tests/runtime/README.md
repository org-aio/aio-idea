# 宿主启动恢复验收

`startup_recovery.py` 使用真实服务二进制、HTTP 登录和独立 PostgreSQL，验证零语言首次启动、增强停用后强杀及单语言恢复、在线拒绝、首次资源初始化失败、显式重试与恢复、锁冲突、来源缺失、端口冲突、写盘失败和损坏记录。增强恢复核对实际调用、版本摘要与代数；初始化失败使用不可作为目录的文件存储路径，下一进程仍恢复已停用文件服务的组合。每次成功启动都确认已停用的文件、权限、租户和市场路由保持 404，根路径字典 API 也不存在。全部子进程使用临时缓存及文件目录；清理只终止脚本自己创建的进程。输出日志目录用于检查失败现场。

先准备一个本机独立测试库 `identity_recovery_test`，撤销该库 `public` schema 的公共权限；不能指向现役业务库。使用 Python 3 标准库，无需额外包：

```sh
cargo build --locked --no-default-features --features server
export AIO_RECOVERY_TEST_BINARY="$PWD/target/debug/aio-idea"
export AIO_RECOVERY_TEST_DATABASE_URL='postgresql://postgres@127.0.0.1:5432/identity_recovery_test'
python3 tests/runtime/startup_recovery.py
```

凭据由本机测试环境提供。脚本拒绝非 `127.0.0.1` 或不同数据库名，首次使用空库；重复执行前由测试环境重建专用库。测试会生成随机临时账户密码，不输出或写入恢复记录。完成后删除专用数据库或任务容器。该验收证明 Linux 进程崩溃恢复，不代表操作系统掉电、macOS/Windows 或生产部署验收。

## 在线系统服务

`src/host/runtime_composition_tests.rs` 与 `system_service_tests.rs` 使用本机独立库 `identity_plugin_test`。令 `AIO_TEST_DATABASE_URL` 与 `AIO_DATABASE_URL` 指向同一测试库，提供测试专用 `AIO_BOOTSTRAP_ACCOUNT`、`AIO_BOOTSTRAP_PASSWORD`，将 `AIO_FILE_STORAGE_DIR`、`AIO_PROCESS_ROOT` 和 `AIO_PLUGIN_SUPERVISOR_SOCKET` 指向任务临时目录，再执行：

```sh
cargo test --locked --no-default-features --features server host:: -- --include-ignored --test-threads=1
```

测试通过真实 HTTP 创建文件，停用四项可选系统服务后验证 API 为 404、旧 Service 引用返回 `PoolClosed`；重新启用创建新实例并保留数据。身份重启会连带重建系统服务，持久化登录会话继续可用；MCP 检查工具目录结果，gRPC 使用正式客户端验证最终协议状态。身份专用测试另外覆盖部分启动失败、失败替换后恢复和资源最终关闭；配置测试覆盖旧字典字段迁移、保留其他停用状态以及候选未启动时不提前确认。测试后删除专用数据库和任务临时目录。

`src/host/script_service_tests.rs` 在同一产品宿主中发布两种语言的增强，保持 MCP HTTP 客户端与 gRPC 连接，在线移除全部语言及脚本服务。原生设备能力在显式授权后继续执行，增强入口消失，身份与数据库单例保留；分别恢复一种语言后核对版本摘要、激活代数和调用结果。只恢复与活动包不匹配的语言时，增强目录为空、调用被拒绝，原生目录及调用仍可用。测试还覆盖停用语言无法编译候选、语言重启会重建脚本服务、演进控制开关保留以及原生能力撤权。

字典业务与浏览器验收随独立插件维护，见 [整包测试](https://github.com/org-aio/aio-plugin-dictionary/blob/main/tests/README.md)。它使用真实产品、Docker Sidecar 和 PostgreSQL 验证不重编宿主的安装、升级、回滚、停用、重启、实时撤权、数据复制及桌面/手机操作；产品不再保留第二套静态字典测试。

## 执行器暂停与进程恢复

`executor_service_tests.rs` 在产品组合测试中覆盖 Wasm/Sidecar 的两代接口启停与重建，并使用真实组件存储。`executor_recovery.py` 使用产品二进制、平台 WIT 生命周期夹具、正式字典 Sidecar 包与真实 Docker 监督器，验证零执行器首次启动、同时暂停、强杀后恢复、仅恢复一种执行器、按类型重启、暂停时停用/卸载，以及原摘要、代数、回滚排除记录和 Sidecar 数据保留。MCP 与 HTTP 能力目录在暂停期间持续可用；正式 gRPC 客户端由 Rust 组合测试覆盖。

准备新的本机独立库 `executor_recovery_test`，撤销 `public` schema 的公共权限，并设置：

```sh
export AIO_EXECUTOR_TEST_BINARY=/path/to/aio-idea
export AIO_EXECUTOR_TEST_DATABASE_URL='postgresql://postgres@127.0.0.1:5432/executor_recovery_test'
export AIO_EXECUTOR_TEST_POSTGRES_CONTAINER=your-isolated-postgres
export AIO_EXECUTOR_TEST_PACKAGER=/path/to/aio-platform/target/debug/examples/package
export AIO_EXECUTOR_TEST_WASM_COMPONENT=/path/to/release_fixture.wasm
export AIO_EXECUTOR_TEST_SIDECAR_PACKAGE=/path/to/dictionary.aio-plugin
export AIO_EXECUTOR_TEST_PROCESS_IMAGES=sha256:your-installed-sidecar-image
python3 tests/runtime/executor_recovery.py
```

Wasm 夹具与整包工具分别由平台 `lib/plugin/runtime/tests/fixture/README.md` 和 `lib/plugin/bundle/examples/README.md` 的命令构建；Sidecar 包按字典仓库的整包验收说明构建。镜像摘要必须与包清单一致且已在本机存在。脚本只检查自己进程目录对应的容器，结束时停止自己启动的宿主和监督器；验收后删除测试数据库及输出的临时目录。它不修改现役宿主或生产安装记录。

2026-10-17 在 Linux 隔离环境完成：8 项产品宿主测试、`startup_recovery.py`、`executor_recovery.py` 及 server 全目标 Clippy 均通过。真实恢复测试核对完整安装记录、Wasm 调用结果和 Sidecar 字典数据，确认暂停后旧票据失效、恢复后按原版本继续执行、退出后无本次容器残留；整个流程未重编或替换宿主二进制。浏览器市场交互由 `tests/browser/marketplace.cjs` 独立验收，不能据此宣称生产环境已部署。
