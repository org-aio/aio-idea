# 产品宿主装配

接入 AIO IDEA 的身份服务、管理角色权限、品牌部署配置和默认插件组合。通用宿主、安装与运行时、开发沙箱属于 aio-platform；平台不读取这些业务表，也不依赖本目录。

插件业务权限由通用宿主按租户当前启用的安装版本自动提供给全部成员；产品身份适配只提供会话、成员状态及宿主管理权限，不再在安装时写入角色权限。

服务端启用平台的 `remote` feature；`AIO_REMOTE_CAPABILITIES` 可指定部署方的外部 MCP/unary gRPC 能力绑定文件，未配置时不连接外部服务。配置包含明确租户、版本路径、输入输出 schema、上游地址和凭据环境变量名；实际凭据不进入文件。绑定变更后重启宿主。Rhai 与 TypeScript 通过统一 `call_capability` 使用这些能力，仍要求显式 `runtime:<path>` 授权。配置格式、描述文件示例与边界见平台 `lib/plugin/host/src/runtime/server/remote/README.md`。
