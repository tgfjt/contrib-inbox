//! GitHub OAuth Device Flow client.
//!
//! GitHub's OAuth endpoints send no CORS headers, so a backend-less PWA
//! cannot call them directly. Every request here goes through the
//! same-origin relay (`/gh-oauth`, trunk dev-proxy → https://github.com),
//! which keeps browser requests same-origin and COEP-clean.
//! Device Flow needs no client_secret — only the public OAuth App client_id.

use serde::Deserialize;
use base64::Engine as _;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, Response};

/// Same-origin relay prefix (see Trunk.toml `[[proxy]]`).
pub const OAUTH_BASE: &str = "/gh-oauth";

/// Callback URL for the web flow, derived from the current origin so any
/// trunk port works (register each origin in the OAuth App settings).
pub fn redirect_uri() -> Option<String> {
    let origin = web_sys::window()?.location().origin().ok()?;
    Some(format!("{origin}/auth/callback"))
}

/// Requested scopes for the OAuth App token.
const SCOPE: &str = "repo";

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default = "default_expires")]
    pub expires_in: u64,
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_expires() -> u64 {
    900
}

fn default_interval() -> u64 {
    5
}

#[derive(Debug, Clone, Deserialize)]
struct TokenReply {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub enum PollOutcome {
    Ready(String),
    Pending,
    SlowDown,
    Expired,
    Denied(String),
}

fn enc(text: &str) -> String {
    js_sys::encode_uri_component(text)
        .as_string()
        .unwrap_or_else(|| text.to_string())
}

/// Random hex string (CSRF state / PKCE verifier material).
pub fn random_hex(bytes: usize) -> String {
    (0..bytes)
        .map(|_| format!("{:02x}", (js_sys::Math::random() * 256.0) as u8))
        .collect()
}

/// PKCE S256 challenge for a verifier.
pub fn pkce_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// Step 1 of the web flow: URL to navigate the browser to.
pub fn authorize_url(client_id: &str, redirect_uri: &str, state: &str, challenge: &str) -> String {
    format!(
        "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        enc(client_id),
        enc(redirect_uri),
        enc(SCOPE),
        enc(state),
        enc(challenge),
    )
}

/// Step 2 of the web flow: exchange `code` for a token.
/// Tries PKCE-only first; pass the client_secret if GitHub demands it.
pub async fn exchange_code(
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
    secret: Option<&str>,
) -> Result<String, String> {
    let mut params: Vec<(String, String)> = vec![
        ("client_id".to_string(), client_id.to_string()),
        ("code".to_string(), code.to_string()),
        ("redirect_uri".to_string(), redirect_uri.to_string()),
        ("code_verifier".to_string(), verifier.to_string()),
    ];
    if let Some(secret) = secret.filter(|s| !s.trim().is_empty()) {
        params.push(("client_secret".to_string(), secret.to_string()));
    }
    let body = params
        .iter()
        .map(|(key, value)| format!("{}={}", enc(key), enc(value)))
        .collect::<Vec<_>>()
        .join("&");
    let text = post_form("/login/oauth/access_token", &body).await?;
    let reply: TokenReply =
        serde_json::from_str(&text).map_err(|e| format!("decode token: {e}"))?;
    if let Some(token) = reply.access_token {
        return Ok(token);
    }
    let error = reply.error.unwrap_or_else(|| "exchange_failed".to_string());
    let desc = reply.error_description.unwrap_or_default();
    Err(if desc.is_empty() {
        error
    } else {
        format!("{error}: {desc}")
    })
}

fn form(params: &[(&str, &str)]) -> String {
    params
        .iter()
        .map(|(key, value)| format!("{}={}", enc(key), enc(value)))
        .collect::<Vec<_>>()
        .join("&")
}

async fn post_form(path: &str, body: &str) -> Result<String, String> {
    let url = format!("{OAUTH_BASE}{path}");
    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_body(&wasm_bindgen::JsValue::from_str(body));
    let req = Request::new_with_str_and_init(&url, &init)
        .map_err(|e| format!("build request: {e:?}"))?;
    let headers = req.headers();
    headers
        .set("Accept", "application/json")
        .map_err(|e| format!("set header: {e:?}"))?;
    headers
        .set("Content-Type", "application/x-www-form-urlencoded")
        .map_err(|e| format!("set header: {e:?}"))?;
    let promise = window
        .fetch_with_request(&req)
        .dyn_into::<js_sys::Promise>()
        .map_err(|e| format!("fetch: {e:?}"))?;
    let value = JsFuture::from(promise)
        .await
        .map_err(|e| format!("network: {e:?}"))?;
    let resp: Response = value.dyn_into().map_err(|e| format!("response: {e:?}"))?;
    let status = resp.status();
    let text_promise = resp.text().map_err(|e| format!("body: {e:?}"))?;
    let text_value = JsFuture::from(text_promise)
        .await
        .map_err(|e| format!("body: {e:?}"))?;
    let text = text_value.as_string().unwrap_or_default();
    if (200..300).contains(&status) {
        Ok(text)
    } else {
        Err(format!("oauth {status}: {}", describe_error(&text)))
    }
}

fn describe_error(text: &str) -> String {
    if text.trim().is_empty() {
        return "empty response (check the /gh-oauth relay and client_id)".to_string();
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        let error = value.get("error").and_then(|e| e.as_str()).unwrap_or("");
        let desc = value
            .get("error_description")
            .and_then(|d| d.as_str())
            .unwrap_or("");
        if !error.is_empty() {
            return format!("{error}: {desc}");
        }
        if let Some(message) = value.get("message").and_then(|m| m.as_str()) {
            return message.to_string();
        }
    }
    const MAX: usize = 160;
    if text.len() <= MAX {
        text.to_string()
    } else {
        format!("{}…", &text[..MAX])
    }
}

pub async fn request_device_code(client_id: &str) -> Result<DeviceCode, String> {
    let body = form(&[("client_id", client_id), ("scope", SCOPE)]);
    let text = post_form("/login/device/code", &body).await?;
    serde_json::from_str(&text).map_err(|e| format!("decode device code: {e}"))
}

pub async fn poll_token(client_id: &str, device_code: &str) -> Result<PollOutcome, String> {
    let body = form(&[
        ("client_id", client_id),
        ("device_code", device_code),
        (
            "grant_type",
            "urn:ietf:params:oauth:grant-type:device_code",
        ),
    ]);
    let text = post_form("/login/oauth/access_token", &body).await?;
    let reply: TokenReply =
        serde_json::from_str(&text).map_err(|e| format!("decode token: {e}"))?;
    if let Some(token) = reply.access_token {
        return Ok(PollOutcome::Ready(token));
    }
    let desc = reply.error_description.unwrap_or_default();
    match reply.error.as_deref() {
        Some("authorization_pending") => Ok(PollOutcome::Pending),
        Some("slow_down") => Ok(PollOutcome::SlowDown),
        Some("expired_token") => Ok(PollOutcome::Expired),
        Some("access_denied") => Ok(PollOutcome::Denied(desc)),
        Some(other) => Err(format!("oauth: {other}: {desc}")),
        None => Err("oauth: empty token response".to_string()),
    }
}

/// setTimeout-backed sleep (gpui executor timers are not relied upon here).
pub async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let window = match web_sys::window() {
            Some(window) => window,
            None => return,
        };
        let done = Closure::once(move || {
            let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
        });
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            done.as_ref().unchecked_ref(),
            ms,
        );
        done.forget();
    });
    let _ = JsFuture::from(promise).await;
}
