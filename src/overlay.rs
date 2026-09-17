//! DOM text overlay (token / comment input).
//!
//! A real DOM <input>/<textarea> is used instead of a canvas text field so
//! IME, mobile keyboards and password managers keep working. While the
//! overlay is open the app sets `overlay_open` and ignores canvas shortcuts.

use std::cell::RefCell;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{HtmlInputElement, HtmlTextAreaElement};

thread_local! {
    static HANDLER: RefCell<Option<Box<dyn Fn(Option<String>)>>> = RefCell::new(None);
}

pub fn is_open() -> bool {
    HANDLER.with(|slot| slot.borrow().is_some())
}

pub fn open(title: &str, multiline: bool, initial: &str, on_done: Box<dyn Fn(Option<String>)>) {
    close();
    HANDLER.with(|slot| *slot.borrow_mut() = Some(on_done));

    let document = match web_sys::window().and_then(|w| w.document()) {
        Some(document) => document,
        None => return,
    };
    let root = match document.get_element_by_id("inbox-overlay") {
        Some(existing) => existing,
        None => build_shell(&document),
    };
    if let Some(label) = document.get_element_by_id("inbox-overlay-title") {
        label.set_text_content(Some(title));
    }
    if let Some(single) = document
        .get_element_by_id("inbox-overlay-input")
        .and_then(|el| el.dyn_into::<HtmlInputElement>().ok())
    {
        single.set_value(initial);
        single.set_type(if title.contains("token") {
            "password"
        } else {
            "text"
        });
        let _ = single.style().set_property(
            "display",
            if multiline { "none" } else { "block" },
        );
    }
    if let Some(multi) = document
        .get_element_by_id("inbox-overlay-area")
        .and_then(|el| el.dyn_into::<HtmlTextAreaElement>().ok())
    {
        multi.set_value(initial);
        let _ = multi
            .style()
            .set_property("display", if multiline { "block" } else { "none" });
    }
    let _ = root
        .dyn_ref::<web_sys::HtmlElement>()
        .map(|el| el.style().set_property("display", "flex"));

    // Focus the visible field.
    let field_id = if multiline {
        "inbox-overlay-area"
    } else {
        "inbox-overlay-input"
    };
    if let Some(field) = document
        .get_element_by_id(field_id)
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = field.focus();
    }
}

pub fn close() {
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        if let Some(root) = document.get_element_by_id("inbox-overlay") {
            let _ = root
                .dyn_ref::<web_sys::HtmlElement>()
                .map(|el| el.style().set_property("display", "none"));
        }
    }
    HANDLER.with(|slot| *slot.borrow_mut() = None);
}

fn finish(value: Option<String>) {
    close_inner_dom();
    let handler = HANDLER.with(|slot| slot.borrow_mut().take());
    if let Some(handle) = handler {
        handle(value);
    }
}

fn close_inner_dom() {
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        if let Some(root) = document.get_element_by_id("inbox-overlay") {
            let _ = root
                .dyn_ref::<web_sys::HtmlElement>()
                .map(|el| el.style().set_property("display", "none"));
        }
    }
}

fn current_value() -> String {
    let document = match web_sys::window().and_then(|w| w.document()) {
        Some(document) => document,
        None => return String::new(),
    };
    if let Some(area) = document
        .get_element_by_id("inbox-overlay-area")
        .and_then(|el| el.dyn_into::<HtmlTextAreaElement>().ok())
    {
        let display = area.style().get_property_value("display").unwrap_or_default();
        if display != "none" {
            return area.value();
        }
    }
    document
        .get_element_by_id("inbox-overlay-input")
        .and_then(|el| el.dyn_into::<HtmlInputElement>().ok())
        .map(|input| input.value())
        .unwrap_or_default()
}

fn build_shell(document: &web_sys::Document) -> web_sys::Element {
    let root = document.create_element("div").unwrap();
    root.set_id("inbox-overlay");
    root.set_attribute(
        "style",
        "position:fixed;inset:0;z-index:50;display:flex;align-items:flex-start;justify-content:center;background:rgba(17,17,27,0.72);padding-top:18vh;font-family:system-ui,sans-serif;",
    )
    .ok();
    root.set_inner_html(
        r#"<div style="width:min(560px,92vw);background:#1e1e2e;border:1px solid #45475a;border-radius:10px;padding:16px;color:#cdd6f4;">
      <div id="inbox-overlay-title" style="font-size:14px;font-weight:600;margin-bottom:10px;"></div>
      <input id="inbox-overlay-input" style="width:100%;box-sizing:border-box;background:#11111b;color:#cdd6f4;border:1px solid #45475a;border-radius:6px;padding:8px 10px;font-size:14px;" />
      <textarea id="inbox-overlay-area" rows="6" style="width:100%;box-sizing:border-box;background:#11111b;color:#cdd6f4;border:1px solid #45475a;border-radius:6px;padding:8px 10px;font-size:14px;display:none;"></textarea>
      <div style="display:flex;gap:8px;justify-content:flex-end;margin-top:12px;">
        <button id="inbox-overlay-cancel" style="background:#313244;color:#cdd6f4;border:0;border-radius:6px;padding:6px 14px;font-size:13px;">Cancel (esc)</button>
        <button id="inbox-overlay-ok" style="background:#89b4fa;color:#11111b;border:0;border-radius:6px;padding:6px 14px;font-size:13px;font-weight:600;">OK (ctrl+enter)</button>
      </div>
    </div>"#,
    );
    document.body().unwrap().append_child(&root).ok();

    let submit = Closure::<dyn Fn()>::new(|| finish(Some(current_value())));
    document
        .get_element_by_id("inbox-overlay-ok")
        .unwrap()
        .add_event_listener_with_callback("click", submit.as_ref().unchecked_ref())
        .ok();
    submit.forget();

    let cancel = Closure::<dyn Fn()>::new(|| finish(None));
    document
        .get_element_by_id("inbox-overlay-cancel")
        .unwrap()
        .add_event_listener_with_callback("click", cancel.as_ref().unchecked_ref())
        .ok();
    cancel.forget();

    let key = Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(|event: web_sys::KeyboardEvent| {
        if event.key() == "Escape" {
            finish(None);
        } else if event.key() == "Enter" && event.ctrl_key() {
            finish(Some(current_value()));
        }
    });
    root.add_event_listener_with_callback("keydown", key.as_ref().unchecked_ref())
        .ok();
    key.forget();

    root
}
