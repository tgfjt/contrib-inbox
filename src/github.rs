//! GitHub REST API types + fetch client (browser fetch, CORS mode).
//!
//! COEP: require-corp note: api.github.com is fetched with CORS mode, whose
//! responses carry `Access-Control-Allow-Origin`, so they are allowed through.

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, Response};

pub const API_BASE: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResponse {
    pub total_count: u64,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Label {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullRef {
    pub url: String,
    pub merged_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Item {
    pub id: u64,
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub html_url: String,
    pub repository_url: String,
    pub comments_url: String,
    pub updated_at: String,
    pub created_at: String,
    pub author_association: String,
    #[serde(default)]
    pub labels: Vec<Label>,
    #[serde(default)]
    pub comments: u64,
    pub draft: Option<bool>,
    pub pull_request: Option<PullRef>,
    pub user: Option<User>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Comment {
    pub id: u64,
    pub body: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub user: Option<User>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Review {
    pub id: u64,
    pub state: String,
    pub body: Option<String>,
    pub submitted_at: Option<String>,
    pub user: Option<User>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PrDetail {
    pub head: PrHead,
    #[serde(default)]
    pub mergeable_state: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PrHead {
    pub sha: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CheckRuns {
    pub total_count: u64,
    pub check_runs: Vec<CheckRun>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CheckRun {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CloseBody {
    state: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct CommentBody<'a> {
    body: &'a str,
}

impl Item {
    pub fn is_pr(&self) -> bool {
        self.pull_request.is_some()
    }

    pub fn is_merged(&self) -> bool {
        self.pull_request
            .as_ref()
            .and_then(|p| p.merged_at.as_ref())
            .is_some()
    }

    pub fn is_open(&self) -> bool {
        self.state == "open"
    }

    /// owner/repo parsed from repository_url.
    pub fn repo_full_name(&self) -> String {
        self.repository_url
            .strip_prefix(&format!("{API_BASE}/repos/"))
            .unwrap_or(&self.repository_url)
            .to_string()
    }

    pub fn repo_short(&self) -> String {
        let full = self.repo_full_name();
        match full.split_once('/') {
            Some((_, repo)) => repo.to_string(),
            None => full,
        }
    }
}

/// MEMBER / OWNER / COLLABORATOR are "own" repos: excluded from the inbox.
pub fn is_external(item: &Item) -> bool {
    !matches!(
        item.author_association.as_str(),
        "MEMBER" | "OWNER" | "COLLABORATOR"
    )
}

async fn request(
    method: &str,
    url: &str,
    token: &str,
    body: Option<String>,
) -> Result<String, String> {
    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let init = RequestInit::new();
    init.set_method(method);
    if let Some(text) = body.as_deref() {
        init.set_body(&wasm_bindgen::JsValue::from_str(text));
    }
    let req = Request::new_with_str_and_init(url, &init)
        .map_err(|e| format!("build request: {e:?}"))?;
    let headers = req.headers();
    headers
        .set("Accept", "application/vnd.github+json")
        .map_err(|e| format!("set header: {e:?}"))?;
    headers
        .set("Authorization", &format!("Bearer {token}"))
        .map_err(|e| format!("set header: {e:?}"))?;
    headers
        .set("X-GitHub-Api-Version", API_VERSION)
        .map_err(|e| format!("set header: {e:?}"))?;
    if body.is_some() {
        headers
            .set("Content-Type", "application/json")
            .map_err(|e| format!("set header: {e:?}"))?;
    }
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
        Err(format!("GitHub {status}: {}", shorten(&text)))
    }
}

fn shorten(text: &str) -> String {
    const MAX: usize = 200;
    if text.len() <= MAX {
        return text.to_string();
    }
    // Prefer the API error message if present.
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(message) = value.get("message").and_then(|m| m.as_str()) {
            return message.to_string();
        }
    }
    format!("{}…", &text[..MAX])
}

fn parse<T>(text: &str) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_str(text).map_err(|e| format!("decode: {e}"))
}

pub fn search_url(user: &str) -> String {
    let query = format!("author:{user} archived:false sort:updated-desc");
    format!(
        "{API_BASE}/search/issues?q={}&per_page=50",
        js_sys::encode_uri_component(&query).as_string().unwrap_or(query)
    )
}

pub async fn search(user: &str, token: &str) -> Result<SearchResponse, String> {
    parse(&request("GET", &search_url(user), token, None).await?)
}

pub async fn get_login(token: &str) -> Result<String, String> {
    let user: User = parse(&request("GET", &format!("{API_BASE}/user"), token, None).await?)?;
    Ok(user.login)
}

pub async fn comments(item: &Item, token: &str) -> Result<Vec<Comment>, String> {
    parse(&request("GET", &item.comments_url, token, None).await?)
}

pub async fn reviews(item: &Item, token: &str) -> Result<Vec<Review>, String> {
    let url = format!(
        "{API_BASE}/repos/{}/pulls/{}/reviews?per_page=30",
        item.repo_full_name(),
        item.number
    );
    parse(&request("GET", &url, token, None).await?)
}

pub async fn pr_detail(item: &Item, token: &str) -> Result<PrDetail, String> {
    let url = item
        .pull_request
        .as_ref()
        .map(|p| p.url.clone())
        .ok_or_else(|| "not a pull request".to_string())?;
    parse(&request("GET", &url, token, None).await?)
}

pub async fn check_runs(
    repo: &str,
    sha: &str,
    token: &str,
) -> Result<CheckRuns, String> {
    let url = format!("{API_BASE}/repos/{repo}/commits/{sha}/check-runs?per_page=30");
    parse(&request("GET", &url, token, None).await?)
}

pub async fn post_comment(
    item: &Item,
    body: &str,
    token: &str,
) -> Result<(), String> {
    let url = format!(
        "{API_BASE}/repos/{}/issues/{}/comments",
        item.repo_full_name(),
        item.number
    );
    let payload = serde_json::to_string(&CommentBody { body })
        .map_err(|e| format!("encode: {e}"))?;
    request("POST", &url, token, Some(payload)).await?;
    Ok(())
}

pub async fn close_item(item: &Item, token: &str) -> Result<(), String> {
    let url = format!(
        "{API_BASE}/repos/{}/issues/{}",
        item.repo_full_name(),
        item.number
    );
    let payload = serde_json::to_string(&CloseBody { state: "closed" })
        .map_err(|e| format!("encode: {e}"))?;
    request("PATCH", &url, token, Some(payload)).await?;
    Ok(())
}
