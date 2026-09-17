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
            cx.open_window(WindowOptions::default(), |_, cx| {
                cx.new(|_| app::InboxRoot::new())
            })
            .unwrap();
        });
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    gpui_web::init_logging();
    run();
}
