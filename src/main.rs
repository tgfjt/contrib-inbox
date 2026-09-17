#![cfg_attr(target_family = "wasm", no_main)]

mod app;
mod fonts;
mod github;
mod overlay;
mod store;

use std::rc::Rc;

use gpui::{App, AppContext, WindowOptions};

fn run() {
    let platform = Rc::new(gpui_web::WebPlatform::new(false));
    let http_client = std::sync::Arc::new(platform.fetch_http_client());
    gpui::Application::with_platform(platform)
        .with_http_client(http_client)
        .run(|cx: &mut App| {
            if !fonts::load_fonts(cx) {
                return;
            }
            let window = cx
                .open_window(WindowOptions::default(), |_, cx| {
                    cx.new(|cx| app::Inbox::new(cx))
                })
                .unwrap();
            let inbox = window.update(cx, |_, _, cx| cx.entity()).unwrap();
            app::bind_keys(&inbox, cx);
            window
                .update(cx, |view, window, cx| {
                    window.focus(&view.focus_handle(cx), cx);
                })
                .unwrap();
        });
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    gpui_web::init_logging();
    register_service_worker();
    run();
}

#[cfg(target_family = "wasm")]
fn register_service_worker() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let container = window.navigator().service_worker();
    let promise = container.register("/sw.js");
    wasm_bindgen_futures::spawn_local(async move {
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    });
}
