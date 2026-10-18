//! 独立启动器二进制：由 systemd 以稳定路径启动，不随发布目录切换。
//!
//! 只转交给平台启动器库，自身不承载业务逻辑。

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    az_plugin_launcher::run_cli().await
}
