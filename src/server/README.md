# 服务器验证

后台浏览器验收启动正式系统插件 API，只允许本机独立测试库。
社区运行时目录在此验证中为空，不启动监督器，不改动公网租户。

# 静态资源传输

内容指纹资源使用一年 immutable 缓存，入口 HTML 携带实时验证的工作区快照并使用 private/no-store；此策略不覆盖鉴权 API 或插件票据资源。`static_files.rs` 优先发送构建时生成的 gzip 文件。发布新构建前执行 `node deploy/prepare-web.cjs`，使用 HTML 解析器添加 Wasm 预加载并预压缩静态资源，再合并历史版本资源。

# 运行时退出

产品使用平台默认插件组合，Rhai/TypeScript 增强、Sidecar、MCP 和 gRPC 共享同一运行时权限和版本服务。SIGTERM、Ctrl-C 或监听失败会进入 `RuntimeState::shutdown`，等待已进入的有限工作完成并反序释放资源。发布或进程重启不删除持久化安装及活动版本；不能从源码更新推断现役服务已经升级。

产品的 `server` feature 显式启用平台的 `server`、`wasm` 和 `sidecar`，保留完整插件运行能力。平台精简宿主可仅启用 `server` 提供 MCP/gRPC 与外部执行器，不再编译 Wasmtime；编译选择与 Dill 运行时装配分别决定可用实现和实际启动的插件。

# 增强协议验收

从 `src/server.rs` 的运行时初始化进入，再查看平台的 `runtime/server/plugins` 与 `runtime/server/protocols`。协议入口使用产品的 `ProductIdentity`，沿用真实登录会话与当前租户权限。

| 入口 | 调用方式 |
| --- | --- |
| MCP | `/api/runtime/mcp`，Streamable HTTP；工具名 `v1.math.add` 对应 `/v1/math/add` |
| gRPC | 同一宿主 HTTP/2 监听地址，`aio.runtime.v1.Enhancements/List` 与 `Invoke` |
| Sidecar | 受监督进程通过宿主 broker 调用清单授予的增强路径 |

MCP/gRPC 当前提供运行时增强的发现和执行；不包含外部服务导入、流式 RPC 或 AI 自动激活策略。完整能力边界见 [平台验收清单](https://github.com/org-aio/aio-platform/blob/main/docs/plugin/everything-as-plugin.md)。

基础验证：

```sh
cargo test --locked --no-default-features --features server
cargo clippy --locked --all-targets --no-default-features --features server -- -D warnings
cargo check --locked --no-default-features --features web --target wasm32-unknown-unknown
```

真实协议验收使用独立本机数据库、空默认插件组合、临时文件/密钥/组件存储，以及不存在的独立监督器 socket。启动正式服务端二进制后登录测试账号，提交并激活同时包含 Rhai 与 TypeScript 的候选包；通过 MCP 与生成契约的 gRPC 客户端核对结果及版本，再发 SIGTERM，确认正常退出并释放端口。身份启动快照与跨租户前端票据另由 `runtime_tests` 中的隔离 HTTP 测试验证；测试配置不得连接现役监督器或生产数据库。
