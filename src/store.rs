//! localStorage-backed persistence: token, login, seen markers.

use std::collections::HashMap;

const KEY_TOKEN: &str = "contrib-inbox.token";
const KEY_LOGIN: &str = "contrib-inbox.login";
const KEY_SEEN: &str = "contrib-inbox.seen";

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

pub fn load_token() -> Option<String> {
    storage()?.get_item(KEY_TOKEN).ok()?
}

pub fn save_token(token: &str) {
    if let Some(store) = storage() {
        let _ = store.set_item(KEY_TOKEN, token);
    }
}

pub fn clear_token() {
    if let Some(store) = storage() {
        let _ = store.remove_item(KEY_TOKEN);
    }
}

pub fn load_login() -> Option<String> {
    storage()?.get_item(KEY_LOGIN).ok()?
}

pub fn save_login(login: &str) {
    if let Some(store) = storage() {
        let _ = store.set_item(KEY_LOGIN, login);
    }
}

/// item id -> updated_at of the last version the user looked at.
pub fn load_seen() -> HashMap<String, String> {
    let text = storage()
        .and_then(|s| s.get_item(KEY_SEEN).ok())
        .flatten()
        .unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save_seen(seen: &HashMap<String, String>) {
    if let Some(store) = storage() {
        if let Ok(text) = serde_json::to_string(seen) {
            let _ = store.set_item(KEY_SEEN, &text);
        }
    }
}

pub fn open_github(url: &str) {
    let window = match web_sys::window() {
        Some(window) => window,
        None => return,
    };
    let _ = window.open_with_url_and_target_and_features(url, "_blank", "noopener");
}
