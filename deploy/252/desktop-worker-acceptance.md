# 网页到 macOS 应用启动验收

2026-09-16，在正式入口 https://aio.addzero.site 使用既有 zjarlin 账号验证。沿用既有设备配对，未新增账号或重新输入客户端密码。

## 实际结果

在 Chrome 的“智能体”页面新建“设备验收：打开 Postman”对话，实际输入并发送 `打开 Postman`。服务端直接识别完整应用启动指令，列出当前账号授权设备，选取唯一在线设备，通过宿主 broker 向客户端下发 `desktop.open-app`。

- 设备：`addzeroMacBook-Pro.local`，macOS，worker `0.3.1`。
- 任务：`f16bee39-b453-7a39-9960-94aba2f84b86`。
- 服务端持久状态：`complete`。
- 客户端结果：`{"application":"Postman","bundlePath":"/Applications/Postman.app","pid":49802,"running":true}`。
- `ps -p 49802 -o pid=,comm=` 返回 `/Applications/Postman.app/Contents/MacOS/Postman`。
- 网页实际回复：`已在 addzeroMacBook-Pro.local 打开 Postman，客户端已确认进程 PID 49802。`，`0 tokens`。
- 原生 Postman 窗口可读取，PID 同为 `49802`。Postman 验收前已在运行，本次验证唤起现有实例，未退出用户应用或丢弃窗口数据。

已配对 Mac 的后台服务在升级后仍为 `running`。配对凭据、会话 Cookie、模型密钥不进入验收文档。

## 交付版本

- 平台设备授权及 broker：`c84ce81dd461dcb31fc9268dff983b96e714d3d6`。
- 产品宿主应用控制发布：`56a78c9bf81035d21f772d8a44af899a35403984`。
- Agent：交付任务 `42` 已 `active`，当前用户工作区安装 `0.0.0-dev.42+549569761ea1effd55de4691eb637706c5fc48e0`。
- 本机 worker：`0.3.1`，修复了无关包外启动器导致应用发现中断的问题；通过本地 tarball 安装并更新后台服务。本记录不表示该版本已在 npm 发布。
- 252 交付服务已同步升级以识别清单 `worker_capabilities`，保留原二进制备份。

## 验证与边界

宿主常规测试 63 项通过；独立 PostgreSQL 配对、归档恢复及设备授权测试另行通过，包含跨账号和跨租户拒绝、未授权应用拒绝、重复请求去重、撤权取消任务与拒绝旧租约回报。Bundle/contract 17 项、交付服务 2 项测试通过；Web wasm 检查及正式宿主构建通过。

Agent backend 9 项与执行器 3 项测试通过，Clippy 通过。worker 12 项测试、TypeScript 检查及实际安装通过。包含应用匹配、路径/参数拒绝、同名歧义、未启动、包外启动器跳过、错误 JSON 不执行工具、排队不冒充完成、多个在线设备不自动猜选等场景。

当前能力是打开 macOS 已安装应用。它不提供任意 shell、鼠标键盘或完整远程桌面控制；其他系统未接入应用启动。应用控制由设备所属账号在“我的设备”开启，关闭后取消排队或运行中的应用任务。完整的 `打开 应用名` 指令由确定流程执行并根据真实回报生成回复；多台设备在线时要求明确目标。
