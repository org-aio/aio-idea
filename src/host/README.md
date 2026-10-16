# 产品宿主装配

接入 AIO IDEA 的身份服务、管理角色权限、品牌部署配置和默认插件组合。通用宿主、安装与运行时、开发沙箱属于 aio-platform；平台不读取这些业务表，也不依赖本目录。

插件业务权限由通用宿主按租户当前启用的安装版本自动提供给全部成员；产品身份适配只提供会话、成员状态及宿主管理权限，不再在安装时写入角色权限。

服务端启用平台的 `remote` feature；`AIO_REMOTE_CAPABILITIES` 可指定部署方的外部 MCP/unary gRPC 能力绑定文件，未配置时不连接外部服务。配置包含明确租户、版本路径、输入输出 schema、上游地址和凭据环境变量名；实际凭据不进入文件。

正式宿主就绪后自动观察配置和 gRPC 描述文件；原子替换部署文件即可在线添加、更新或停用绑定，空 imports 可从零启动。完整候选校验成功后原子切换，失败保留旧版；同一脚本调用链固定使用同一能力快照。修改已有路径的契约需要新版本路径，凭据环境变量值的轮换仍需重启。

Rhai 与 TypeScript 通过统一 `call_capability` 使用这些能力，仍要求显式 `runtime:<path>` 授权。配置格式、描述文件示例与边界见平台 `lib/plugin/host/src/runtime/server/remote/README.md`。

默认组合提供独立 `EvolutionPlugin`，把候选、激活、回滚、观察及审计接到脚本能力层。`ProductIdentity::session_has_permissions` 在同一次 SQL 查询中检查当前会话、租户成员和全部所需角色权限；生成或验证后的提交重新查询，不沿用调用开始时的授权。管理员仍需逐项配置 `runtime:/v1/runtime/enhancements/...`，已有 `plugin:manage` 不自动获得这些能力。

双语言策略和安装示例在平台 `docs/plugin/evolution/`。模型能力独立提供；此产品没有配置默认模型、自动批准或后台演进调度。调用计数按租户、版本和激活代数持久化，源码回滚不包含业务数据回滚。

`plugins/server.rs` 将身份、文件、权限、租户和市场服务贡献到运行时的同一个 Dill 容器。每代先创建资源并登记关闭责任，再初始化和装配原有 Controller；初始化或路由装配失败由生命周期补偿。停用会移除路由并关闭对应连接池，重新启用创建新实例，业务数据保留。市场服务当前只有无资源的健康路由。

字典通过市场独立安装 v2 Sidecar 整包，目录、前端、业务进程与数据库迁移均属于该包。`required_permissions` 保留宿主 `dictionary:manage` 门槛，普通租户成员不能通过安装获取管理权限；发现、挂载和请求实时检查授权。产品不注册 `/api/dictionaries`，不链接字典实现。旧数据需要按 [字典迁移步骤](https://github.com/org-aio/aio-plugin-dictionary/blob/main/tests/README.md) 显式复制，不双写旧表。

`IdentityServicePlugin` 拥有真实身份服务；`ProductIdentityPlugin` 从当前容器注入它，在每次 `on_initialize` 中新建适配器自己的连接池，再将同一个 `ProductIdentity` 绑定到身份、授权和计费接口。停止和部分初始化失败均关闭本代资源。`identity.restart` 重建身份服务及依赖它的服务、控制器和适配器，会话仍从持久化数据库读取。`identity_tests` 使用真实 PostgreSQL 验证共享实例、重启、失败恢复和旧引用已关闭。

`server.rs` 先绑定监听器，再由 `RuntimeBootstrap` 准备配置，最后使用 `RuntimeHost::initialize_with_router` 装配。每次切换或恢复都会重新装配应用路由和首页权限快照。监听地址保持不变，系统 API 和静态资源请求也参与排空；切换窗口返回 503，旧响应流可能结束。系统服务实现仍编译进宿主，系统 UI 仍静态装配；在线生命周期尚不等于独立更新实现或动态交付 UI。源代码升级和隔离环境验收不代表生产宿主已经替换。

## 部署方在线管理

将 `AIO_RUNTIME_COMPOSITION` 指向部署方拥有的 JSON 文件。未配置时使用默认原生组合；配置后，启动时校验，并在运行期间每秒观察文件。先写新文件再原子替换目标文件：

```json
{
  "schema_version": 1,
  "rhai": { "enabled": true, "restart": 0 },
  "typescript": { "enabled": true, "restart": 0 },
  "mcp": { "enabled": true, "restart": 0 },
  "grpc": { "enabled": true, "restart": 0 },
  "evolution": { "enabled": true, "restart": 0 },
  "files": { "enabled": true, "restart": 0 },
  "access_control": { "enabled": true, "restart": 0 },
  "tenants": { "enabled": true, "restart": 0 },
  "marketplace": { "enabled": true, "restart": 0 },
  "identity": { "restart": 0 },
  "database": { "restart": 0 }
}
```

未填写的能力默认启用，`restart` 默认 0。改变某项的 `restart` 会重建该项及依赖它的插件；未变配方保留 Arc 身份和无关单例。字段表示产品支持的部署选项，原生插件注册和依赖身份仍使用 TypeId。身份服务和数据库是当前产品的必需资源，只允许重建；脚本语言、协议入口、演进控制及四项可选系统服务可以启停。停用某种语言后，依赖它的增强不可执行，已保存版本不会被删除。停用这些原生系统服务时对应 API 返回 404，静态 UI 菜单目前仍会显示。字典整包的启停由租户安装记录管理，其菜单跟随包生命周期。

整个候选经平台组合校验后提交；配置错误或可恢复的在线启动、路由装配失败恢复旧组合，记录错误。相同失败内容不自动重试，修改文件内容可再次提交；删文件不等于停用。文件最多 64 KiB，未知字段和未知 schema 版本均拒绝。恢复失败会停止监听，由部署环境的进程监督器重启。连接池已关闭时，恢复会创建新实例，不复用旧服务引用。

观察任务属于监听器所有者，位于运行时请求和插件任务的排空边界之外。该配置拥有全局宿主管理权，不应放在普通租户可写目录中；没有新增租户 HTTP 修改原生组合的入口。凭据和业务插件安装仍走各自正式授权流程。

## 启动恢复与本机命令

恢复记录存放于 `AIO_PLUGIN_CACHE/runtime-composition/`，未配置缓存目录时使用 `.aio/runtime/runtime-composition/`。记录保存绝对来源路径、最后成功组合和当前尝试摘要。正常启动和恢复命令必须使用相同的工作目录及环境变量，不能删除 `owner.lock` 抢占正在运行的宿主。

`RuntimeBootstrap` 在连接业务数据库、初始化身份插件之前持久化尝试。只有插件、完整应用路由和已绑定监听器全部就绪后才确认组合；在线更新同样先记录尝试、提交运行时、再确认。持久化失败会停止服务，不继续假定恢复记录有效。锁直到运行时关闭完成才释放。

新进程发现同一候选上次被拒绝或未完成，会按当前产品契约校验并启动最后成功组合，保留其中已停用的语言、协议、演进能力和系统服务。部署文件缺失或解析失败也使用同一路径。没有成功记录、记录损坏、来源路径不符或恢复内容不符合当前契约时停止启动。首次运行时初始化失败不会在同一进程冷启动第二套资源，下一次全新进程才尝试恢复。

命令使用同一个 `aio-idea` 二进制，无需 `aio.toml`、数据库、登录或已启动的业务插件：

```sh
export AIO_PLUGIN_CACHE=/path/to/runtime-cache
export AIO_RUNTIME_COMPOSITION=/path/to/runtime-composition.json
aio-idea runtime status
# 停服后显式允许当前配置再次初始化；先校验文件格式。
aio-idea runtime retry
# 停服后按当前契约校验并原子写回最后成功组合。
aio-idea runtime restore
# 停服后迁移旧部署文件中的 dictionary 字段，其他选项保持不变。
aio-idea runtime migrate
```

`status` 在服务运行时也可读取，输出持久化 JSON 快照或 `null`，不表示进程健康。`retry`、`restore`、`migrate` 必须取得独占锁，宿主存活时拒绝执行。自动回退从不改写部署文件；`restore` 与 `migrate` 原子写入候选，实际启动成功前保留旧的最后成功记录。已保存旧记录中的 `dictionary` 会校验并移除，其他停用状态保留；新部署输入直接拒绝这个退役字段。迁移不自动安装或启用字典包，原来停用的租户应保持整包停用。

恢复范围是原生运行时组合，不撤销业务数据迁移和外部副作用，也不更换宿主二进制。Unix 会同步记录与父目录，其他操作系统的运行及掉电恢复尚未验收。

阅读顺序：`runtime_bootstrap.rs` 的启动选择 → `runtime_composition.rs` 的在线提交 → `runtime_recovery.rs` 的本机恢复。无需数据库的配置测试使用 `cargo test --no-default-features --features server runtime_composition::bootstrap`；真实进程验收见 `tests/runtime/README.md`。
