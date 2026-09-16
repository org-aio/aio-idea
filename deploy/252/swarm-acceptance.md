# 工作区蜂群运行

宿主升级至 `az-plugin-host 2026.9.19`，源码提交 `598f6bab5ee54bbf0b0003174962771b55450336`。Agent 清单必须声明 `workspace.execute`；部署环境 `process.env` 的 `AIO_PROCESS_WORKER_CAPABILITIES` 也必须包含此能力。设备不因此自动扩大目录授权。

本机以已配对账号注册逻辑工作区：

```sh
aio-space workspace-add --name my-app --path /path/to/my-app
# 写入及运行本机预先登记的命令时才加 --execute、--manifest。
aio-space workspace-add --name my-app --path /path/to/my-app --execute --manifest /private/path/commands.json
```

命令清单存于工作区以外的 0600 私有文件；配置内容与命令通过本机 CLI 授予，不允许模型远程添加可执行程序、任意参数或环境变量。`describe` 返回逻辑 ID、可用操作和命令名。macOS/Linux 客户端是当前执行平台，Windows 原生能力仍待实现。

独立验收使用两份临时 Git 项目、真实文件、Node 进程和 HTTP 服务：Git 状态和文件 CAS/备份、两项目并发构建、测试中启动服务并请求 `/health`、非零退出码隔离、正常结束/超时/取消清理。客户端全套 37 项通过，新增工作区聚焦 10 项通过；隔离真实项目验收 9 项通过。宿主 PostgreSQL 3 项验证跨用户/租户、能力校验及撤权取消。Agent 持久任务、幂等派发、并发目标、取消与重启回归及 desktop/mobile 浏览器由插件仓库维护。

这些本地证据不等于多台物理设备或生产浏览器任务链路验收。生产部署与实际账号验证结果另附在该文件后续记录中。
