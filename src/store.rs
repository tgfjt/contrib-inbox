//! localStorage-backed persistence: token, login, seen markers, OAuth client_id.

use std::collections::HashMap;

const KEY_TOKEN: &str = "contrib-inbox.token";
const KEY_LOGIN: &str = "contrib-inbox.login";
const KEY_SEEN: &str = "contrib-inbox.seen";
const KEY_CLIENT_ID: &str = "contrib-inbox.client-id";
const KEY_CLIENT_SECRET: &str = "contrib-inbox.client-secret";
const KEY_OAUTH_STATE: &str = "contrib-inbox.oauth-state";

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

pub fn clear_login() {
    if let Some(store) = storage() {
        let _ = store.remove_item(KEY_LOGIN);
    }
}

pub fn load_client_id() -> Option<String> {
    storage()?.get_item(KEY_CLIENT_ID).ok()?
}

pub fn save_client_id(client_id: &str) {
    if let Some(store) = storage() {
        let _ = store.set_item(KEY_CLIENT_ID, client_id);
    }
}

pub fn load_client_secret() -> Option<String> {
    storage()?.get_item(KEY_CLIENT_SECRET).ok()?
}

pub fn save_client_secret(secret: &str) {
    if let Some(store) = storage() {
        let _ = store.set_item(KEY_CLIENT_SECRET, secret);
    }
}

/// Pending web-flow login: CSRF state + PKCE verifier, saved before redirect.
pub fn save_oauth_state(state: &str, verifier: &str) {
    if let Some(store) = storage() {
        let value = serde_json::json!({"state": state, "verifier": verifier}).to_string();
        let _ = store.set_item(KEY_OAUTH_STATE, &value);
    }
}

pub fn take_oauth_state() -> Option<(String, String)> {
    let store = storage()?;
    let text = store.get_item(KEY_OAUTH_STATE).ok()??;
    let _ = store.remove_item(KEY_OAUTH_STATE);
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some((
        value.get("state")?.as_str()?.to_string(),
        value.get("verifier")?.as_str()?.to_string(),
    ))
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
