# 智能体自动交付验收

日期：2026-09-16。正式入口：<https://aio.addzero.site>。

## 结果

智能体仓库已加入 `aio-delivery.toml`。默认分支推送后，服务器发现完整 SHA，在固定 Fullstack 镜像中构建 Compose 前端和 glibc 2.17 Linux 后端，上传原生 v2 包，通过实际运行验证后发布，再更新已有启用安装。无需人工上传插件包，也不依赖开发机持续在线。

本次第 28 号任务和租户升级记录均为 `active`，错误为空；原市场版本 `99774f7b5a4fccb82182c69a6596048291258a7c7eab703b129dac0130905234` 已替换。公网发布目录、市场记录、租户安装记录与实际 process 进程指向同一新版本。原有提供方、会话、消息数量在切换前后一致。

| 项目 | 已验证版本 |
| --- | --- |
| 宿主 release | `3fa943f496edc426e8f10520575aa7fa14b3d346` |
| 宿主平台依赖 | `c0252a5626fb6d00210d416b3782126849fca136` |
| 构建工作进程源码 | `4f51de8` |
| Agent 来源提交 | `e13793ea9f614758eb1dd9d4305058af47896929` |
| 原生包摘要 | `d593777994999e9330e25695ca4803449f642bd35122277c8fc79e9b7e4e41df` |
| 实际后端 SHA-256 | `7906a177d2194661d9ee1c036626b4fb76733970b4ec7d4296dfed5c3092547d` |
| Fullstack 构建镜像 | `sha256:9d133f3cc79b024e1ae45a474f85e05cca48052ca93371dd536aec253d3907d3` |

运行容器健康接口返回 200，新增 `/providers/models` 与 `/conversations/{id}/model` 路由均存在，未带身份调用返回 401。此次上线包含 URL 手动输入、模型列表发现、名称自动生成和会话中切换模型的既有改动。

## 验证

- 8 项 Component 测试在独立 PostgreSQL 与真实 WIT 测试产物上串行通过，覆盖原生交付 HTTP、包来源绑定、实际运行验证失败保留旧版、过期任务拒绝、多租户升级、停用/卸载、回滚排除和重复轮询。
- 工作进程 2 项测试通过，覆盖 v2 Process 整包打包及 Rust/Kotlin 共用代理；交付清单测试与 Clippy 通过。
- 宿主发布器完成服务端测试、Web 检查、Linux release 与 Web release 构建，并验证 MainPID、当前 release、本机和公网健康检查。
- 第 27 号任务在 Java 未使用代理时无法下载依赖。工作进程统一配置 Java HTTP/HTTPS 代理后，新推送自动产生第 28 号任务并取代旧任务，完整构建、上传、发布和自动安装成功。
- 生产运行证据保存于本机 `target/component-delivery/native-automatic-delivery.json`；该文件不进入 Git。此次未重新执行生产浏览器交互测试，页面配置和模型切换的桌面/手机回归属于此前功能验证。

## 运维

构建服务使用 `aio-platform/delivery/configure.cjs` 配置四种固定镜像。私有图谱依赖按完整提交预置到该插件、该镜像专属缓存，使用 `delivery/seed-source.cjs` 导入，不将私有源码提交到公开插件仓库，构建容器不接收 GitHub 或发布凭据。更新依赖 SHA 或构建镜像时需重新预置对应缓存。

已停用、已卸载及未安装的租户不会被后台恢复或安装。手动回滚排除当前市场版本，后续新版本继续跟进。Process 更新保留失败恢复路径，但切换会停止旧实例，不承诺零中断。

部署前保存宿主数据库、Component 数据库和密钥备份；数据库备份 SHA-256 分别为 `e6342b3a9167ae3c59bd95b65b4daac536dc9d2f664d839ab092491a4958896f`、`0e6b65ae61f2a82a5b50db6e0bd56457bc1e6dc6ffb8faa06dc1b95bf6b3696e`。运行地址授权同步采用 Agent 清单已声明的私有模型网关。
