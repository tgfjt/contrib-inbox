use std::collections::HashMap;

use gpui::{
    App, AsyncApp, Context, Entity, FocusHandle, Focusable, IntoElement, KeyBinding, Render,
    Styled, Window, actions, div, prelude::*, px, rgb,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::github::{self, CheckRuns, Comment, Item, Review, SearchResponse};
use crate::{overlay, store};

actions!(
    inbox,
    [
        OpenGithub,
        Refresh,
        AddComment,
        CloseItem,
        EditToken,
        MoveDown,
        MoveUp,
        ShowOpen,
        ShowMerged,
        ShowClosed,
        ShowStale
    ]
);

const BG: u32 = 0x1e1e2e;
const PANEL: u32 = 0x181825;
const BORDER: u32 = 0x45475a;
const TEXT: u32 = 0xcdd6f4;
const DIM: u32 = 0x6c7086;
const ACCENT: u32 = 0x89b4fa;
const GREEN: u32 = 0xa6e3a1;
const RED: u32 = 0xf38ba8;
const YELLOW: u32 = 0xf9e2af;
const MAUVE: u32 = 0xcba6f7;

const STALE_DAYS: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filter {
    Open,
    Merged,
    Closed,
    Stale,
}

impl Filter {
    fn label(self) -> &'static str {
        match self {
            Filter::Open => "open",
            Filter::Merged => "merged",
            Filter::Closed => "closed",
            Filter::Stale => "stale",
        }
    }

    fn all() -> [Filter; 4] {
        [Filter::Open, Filter::Merged, Filter::Closed, Filter::Stale]
    }
}

#[derive(Debug, Clone)]
struct Ci {
    label: String,
    color: u32,
}

#[derive(Debug, Clone, Default)]
struct Detail {
    comments: Vec<Comment>,
    reviews: Vec<Review>,
    ci: Option<Ci>,
    error: Option<String>,
}

pub struct Inbox {
    focus: FocusHandle,
    token: Option<String>,
    login: Option<String>,
    items: Vec<Item>,
    loading: bool,
    error: Option<String>,
    status: String,
    filter: Filter,
    selected: usize,
    seen: HashMap<String, String>,
    details: HashMap<u64, Detail>,
    detail_loading: bool,
}

/// Wall clock that works on wasm (std SystemTime panics on wasm-unknown).
fn now_utc() -> OffsetDateTime {
    let millis = js_sys::Date::now() as i128;
    OffsetDateTime::from_unix_timestamp_nanos(millis * 1_000_000)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

impl Inbox {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let token = store::load_token().filter(|t| !t.trim().is_empty());
        let login = store::load_login().filter(|l| !l.trim().is_empty());
        let has_token = token.is_some();
        let inbox = Self {
            focus: cx.focus_handle(),
            token,
            login,
            items: Vec::new(),
            loading: false,
            error: None,
            status: if has_token {
                String::new()
            } else {
                "no token — press t to set a GitHub token".to_string()
            },
            filter: Filter::Open,
            selected: 0,
            seen: store::load_seen(),
            details: HashMap::new(),
            detail_loading: false,
        };
        // NOTE: Application::run holds the AppCell borrow for the whole launch
        // callback, so no AsyncApp (RefCell) access is allowed here. Defer boot
        // to a foreground task, which runs after the borrow is released.
        cx.spawn(async move |weak, cx| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            Self::boot(&this, cx);
        })
        .detach();
        inbox
    }

    pub fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }

    /// Kick off login + first refresh. Call once from outside any update.
    pub fn boot(this: &Entity<Self>, cx: &mut AsyncApp) {
        let (token, login) =
            this.read_with(cx, |inbox, _| (inbox.token.clone(), inbox.login.clone()));
        let Some(token) = token else {
            return;
        };
        if login.is_some() {
            Self::start_refresh(this, cx);
            return;
        }
        this.update(cx, |inbox, cx| {
            inbox.loading = true;
            inbox.status = "signing in …".to_string();
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let login = github::get_login(&token).await;
            let have_login = this.update(&mut *cx, |inbox, cx| {
                match login {
                    Ok(login) => {
                        store::save_login(&login);
                        inbox.login = Some(login);
                        inbox.error = None;
                        true
                    }
                    Err(message) => {
                        inbox.error = Some(message);
                        inbox.loading = false;
                        inbox.status = "sign-in failed — press t to retry".to_string();
                        cx.notify();
                        false
                    }
                }
            });
            if have_login {
                Self::start_refresh(&this, cx);
            }
        })
        .detach();
    }

    // ---- pure state helpers (safe inside update closures) ----

    fn visible(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| match self.filter {
                Filter::Open => item.is_open(),
                Filter::Merged => item.is_merged(),
                Filter::Closed => !item.is_open() && !item.is_merged(),
                Filter::Stale => item.is_open() && is_stale(item),
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn count(&self, filter: Filter) -> usize {
        self.items
            .iter()
            .filter(|item| match filter {
                Filter::Open => item.is_open(),
                Filter::Merged => item.is_merged(),
                Filter::Closed => !item.is_open() && !item.is_merged(),
                Filter::Stale => item.is_open() && is_stale(item),
            })
            .count()
    }

    fn selected_item(&self) -> Option<Item> {
        self.visible()
            .get(self.selected)
            .and_then(|index| self.items.get(*index))
            .cloned()
    }

    fn is_fresh(&self, item: &Item) -> bool {
        match self.seen.get(&item.id.to_string()) {
            Some(seen_at) => seen_at.as_str() < item.updated_at.as_str(),
            None => true,
        }
    }

    /// Select a row (no fetching). Returns true when the selection moved.
    fn select_index(&mut self, position: usize) -> bool {
        let len = self.visible().len();
        if len == 0 {
            return false;
        }
        let next = position.min(len - 1);
        if next == self.selected {
            return false;
        }
        self.selected = next;
        true
    }

    fn apply_search(&mut self, result: Result<SearchResponse, String>) {
        self.loading = false;
        match result {
            Ok(response) => {
                self.items = response
                    .items
                    .into_iter()
                    .filter(github::is_external)
                    .collect();
                let fresh = self.items.iter().filter(|i| self.is_fresh(i)).count();
                self.selected = 0;
                self.error = None;
                self.status = format!(
                    "{} external ({} total) · {} updated",
                    self.items.len(),
                    response.total_count,
                    fresh
                );
            }
            Err(message) => {
                self.error = Some(message.clone());
                self.status = format!("refresh failed: {message}");
            }
        }
    }

    // ---- task starters (never call from inside an update closure) ----

    pub fn start_refresh(this: &Entity<Self>, cx: &mut AsyncApp) {
        let (token, login) =
            this.read_with(cx, |inbox, _| (inbox.token.clone(), inbox.login.clone()));
        let (Some(token), Some(login)) = (token, login) else {
            this.update(cx, |inbox, cx| {
                inbox.status = "no token — press t to set a GitHub token".to_string();
                cx.notify();
            });
            return;
        };
        this.update(cx, |inbox, cx| {
            inbox.loading = true;
            inbox.error = None;
            inbox.status = format!("refreshing @{login} …");
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = github::search(&login, &token).await;
            this.update(&mut *cx, |inbox, cx| {
                inbox.apply_search(result);
                cx.notify();
            });
            Self::start_detail(&this, cx);
        })
        .detach();
    }

    pub fn start_detail(this: &Entity<Self>, cx: &mut AsyncApp) {
        let (token, item) =
            this.read_with(cx, |inbox, _| (inbox.token.clone(), inbox.selected_item()));
        let (Some(token), Some(item)) = (token, item) else {
            return;
        };
        this.update(&mut *cx, |inbox, cx| {
            inbox
                .seen
                .insert(item.id.to_string(), item.updated_at.clone());
            inbox.detail_loading = true;
            cx.notify();
        });
        let seen = this.read_with(cx, |inbox, _| inbox.seen.clone());
        store::save_seen(&seen);
        let this = this.clone();
        cx.spawn(async move |cx| {
            let comments = github::comments(&item, &token).await.unwrap_or_default();
            let mut reviews = Vec::new();
            let mut ci = None;
            let mut error = None;
            if item.is_pr() {
                match github::reviews(&item, &token).await {
                    Ok(list) => reviews = list,
                    Err(message) => error = Some(message),
                }
                match github::pr_detail(&item, &token).await {
                    Ok(detail) => {
                        let repo = item.repo_full_name();
                        match github::check_runs(&repo, &detail.head.sha, &token).await {
                            Ok(runs) => ci = Some(summarize_ci(&runs)),
                            Err(message) => error = Some(message),
                        }
                    }
                    Err(message) => error = Some(message),
                }
            }
            let detail = Detail {
                comments,
                reviews,
                ci,
                error,
            };
            let id = item.id;
            this.update(cx, |inbox, cx| {
                inbox.details.insert(id, detail);
                inbox.detail_loading = false;
                cx.notify();
            });
        })
        .detach();
    }

    pub fn reselect(this: &Entity<Self>, cx: &mut AsyncApp, position: usize) {
        let moved = this.update(cx, |inbox, cx| {
            let moved = inbox.select_index(position);
            cx.notify();
            moved
        });
        if moved {
            Self::start_detail(this, cx);
        }
    }

    pub fn move_selection(this: &Entity<Self>, cx: &mut AsyncApp, delta: isize) {
        let len = this.read_with(cx, |inbox, _| inbox.visible().len());
        if len == 0 {
            return;
        }
        let current = this.read_with(cx, |inbox, _| inbox.selected);
        let next = (current as isize + delta).clamp(0, len as isize - 1) as usize;
        Self::reselect(this, cx, next);
    }

    fn set_filter(this: &Entity<Self>, cx: &mut AsyncApp, filter: Filter) {
        this.update(cx, |inbox, cx| {
            inbox.filter = filter;
            inbox.selected = 0;
            cx.notify();
        });
        Self::start_detail(this, cx);
    }

    pub fn open_selected(this: &Entity<Self>, cx: &mut AsyncApp) {
        if let Some(item) = this.read_with(cx, |inbox, _| inbox.selected_item()) {
            store::open_github(&item.html_url);
        }
    }

    pub fn start_comment(this: &Entity<Self>, cx: &mut AsyncApp) {
        let (token, item) =
            this.read_with(cx, |inbox, _| (inbox.token.clone(), inbox.selected_item()));
        let (Some(token), Some(item)) = (token, item) else {
            this.update(cx, |inbox, cx| {
                inbox.status = "nothing to comment on".to_string();
                cx.notify();
            });
            return;
        };
        let prompt = format!("comment on {}#{}", item.repo_full_name(), item.number);
        let this = this.clone();
        let async_cx = cx.clone();
        overlay::open(
            &prompt,
            true,
            "",
            Box::new(move |value| {
                let Some(text) = value else { return };
                if text.trim().is_empty() {
                    return;
                }
                let this = this.clone();
                let token = token.clone();
                let item = item.clone();
                async_cx
                    .spawn(async move |cx| {
                        let posted = github::post_comment(&item, &text, &token).await;
                        let id = item.id;
                        let posted_ok = posted.is_ok();
                        this.update(&mut *cx, |inbox, cx| {
                            match posted {
                                Ok(()) => {
                                    inbox.status = format!("comment posted on #{}", item.number);
                                    inbox.details.remove(&id);
                                }
                                Err(message) => {
                                    inbox.status = format!("comment failed: {message}");
                                }
                            }
                            cx.notify();
                        });
                        if posted_ok {
                            Self::start_detail(&this, cx);
                        }
                    })
                    .detach();
            }),
        );
    }

    pub fn start_close(this: &Entity<Self>, cx: &mut AsyncApp) {
        let (token, item) =
            this.read_with(cx, |inbox, _| (inbox.token.clone(), inbox.selected_item()));
        let (Some(token), Some(item)) = (token, item) else {
            this.update(cx, |inbox, cx| {
                inbox.status = "nothing to close".to_string();
                cx.notify();
            });
            return;
        };
        if !item.is_open() {
            this.update(cx, |inbox, cx| {
                inbox.status = "already closed — x only works on open items".to_string();
                cx.notify();
            });
            return;
        }
        this.update(cx, |inbox, cx| {
            inbox.status = format!("closing #{} …", item.number);
            cx.notify();
        });
        let this = this.clone();
        cx.spawn(async move |cx| {
            let result = github::close_item(&item, &token).await;
            let closed = this.update(&mut *cx, |inbox, cx| {
                match result {
                    Ok(()) => {
                        inbox.status = format!("closed #{}", item.number);
                        cx.notify();
                        true
                    }
                    Err(message) => {
                        inbox.status = format!("close failed: {message}");
                        cx.notify();
                        false
                    }
                }
            });
            if closed {
                Self::start_refresh(&this, cx);
            }
        })
        .detach();
    }

    pub fn start_token_editor(this: &Entity<Self>, cx: &mut AsyncApp) {
        let initial = this.read_with(cx, |inbox, _| inbox.token.clone().unwrap_or_default());
        let this = this.clone();
        let async_cx = cx.clone();
        overlay::open(
            "GitHub token (classic PAT with repo scope)",
            false,
            &initial,
            Box::new(move |value| {
                let Some(token) = value else { return };
                let token = token.trim().to_string();
                let this = this.clone();
                if token.is_empty() {
                    store::clear_token();
                    async_cx.update(|cx| {
                        this.update(cx, |inbox, cx| {
                            inbox.token = None;
                            inbox.login = None;
                            inbox.items.clear();
                            inbox.status =
                                "token cleared — press t to set a GitHub token".to_string();
                            cx.notify();
                        });
                    });
                    return;
                }
                async_cx
                    .spawn(async move |cx| {
                        let login = github::get_login(&token).await;
                        let valid = this.update(&mut *cx, |inbox, cx| match login {
                            Ok(login) => {
                                store::save_token(&token);
                                store::save_login(&login);
                                inbox.token = Some(token);
                                inbox.login = Some(login);
                                inbox.error = None;
                                inbox.status = "token saved".to_string();
                                cx.notify();
                                true
                            }
                            Err(message) => {
                                inbox.error = Some(message.clone());
                                inbox.status = format!("invalid token: {message}");
                                cx.notify();
                                false
                            }
                        });
                        if valid {
                            Self::start_refresh(&this, cx);
                        }
                    })
                    .detach();
            }),
        );
    }
}

impl Focusable for Inbox {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn is_stale(item: &Item) -> bool {
    let updated = match OffsetDateTime::parse(&item.updated_at, &Rfc3339) {
        Ok(time) => time,
        Err(_) => return false,
    };
    updated < now_utc() - time::Duration::days(STALE_DAYS)
}

fn ago(when: &str) -> String {
    let parsed = match OffsetDateTime::parse(when, &Rfc3339) {
        Ok(time) => time,
        Err(_) => return when.to_string(),
    };
    let delta = now_utc() - parsed;
    if delta.is_negative() {
        return "now".to_string();
    }
    let seconds = delta.whole_seconds();
    if seconds < 60 {
        "just now".to_string()
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86400)
    }
}

fn summarize_ci(runs: &CheckRuns) -> Ci {
    if runs.total_count == 0 {
        return Ci {
            label: "no checks".to_string(),
            color: DIM,
        };
    }
    let mut passed = 0u64;
    let mut failed = 0u64;
    let mut pending = 0u64;
    for run in &runs.check_runs {
        if run.status != "completed" {
            pending += 1;
            continue;
        }
        match run.conclusion.as_deref() {
            Some("success") | Some("skipped") | Some("neutral") => passed += 1,
            _ => failed += 1,
        }
    }
    if failed > 0 {
        Ci {
            label: format!("CI ✗ {failed} failed"),
            color: RED,
        }
    } else if pending > 0 {
        Ci {
            label: format!("CI … {pending} pending"),
            color: YELLOW,
        }
    } else {
        Ci {
            label: format!("CI ✓ {passed} passed"),
            color: GREEN,
        }
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    format!(
        "{}…",
        text.chars().take(max_chars.saturating_sub(1)).collect::<String>()
    )
}

fn state_badge(item: &Item) -> (String, u32) {
    if item.is_merged() {
        ("merged".to_string(), MAUVE)
    } else if item.is_pr() {
        if item.is_open() {
            if item.draft == Some(true) {
                ("draft".to_string(), DIM)
            } else {
                ("open".to_string(), GREEN)
            }
        } else {
            ("closed".to_string(), RED)
        }
    } else if item.is_open() {
        ("open".to_string(), GREEN)
    } else {
        ("closed".to_string(), MAUVE)
    }
}


/// Colored status dot (a text glyph like ● is tofu in the bundled font).
fn dot(color: u32) -> gpui::Div {
    div()
        .w(px(8.0))
        .h(px(8.0))
        .mr(px(5.0))
        .rounded_md()
        .bg(rgb(color))
}

fn review_badge(state: &str) -> (String, u32) {
    match state {
        "APPROVED" => ("approved".to_string(), GREEN),
        "CHANGES_REQUESTED" => ("changes requested".to_string(), RED),
        "COMMENTED" => ("commented".to_string(), YELLOW),
        "DISMISSED" => ("dismissed".to_string(), DIM),
        _ => (state.to_lowercase(), DIM),
    }
}

impl Render for Inbox {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let visible = self.visible();
        let selected_id = visible
            .get(self.selected)
            .and_then(|index| self.items.get(*index))
            .map(|item| item.id);
        let this = cx.entity();

        let tabs = Filter::all()
            .into_iter()
            .map(|filter| {
                let active = filter == self.filter;
                let label = format!("{} {}", filter.label(), self.count(filter));
                let handler = this.clone();
                div()
                    .id(format!("tab-{}", filter.label()))
                    .px(px(10.0))
                    .py(px(4.0))
                    .mr(px(6.0))
                    .rounded_md()
                    .text_sm()
                    .bg(if active { rgb(ACCENT) } else { rgb(PANEL) })
                    .text_color(if active { rgb(BG) } else { rgb(DIM) })
                    .child(label)
                    .on_click(move |_, _, cx| {
                        let handler = handler.clone();
                        cx.spawn(async move |cx| {
                            Self::set_filter(&handler, cx, filter);
                        })
                        .detach();
                    })
            })
            .collect::<Vec<_>>();

        let rows = visible
            .iter()
            .enumerate()
            .map(|(position, index)| {
                let item = &self.items[*index];
                let active = Some(item.id) == selected_id;
                let fresh = self.is_fresh(item);
                let (state, color) = state_badge(item);
                let handler = this.clone();
                div()
                    .id(format!("row-{}", item.id))
                    .px(px(10.0))
                    .py(px(7.0))
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .bg(if active {
                        rgb(0x313244)
                    } else if fresh {
                        rgb(0x24273a)
                    } else {
                        rgb(BG)
                    })
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .child(
                                div()
                                    .mr(px(8.0))
                                    .text_xs()
                                    .text_color(rgb(if fresh { ACCENT } else { DIM }))
                                    .child(if item.is_pr() { "PR" } else { "Issue" }),
                            )
                            .child(
                                div()
                                    .mr(px(8.0))
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .text_xs()
                                    .text_color(rgb(color))
                                    .child(dot(color))
                                    .child(state.clone()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_sm()
                                    .text_color(rgb(TEXT))
                                    .child(truncate(&item.title, 72)),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(2.0))
                            .text_xs()
                            .text_color(rgb(DIM))
                            .child(format!(
                                "{}#{} · {}",
                                item.repo_short(),
                                item.number,
                                ago(&item.updated_at)
                            )),
                    )
                    .on_click(move |_, _, cx| {
                        let handler = handler.clone();
                        cx.spawn(async move |cx| {
                            Self::reselect(&handler, cx, position);
                        })
                        .detach();
                    })
            })
            .collect::<Vec<_>>();

        let detail = selected_id.and_then(|id| self.details.get(&id).cloned());
        let selected = self.selected_item();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            // header
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .px(px(12.0))
                    .py(px(8.0))
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(PANEL))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(TEXT))
                            .mr(px(12.0))
                            .child("contrib-inbox"),
                    )
                    .children(tabs)
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(rgb(DIM))
                            .text_right()
                            .child(match &self.login {
                                Some(login) => format!("@{login}"),
                                None => "no login".to_string(),
                            }),
                    ),
            )
            // main 3 panes
            .child(
                div()
                    .flex()
                    .flex_1()
                    .flex_row()
                    .overflow_hidden()
                    // left: list
                    .child(
                        div()
                            .w(px(360.0))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .border_r_1()
                            .border_color(rgb(BORDER))
                            .child(
                                div()
                                    .id("list-scroll")
                                    .flex_1()
                                    .overflow_y_scroll()
                                    .children(if rows.is_empty() {
                                        vec![div()
                                            .id("list-empty")
                                            .p(px(16.0))
                                            .text_sm()
                                            .text_color(rgb(DIM))
                                            .child(if self.loading {
                                                "loading …"
                                            } else if self.token.is_none() {
                                                "press t to set a GitHub token"
                                            } else {
                                                "nothing here"
                                            })]
                                    } else {
                                        rows
                                    }),
                            ),
                    )
                    // middle: meta
                    .child(
                        div()
                            .id("meta-scroll")
                            .w(px(300.0))
                            .flex_none()
                            .overflow_y_scroll()
                            .p(px(12.0))
                            .border_r_1()
                            .border_color(rgb(BORDER))
                            .children(render_meta(&selected, &detail)),
                    )
                    // right: body + comments + reviews
                    .child(
                        div()
                            .id("detail-scroll")
                            .flex_1()
                            .overflow_y_scroll()
                            .p(px(12.0))
                            .children(render_detail(&selected, &detail, self.detail_loading)),
                    ),
            )
            // footer
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .px(px(12.0))
                    .py(px(6.0))
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(PANEL))
                    .text_xs()
                    .child(
                        div().text_color(rgb(DIM)).child(
                            "j/k move · enter open · r refresh · c comment · x close · t token · 1-4 filter",
                        ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_right()
                            .text_color(rgb(if self.error.is_some() { RED } else { DIM }))
                            .child(if let Some(error) = &self.error {
                                truncate(error, 80)
                            } else if self.status.is_empty() {
                                String::new()
                            } else {
                                truncate(&self.status, 80)
                            }),
                    ),
            )
    }
}

fn render_meta(selected: &Option<Item>, detail: &Option<Detail>) -> Vec<gpui::AnyElement> {
    let Some(item) = selected else {
        return vec![
            div()
                .text_sm()
                .text_color(rgb(DIM))
                .child("select an item")
                .into_any_element(),
        ];
    };
    let (state, color) = state_badge(item);
    let mut rows: Vec<gpui::AnyElement> = vec![
        div()
            .text_base()
            .text_color(rgb(TEXT))
            .child(item.title.clone())
            .into_any_element(),
        div()
            .mt(px(6.0))
            .text_sm()
            .text_color(rgb(ACCENT))
            .child(format!("{}#{}", item.repo_full_name(), item.number))
            .into_any_element(),
        div()
            .mt(px(6.0))
            .flex()
            .flex_row()
            .text_xs()
            .child(
                div()
                    .mr(px(8.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .text_color(rgb(color))
                    .child(dot(color))
                    .child(state.clone()),
            )
            .child(
                div()
                    .text_color(rgb(DIM))
                    .child(if item.is_pr() { "pull request" } else { "issue" }),
            )
            .into_any_element(),
        div()
            .mt(px(6.0))
            .text_xs()
            .text_color(rgb(DIM))
            .child(format!(
                "updated {} · created {}",
                ago(&item.updated_at),
                ago(&item.created_at)
            ))
            .into_any_element(),
    ];
    if item.is_pr() {
        let ci = detail.as_ref().and_then(|d| d.ci.clone());
        rows.push(
            div()
                .mt(px(6.0))
                .text_sm()
                .text_color(rgb(ci.as_ref().map(|c| c.color).unwrap_or(DIM)))
                .child(ci.map(|c| c.label).unwrap_or_else(|| "CI …".to_string()))
                .into_any_element(),
        );
    }
    if !item.labels.is_empty() {
        let labels = item
            .labels
            .iter()
            .map(|label| label.name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        rows.push(
            div()
                .mt(px(6.0))
                .text_xs()
                .text_color(rgb(YELLOW))
                .child(truncate(&labels, 60))
                .into_any_element(),
        );
    }
    rows.push(
        div()
            .mt(px(6.0))
            .text_xs()
            .text_color(rgb(DIM))
            .child(format!("{} comments", item.comments))
            .into_any_element(),
    );
    rows
}

fn render_detail(
    selected: &Option<Item>,
    detail: &Option<Detail>,
    loading: bool,
) -> Vec<gpui::AnyElement> {
    let Some(item) = selected else {
        return vec![
            div()
                .text_sm()
                .text_color(rgb(DIM))
                .child("nothing selected")
                .into_any_element(),
        ];
    };
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    rows.push(
        div()
            .text_xs()
            .text_color(rgb(DIM))
            .child("BODY")
            .into_any_element(),
    );
    let body = item.body.as_deref().unwrap_or("(no description)").trim();
    for line in body.lines().take(40) {
        rows.push(
            div()
                .text_sm()
                .text_color(rgb(TEXT))
                .child(if line.is_empty() {
                    " ".to_string()
                } else {
                    truncate(line, 110)
                })
                .into_any_element(),
        );
    }
    let Some(detail) = detail else {
        rows.push(
            div()
                .mt(px(8.0))
                .text_xs()
                .text_color(rgb(DIM))
                .child(if loading { "loading …" } else { "no detail" })
                .into_any_element(),
        );
        return rows;
    };
    if let Some(error) = &detail.error {
        rows.push(
            div()
                .mt(px(8.0))
                .text_xs()
                .text_color(rgb(RED))
                .child(truncate(error, 100))
                .into_any_element(),
        );
    }
    rows.push(
        div()
            .mt(px(12.0))
            .text_xs()
            .text_color(rgb(DIM))
            .child(format!("COMMENTS ({})", detail.comments.len()))
            .into_any_element(),
    );
    let latest: Vec<&Comment> = detail.comments.iter().rev().take(5).collect();
    if latest.is_empty() {
        rows.push(
            div()
                .text_sm()
                .text_color(rgb(DIM))
                .child("no comments")
                .into_any_element(),
        );
    }
    for comment in latest.into_iter().rev() {
        let login = comment
            .user
            .as_ref()
            .map(|u| u.login.clone())
            .unwrap_or_else(|| "?".to_string());
        rows.push(
            div()
                .mt(px(6.0))
                .p(px(8.0))
                .rounded_md()
                .bg(rgb(PANEL))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(ACCENT))
                        .child(format!("@{login} · {}", ago(&comment.updated_at))),
                )
                .children(
                    comment
                        .body
                        .as_deref()
                        .unwrap_or("")
                        .lines()
                        .take(6)
                        .map(|line| {
                            div()
                                .text_sm()
                                .text_color(rgb(TEXT))
                                .child(if line.is_empty() {
                                    " ".to_string()
                                } else {
                                    truncate(line, 110)
                                })
                                .into_any_element()
                        })
                        .collect::<Vec<_>>(),
                )
                .into_any_element(),
        );
    }
    if item.is_pr() {
        rows.push(
            div()
                .mt(px(12.0))
                .text_xs()
                .text_color(rgb(DIM))
                .child(format!("REVIEWS ({})", detail.reviews.len()))
                .into_any_element(),
        );
        if detail.reviews.is_empty() {
            rows.push(
                div()
                    .text_sm()
                    .text_color(rgb(DIM))
                    .child("no reviews")
                    .into_any_element(),
            );
        }
        for review in &detail.reviews {
            let (label, color) = review_badge(&review.state);
            let login = review
                .user
                .as_ref()
                .map(|u| u.login.clone())
                .unwrap_or_else(|| "?".to_string());
            let when = review.submitted_at.as_deref().map(ago).unwrap_or_default();
            rows.push(
                div()
                    .mt(px(4.0))
                    .text_sm()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .child(
                                div()
                                    .mr(px(8.0))
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .text_color(rgb(color))
                                    .child(dot(color))
                                    .child(label.clone()),
                            )
                            .child(
                                div()
                                    .text_color(rgb(DIM))
                                    .child(format!("@{login} {when}")),
                            ),
                    )
                    .children(
                        review
                            .body
                            .as_deref()
                            .unwrap_or("")
                            .lines()
                            .take(3)
                            .filter(|line| !line.trim().is_empty())
                            .map(|line| {
                                div()
                                    .text_xs()
                                    .text_color(rgb(DIM))
                                    .child(truncate(line, 110))
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>(),
                    )
                    .into_any_element(),
            );
        }
    }
    rows
}

/// Wire global key bindings + actions. Call once, outside any update.
pub fn bind_keys(inbox: &Entity<Inbox>, cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", OpenGithub, None),
        KeyBinding::new("r", Refresh, None),
        KeyBinding::new("c", AddComment, None),
        KeyBinding::new("x", CloseItem, None),
        KeyBinding::new("t", EditToken, None),
        KeyBinding::new("j", MoveDown, None),
        KeyBinding::new("down", MoveDown, None),
        KeyBinding::new("k", MoveUp, None),
        KeyBinding::new("up", MoveUp, None),
        KeyBinding::new("1", ShowOpen, None),
        KeyBinding::new("2", ShowMerged, None),
        KeyBinding::new("3", ShowClosed, None),
        KeyBinding::new("4", ShowStale, None),
    ]);

    // While the DOM overlay is open, keystrokes go to the overlay input:
    // swallow every canvas shortcut so typing never triggers actions.
    let inapplicable = || overlay::is_open();

    cx.on_action({
        let inbox = inbox.clone();
        move |_: &OpenGithub, cx: &mut App| {
            if inapplicable() {
                return;
            }
            let inbox = inbox.clone();
            cx.spawn(async move |cx| {
                Inbox::open_selected(&inbox, cx);
            }).detach();
        }
    });
    cx.on_action({
        let inbox = inbox.clone();
        move |_: &Refresh, cx: &mut App| {
            if inapplicable() {
                return;
            }
            let inbox = inbox.clone();
            cx.spawn(async move |cx| {
                Inbox::start_refresh(&inbox, cx);
            }).detach();
        }
    });
    cx.on_action({
        let inbox = inbox.clone();
        move |_: &AddComment, cx: &mut App| {
            if inapplicable() {
                return;
            }
            let inbox = inbox.clone();
            cx.spawn(async move |cx| {
                Inbox::start_comment(&inbox, cx);
            }).detach();
        }
    });
    cx.on_action({
        let inbox = inbox.clone();
        move |_: &CloseItem, cx: &mut App| {
            if inapplicable() {
                return;
            }
            let inbox = inbox.clone();
            cx.spawn(async move |cx| {
                Inbox::start_close(&inbox, cx);
            }).detach();
        }
    });
    cx.on_action({
        let inbox = inbox.clone();
        move |_: &EditToken, cx: &mut App| {
            if inapplicable() {
                return;
            }
            let inbox = inbox.clone();
            cx.spawn(async move |cx| {
                Inbox::start_token_editor(&inbox, cx);
            }).detach();
        }
    });
    cx.on_action({
        let inbox = inbox.clone();
        move |_: &MoveDown, cx: &mut App| {
            if inapplicable() {
                return;
            }
            let inbox = inbox.clone();
            cx.spawn(async move |cx| {
                Inbox::move_selection(&inbox, cx, 1);
            }).detach();
        }
    });
    cx.on_action({
        let inbox = inbox.clone();
        move |_: &MoveUp, cx: &mut App| {
            if inapplicable() {
                return;
            }
            let inbox = inbox.clone();
            cx.spawn(async move |cx| {
                Inbox::move_selection(&inbox, cx, -1);
            }).detach();
        }
    });
    cx.on_action({
        let inbox = inbox.clone();
        move |_: &ShowOpen, cx: &mut App| {
            if inapplicable() {
                return;
            }
            let inbox = inbox.clone();
            cx.spawn(async move |cx| {
                Inbox::set_filter(&inbox, cx, Filter::Open);
            }).detach();
        }
    });
    cx.on_action({
        let inbox = inbox.clone();
        move |_: &ShowMerged, cx: &mut App| {
            if inapplicable() {
                return;
            }
            let inbox = inbox.clone();
            cx.spawn(async move |cx| {
                Inbox::set_filter(&inbox, cx, Filter::Merged);
            }).detach();
        }
    });
    cx.on_action({
        let inbox = inbox.clone();
        move |_: &ShowClosed, cx: &mut App| {
            if inapplicable() {
                return;
            }
            let inbox = inbox.clone();
            cx.spawn(async move |cx| {
                Inbox::set_filter(&inbox, cx, Filter::Closed);
            }).detach();
        }
    });
    cx.on_action({
        let inbox = inbox.clone();
        move |_: &ShowStale, cx: &mut App| {
            if inapplicable() {
                return;
            }
            let inbox = inbox.clone();
            cx.spawn(async move |cx| {
                Inbox::set_filter(&inbox, cx, Filter::Stale);
            }).detach();
        }
    });
}
