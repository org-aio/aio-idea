use dioxus::prelude::*;

pub fn account_action(action: String) {
    if action == "logout" {
        spawn(async move {
            let _ = gloo_net::http::Request::post("/api/auth/logout")
                .send()
                .await;
            az_plugin_host::runtime::client::reload();
        });
    }
}
