use gpui::{Context, IntoElement, Render, Styled, Window, div, prelude::*, px, rgb};

pub struct InboxRoot;

impl InboxRoot {
    pub fn new() -> Self {
        Self
    }
}

impl Render for InboxRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .bg(rgb(0x1e1e2e))
            .text_color(rgb(0xcdd6f4))
            .child(
                div()
                    .text_lg()
                    .child("contrib-inbox — booting"),
            )
            .child(
                div()
                    .mt(px(8.0))
                    .text_sm()
                    .text_color(rgb(0x6c7086))
                    .child("scaffold online"),
            )
    }
}
