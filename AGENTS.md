# AIO IDEA 开发约定

- 本仓库是 AIO IDEA 发布应用，不是整个 AIO 工作区；跨仓库任务必须分别检查相关子仓库状态，保留无关脏文件。
- 发布应用壳消费 `lib/dioxus-admin-workbench` 中的通用 shell；AIO 专属业务入口、平台文档和组织仓库链接放在插件中，不写进通用 shell。
- 新增或调整 AIO 插件时，同步依赖、feature、`src/plugins.rs` 静态注册、catalog、`.aio/plugins.lock`、配置和相关 Cargo Git revision。
- 涉及 submodule 或 Git revision 时，先确认子模块本地改动是否已经等价于目标提交；移动 detached submodule 前保护现有改动。
- Dioxus Web 浏览器验收使用 `dx serve --platform web`。`cargo run --features web` 只能作为编译检查，不能证明 wasm Web 页面运行正常。
- 顶栏资源应区分平台文档入口和 GitHub/组织仓库入口；验收时检查实际菜单项、链接数量、控制台错误和页面渲染。
- 插件独立检查如果遇到 `dill` 工具链要求，优先按仓库既有 `rust-toolchain.toml` 固定 nightly，不临时全局切换工具链。

