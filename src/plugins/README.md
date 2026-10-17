# 产品插件装配

`mod.rs` 由应用维护，通过 Dill 汇集静态页面、账户及顶栏入口。测试覆盖现有系统入口和设备助手的云端文件入口。

`server.rs` 选择身份、文件、权限、租户和设备通道的原生插件。`server_plugins()` 的结果交给平台 `RuntimeHost`；每个插件把原有 Service 和 Controller 注册到运行时唯一的 Dill 容器，声明初始化、路由和关闭步骤。创建资源后先登记到 `Resource<T>`，初始化失败也能关闭；重新启用创建新实例，不清空持久数据。身份服务变化会重建依赖它的控制器和产品身份适配器。

在线选择由 `AIO_RUNTIME_COMPOSITION` 控制，格式见 `../host/README.md`。文件、权限、租户和设备隧道可停用或重建；身份服务保持必需，只允许重建。这些原生服务仍编译进宿主，其对应系统 UI 仍静态装配；停用 API 尚不会隐藏静态页面。产品不维护另一套服务容器或复制业务实现。

Cargo 依赖和 feature、`aio.toml`、`.aio/plugins.lock` 必须一起更新。通用 CLI 的 `src/plugins.rs` 模板缺少本产品的系统装配，不能替换此模块；目录入口也让误运行生成器产生的同名文件在编译时明确报错，避免静默丢失菜单。

设备通道由 `DeviceWebviewsPlugin` 声明 Codex/dsh 协议，`DeviceTunnelPlugin` 单独声明内网服务协议及网关生命周期。平台不再内置设备协议枚举。字典和市场已通过固定摘要的 v2 整包动态引导，详见 `../host/README.md`；它们不在原生系统服务配置中。
