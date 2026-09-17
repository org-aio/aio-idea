#![forbid(unsafe_code)]

#[cfg(any(feature = "web", feature = "desktop"))]
mod account;
#[cfg(feature = "server")]
mod host;
mod plugins;
#[cfg(feature = "server")]
mod server;

#[cfg(all(feature = "server", any(feature = "web", feature = "desktop")))]
compile_error!("server 不能和 web 或 desktop 同时启用");

#[cfg(not(any(feature = "web", feature = "desktop", feature = "server")))]
fn main() {
    eprintln!("请选择 --features web、desktop 或 server");
}

#[cfg(feature = "server")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("supervisor") {
        return az_plugin_host::runtime::server::run_supervisor().await;
    }
    server::run().await
}

#[cfg(any(feature = "web", feature = "desktop"))]
fn main() {
    dioxus::launch(App);
}

#[cfg(any(feature = "web", feature = "desktop"))]
#[allow(non_snake_case)]
fn App() -> dioxus::prelude::Element {
    use dioxus::prelude::*;

    rsx! {
        az_ui_components::UiStylesheets {}
        ProductWorkspace {}
    }
}

#[cfg(any(feature = "web", feature = "desktop"))]
#[dioxus::prelude::component]
fn ProductWorkspace() -> dioxus::prelude::Element {
    use dioxus::prelude::*;
    let catalog = match plugins::client_catalog() {
        Ok(catalog) => catalog,
        Err(error) => return rsx! { p { role: "alert", "加载产品插件失败: {error}" } },
    };
    rsx! {
        az_plugin_host::Workspace {
            config: az_plugin_host::composition::BrowserComposition {
                label: "AIO IDEA".into(),
                pages: catalog.pages,
                account_items: catalog.account_items,
                topbar_items: catalog.topbar_items,
                login: aio_plugin_identity_client::LoginPage,
                account_action: account::account_action,
            }
        }
    }
}
