pub const API_VERSION: &str = "5.207";
pub const API_HOST: &str = "https://api.vk.ru/method/";
pub const REQUEST_TIMEOUT_SECS: u64 = 20;
pub const PLAY_MAX_BYTES: usize = 40 * 1024 * 1024;
pub const DEFAULT_ANDROID_UA: &str = "VKAndroidApp/8.154-99999 (Android 12; SDK 32; arm64-v8a; Pixel 6; ru; 2960x1440)";

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct VkResponse<T> {
    pub response: Option<T>,
    pub error: Option<VkError>,
}

#[derive(Debug, Deserialize)]
pub struct AudioGetResponse {
    pub count: Option<u64>,
    pub items: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
pub struct PlaylistsGetResponse {
    #[allow(dead_code)]
    pub count: Option<u64>,
    pub items: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
pub struct VkError {
    pub error_code: Option<i64>,
    pub error_msg: Option<String>,
}

pub fn is_auth_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        let msg = cause.to_string();
        msg.contains("VK API error 5:") || msg.contains("VK API error 15:")
    })
}

#[derive(Clone, Debug)]
pub struct Track {
    pub artist: String,
    pub title: String,
    pub duration_secs: Option<u64>,
    pub url: Option<String>,
    pub audio_streams: Option<Value>,
}

impl Track {
    pub fn from_value(item: &Value) -> Self {
        Self {
            artist: string_field(item, "artist"),
            title: string_field(item, "title"),
            duration_secs: duration_secs(item, "duration"),
            url: item.get("url").and_then(Value::as_str).map(|s| s.to_string()),
            audio_streams: item.get("audio_streams").cloned(),
        }
    }

    pub fn display_line(&self) -> String {
        let dur = self
            .duration_secs
            .map(format_mm_ss)
            .unwrap_or_else(|| "--:--".to_string());
        format!("{} - {}  {}", self.artist, self.title, dur)
    }

    pub fn is_playable(&self) -> bool {
        self.url.as_deref().map(|u| !u.trim().is_empty()).unwrap_or(false)
    }
}

#[derive(Clone, Debug)]
pub struct Playlist {
    pub owner_id: i64,
    pub id: i64,
    pub access_key: Option<String>,
    pub title: String,
    pub count: Option<u64>,
}

impl Playlist {
    pub fn from_value(item: &Value) -> Option<Self> {
        let owner_id = item.get("owner_id").and_then(Value::as_i64)?;
        let id = item.get("id").and_then(Value::as_i64)?;
        let title = string_field(item, "title");
        let count = match item.get("count") {
            Some(Value::Number(n)) => n.as_u64(),
            _ => None,
        };
        let access_key = match item.get("access_key") {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        };
        Some(Self { owner_id, id, access_key, title, count })
    }
}

pub fn build_client(user_agent: &str) -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .user_agent(user_agent)
        .build()
        .context("failed to build HTTP client")
}

pub fn vk_call<T: for<'de> Deserialize<'de>>(
    client: &reqwest::blocking::Client,
    token: &str,
    method: &str,
    params: &[(&str, &str)],
) -> Result<T> {
    let mut query: Vec<(&str, &str)> = Vec::with_capacity(params.len() + 2);
    query.push(("access_token", token));
    query.push(("v", API_VERSION));
    query.extend_from_slice(params);
    let url = reqwest::Url::parse_with_params(&format!("{API_HOST}{method}"), &query)
        .context("failed to build VK API URL")?;
    let response = client
        .get(url)
        .send()
        .map_err(map_reqwest_error)
        .with_context(|| format!("request failed: {method}"))?;
    let status = response.status();
    let body: Value = response
        .json()
        .map_err(map_reqwest_error)
        .with_context(|| format!("failed to parse JSON: {method}"))?;
    if !status.is_success() {
        return Err(anyhow!("request failed (status {})", status.as_u16()));
    }
    let parsed: VkResponse<T> = serde_json::from_value(body)
        .with_context(|| format!("failed to decode response: {method}"))?;
    if let Some(err) = parsed.error {
        let code = err
            .error_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "<missing>".to_string());
        let msg = err.error_msg.unwrap_or_else(|| "<missing>".to_string());
        return Err(anyhow!("VK API error {}: {}", code, sanitize_output(&msg)));
    }
    parsed
        .response
        .ok_or_else(|| anyhow!("VK API response missing 'response' field for {method}"))
}

pub fn map_reqwest_error(err: reqwest::Error) -> anyhow::Error {
    if let Some(status) = err.status() {
        anyhow!("request failed (status {})", status.as_u16())
    } else if err.is_timeout() {
        anyhow!("request failed (timeout)")
    } else if err.is_connect() {
        anyhow!("request failed (connect)")
    } else if err.is_decode() {
        anyhow!("request failed (decode)")
    } else {
        anyhow!("request failed")
    }
}

pub fn sanitize_output(input: &str) -> String {
    let mut out = redact_urls(input);
    if let Ok(token) = std::env::var("VK_TOKEN") {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            out = out.replace(trimmed, "<redacted>");
        }
    }
    out
}

pub fn redact_urls(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        let rest = &input[i..];
        let is_http = rest.starts_with("http://") || rest.starts_with("https://");
        if is_http {
            out.push_str("<url>");
            while i < bytes.len() {
                let b = bytes[i] as char;
                if b.is_whitespace() || matches!(b, '"' | '\'' | '<' | '>' | ')' | ']' | '}' | ',') {
                    break;
                }
                i += b.len_utf8();
            }
        } else {
            let ch = rest.chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

pub fn string_field(item: &Value, key: &str) -> String {
    match item.get(key) {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(Value::String(_)) => "<empty>".to_string(),
        Some(_) => "<invalid>".to_string(),
        None => "<missing>".to_string(),
    }
}

pub fn duration_secs(item: &Value, key: &str) -> Option<u64> {
    match item.get(key) {
        Some(Value::Number(n)) => {
            if let Some(secs) = n.as_u64() {
                Some(secs)
            } else if let Some(secs) = n.as_i64() {
                if secs >= 0 { Some(secs as u64) } else { None }
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn format_mm_ss(total_secs: u64) -> String {
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    format!("{minutes:02}:{seconds:02}")
}

pub fn extract_access_token(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(url) = reqwest::Url::parse(trimmed) {
        if let Some((_, v)) = url.query_pairs().find(|(k, _)| k == "access_token") {
            let t = v.to_string();
            if !t.is_empty() {
                return Some(t);
            }
        }
        if let Some(fragment) = url.fragment() {
            for part in fragment.split('&') {
                let mut kv = part.splitn(2, '=');
                if let (Some(k), Some(v)) = (kv.next(), kv.next()) {
                    if k == "access_token" && !v.is_empty() {
                        return Some(v.to_string());
                    }
                }
            }
        }
    }
    if trimmed.contains("access_token=") {
        for part in trimmed.split(['?', '#', '&']) {
            let mut kv = part.splitn(2, '=');
            if let (Some(k), Some(v)) = (kv.next(), kv.next()) {
                if k == "access_token" && !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    if !trimmed.chars().any(|c| c.is_whitespace()) && !trimmed.contains("://") {
        return Some(trimmed.to_string());
    }
    None
}
