# 工作区蜂群运行

宿主升级至 `az-plugin-host 2026.9.19`，源码提交 `abe91afce5a783bf1f099595f0d18845895bacc7`。Agent 清单必须声明 `workspace.execute`；部署环境 `process.env` 的 `AIO_PROCESS_WORKER_CAPABILITIES` 也必须包含此能力。设备不因此自动扩大目录授权。

本机以已配对账号注册逻辑工作区：

```sh
aio-space workspace-add --name my-app --path /path/to/my-app
# 写入及运行本机预先登记的命令时才加 --execute、--manifest。
aio-space workspace-add --name my-app --path /path/to/my-app --execute --manifest /private/path/commands.json
```

命令清单存于工作区以外的 0600 私有文件；配置内容与命令通过本机 CLI 授予，不允许模型远程添加可执行程序、任意参数或环境变量。`describe` 返回逻辑 ID、可用操作和命令名。macOS/Linux 客户端是当前执行平台，Windows 原生能力仍待实现。

独立验收使用两份临时 Git 项目、真实文件、Node 进程和 HTTP 服务：Git 状态和文件 CAS/备份、两项目并发构建、测试中启动服务并请求 `/health`、非零退出码隔离、正常结束/超时/取消清理。客户端全套 37 项通过，新增工作区聚焦最终 12 项通过（含真实 Cargo/rustc 调用别名）；隔离真实项目验收 9 项通过。宿主 PostgreSQL 3 项验证跨用户/租户、能力校验及撤权取消。Agent 持久任务、幂等派发、并发目标、取消与重启回归及 desktop/mobile 浏览器由插件仓库维护。

这些本地证据不等于多台物理设备或生产浏览器任务链路验收。生产部署与实际账号验证结果另附在该文件后续记录中。

## 2026-09-16 公网与本机验收

- Platform 源码 `abe91afce5a783bf1f099595f0d18845895bacc7`，252 宿主发布物 `9b612d3c5192d08ffbf72b2ea8735ba603ab3538` 已激活。独立 `aio-delivery` 同步升级 Bundle 校验器；二进制 SHA256 为 `4d199c6b6db62f77c41526df5e60029708774780a7b60c16c73e680ce050c23d`。Bundle 15 项、Delivery 2 项通过。
- 真实公网 `https://aio.addzero.site` 临时配对 Mac，使用正式队列、领取与续约：发现任务 `a15ca1f2-fe39-43fe-be9e-4780df70a0ea` 完成；双项目 Git/读取/构建/HTTP 200 任务 `f8cfc597-9a50-4824-9624-8bea02001a5e` 完成；故意失败 `da6661ec-0e1f-4ba4-ae7d-405722d0892d` 保留退出码 7，另一项目完成；远程取消 `199ed54e-084d-4d29-a11b-a96d2c84883c` 后实际进程退出。能力撤销生效，验收临时设备已撤销。
- 实际用户 Mac worker 安装 0.7.1，配对保持不变。登记逻辑工作区 `aio-space`（build/typecheck/test）、`aio-agent`（test/check-web）、`aio-platform`（只读）；命令清单在本机私有目录，前两者允许普通项目文件 CAS 写入。执行器运行 Space typecheck 和 Agent 16 项单元测试均为退出码 0；两项数据库测试此前在隔离数据库单独通过。Git 状态均能读取。
- 执行器的命名命令具有本机账号权限；不是操作系统沙箱。Git 写操作、Windows、多台物理设备、独立模型子智能体和长期开发服务托管不在本次实现范围。真实公网 worker 协议验收与生产自然语言模型调用验收不能混为一谈。

本机证据：`/tmp/aio-swarm-live-result.json`、`/tmp/aio-swarm-real-projects-result.json`、`/tmp/aio-swarm-browser-result.json`。测试目录为临时证据，不作为产品运行依赖。Space 0.7.1 已在本机安装，未宣称 npm 正式发布。

智能体 `f7c313a5c433591e8e4a06d127528e610cc0016b` 自动交付任务 59、60 均为 active；实际用户租户与默认租户的安装版本均已核对。公网浏览器使用真实登录账号验证“蜂群任务”入口、会话任务读取与弹窗，桌面及 390 像素宽度无横向溢出或 JavaScript 页面错误。生产验收账号未配置设备，结果列表为空；带失败、取消回执及刷新保留的界面由隔离端到端测试覆盖。
