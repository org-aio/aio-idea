# 产品宿主装配

接入 AIO IDEA 的身份服务、管理角色权限、品牌部署配置和默认插件组合。通用宿主、安装与运行时、开发沙箱属于 aio-platform；平台不读取这些业务表，也不依赖本目录。

插件业务权限由通用宿主按租户当前启用的安装版本自动提供给全部成员；产品身份适配只提供会话、成员状态及宿主管理权限，不再在安装时写入角色权限。

服务端启用平台的 `remote` feature；`AIO_REMOTE_CAPABILITIES` 可指定部署方的外部 MCP/unary gRPC 能力绑定文件，未配置时不连接外部服务。配置包含明确租户、版本路径、输入输出 schema、上游地址和凭据环境变量名；实际凭据不进入文件。

正式宿主就绪后自动观察配置和 gRPC 描述文件；原子替换部署文件即可在线添加、更新或停用绑定，空 imports 可从零启动。完整候选校验成功后原子切换，失败保留旧版；同一脚本调用链固定使用同一能力快照。修改已有路径的契约需要新版本路径，凭据环境变量值的轮换仍需重启。

Rhai 与 TypeScript 通过统一 `call_capability` 使用这些能力，仍要求显式 `runtime:<path>` 授权。配置格式、描述文件示例与边界见平台 `lib/plugin/host/src/runtime/server/remote/README.md`。

默认组合提供独立 `EvolutionPlugin`，把候选、激活、回滚、观察及审计接到脚本能力层。`ProductIdentity::session_has_permissions` 在同一次 SQL 查询中检查当前会话、租户成员和全部所需角色权限；生成或验证后的提交重新查询，不沿用调用开始时的授权。管理员仍需逐项配置 `runtime:/v1/runtime/enhancements/...`，已有 `plugin:manage` 不自动获得这些能力。

双语言策略和安装示例在平台 `docs/plugin/evolution/`。模型能力独立提供；此产品没有配置默认模型、自动批准或后台演进调度。调用计数按租户、版本和激活代数持久化，源码回滚不包含业务数据回滚。
