use anyhow::{anyhow, Context, Result};
use rodio::{Decoder, DeviceSinkBuilder, Player};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::io::{Cursor, Read};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const API_VERSION: &str = "5.207";
const API_HOST: &str = "https://api.vk.ru/method/";
const TRACK_LIMIT: usize = 20;
const PLAYLIST_LIMIT: usize = 20;
const SEARCH_LIMIT: usize = 10;
const PLAYLIST_TRACK_LIMIT: usize = 10;
const PROBE_TRACK_LIMIT: usize = 2;
const REQUEST_TIMEOUT_SECS: u64 = 20;
const PROBE_BYTES: usize = 16;
const PLAY_DEFAULT_SECONDS: u64 = 20;
const PLAY_MAX_BYTES: usize = 40 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct VkResponse<T> {
    response: Option<T>,
    error: Option<VkError>,
}

#[derive(Debug, Deserialize)]
struct AudioGetResponse {
    count: Option<u64>,
    items: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
struct PlaylistsGetResponse {
    #[allow(dead_code)]
    count: Option<u64>,
    items: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
struct VkError {
    error_code: Option<i64>,
    error_msg: Option<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {}", sanitize_output(&format!("{err:#}")));
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let shape_mode = args.iter().any(|arg| arg == "--shape");
    let playlists_mode = args.iter().any(|arg| arg == "--playlists");
    let probe_mode = args.iter().any(|arg| arg == "--probe");
    let probe_bytes_mode = args.iter().any(|arg| arg == "--probe-bytes");
    let search_query = arg_value(&args, "--search");
    let playlist_index = arg_value(&args, "--playlist");
    let play_index = arg_value(&args, "--play");
    let play_seconds = arg_value(&args, "--seconds");

    let token = env::var("VK_TOKEN")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| anyhow!("VK_TOKEN is not set or empty"))?;
    let user_agent = env::var("VK_UA")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| anyhow!("VK_UA is not set or empty"))?;

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .user_agent(user_agent)
        .build()
        .context("failed to build HTTP client")?;

    if playlists_mode {
        run_playlists(&client, &token)?;
        return Ok(());
    }
    if probe_mode {
        run_probe(&client, &token)?;
        return Ok(());
    }
    if probe_bytes_mode {
        run_probe_bytes(&client, &token)?;
        return Ok(());
    }
    if let Some(query) = search_query {
        run_search(&client, &token, &query)?;
        return Ok(());
    }
    if let Some(index_raw) = playlist_index {
        run_playlist_tracks(&client, &token, &index_raw)?;
        return Ok(());
    }
    if let Some(index_raw) = play_index {
        let seconds = match play_seconds {
            Some(raw) => parse_play_seconds(&raw)?,
            None => PLAY_DEFAULT_SECONDS,
        };
        run_play(&client, &token, &index_raw, seconds)?;
        return Ok(());
    }

    let response: AudioGetResponse = vk_call(
        &client,
        &token,
        "audio.get",
        &[("count", TRACK_LIMIT.to_string().as_str())],
    )?;
    let count = response.count;
    let items = response.items.unwrap_or_default();
    if shape_mode {
        print_shape(&items)?;
        return Ok(());
    }
    print_tracks(count, &items);
    Ok(())
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == flag {
            return iter.next().cloned();
        }
    }
    None
}

fn run_playlists(client: &reqwest::blocking::Client, token: &str) -> Result<()> {
    let items = fetch_playlists(client, token)?;
    for item in items.iter().take(PLAYLIST_LIMIT) {
        let title = string_field(item, "title");
        let track_count = count_field(item);
        let keys = object_keys(item);
        println!("{title} | tracks: {track_count} | keys: {keys}");
    }
    Ok(())
}

fn fetch_playlists(client: &reqwest::blocking::Client, token: &str) -> Result<Vec<Value>> {
    let users: Vec<Value> = vk_call(client, token, "users.get", &[])?;
    let user = users
        .first()
        .ok_or_else(|| anyhow!("users.get returned no user"))?;
    let user_id = user
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("users.get response missing numeric id"))?;
    let owner_id = user_id.to_string();
    let count = PLAYLIST_LIMIT.to_string();
    let response: PlaylistsGetResponse = vk_call(
        client,
        token,
        "audio.getPlaylists",
        &[("owner_id", owner_id.as_str()), ("count", count.as_str())],
    )?;
    Ok(response.items.unwrap_or_default())
}

fn run_probe(client: &reqwest::blocking::Client, token: &str) -> Result<()> {
    println!("Classifier: m3u8 if url contains \".m3u8\" (case-insensitive); mp3/other if url contains \".mp3\"; otherwise mp3/other; no url if url missing/empty/not string.");
    let response: AudioGetResponse = vk_call(
        client,
        &token,
        "audio.get",
        &[("count", TRACK_LIMIT.to_string().as_str())],
    )?;
    let items = response.items.unwrap_or_default();
    let mut combos: BTreeMap<String, usize> = BTreeMap::new();
    for (idx, item) in items.iter().take(TRACK_LIMIT).enumerate() {
        let artist = string_field(item, "artist");
        let title = string_field(item, "title");
        let kind = url_kind(item);
        let url_info = url_shape(item.get("url"));
        let streams_info = audio_streams_shape(item.get("audio_streams"));
        let combo = format!("kind={kind},url_m3u8={},streams={}", url_info.contains_m3u8, streams_info.count);
        *combos.entry(combo).or_insert(0) += 1;
        println!("Track {}: {} - {}", idx + 1, artist, title);
        println!("  kind: {kind}");
        println!("  url.empty: {}", url_info.empty);
        println!("  url.contains_m3u8: {}", url_info.contains_m3u8);
        println!("  url.host: {}", url_info.host);
        println!("  url.last_path_ext: {}", url_info.last_path_ext);
        println!("  url.query_params: {}", url_info.query_params);
        println!("  url.length: {}", url_info.length);
        println!("  audio_streams.count: {}", streams_info.count);
        for (sidx, stream) in streams_info.items.iter().enumerate() {
            println!("  audio_streams[{sidx}].type: {}", stream.type_value);
            println!("  audio_streams[{sidx}].url_empty: {}", stream.url_empty);
            println!("  audio_streams[{sidx}].contains_m3u8: {}", stream.contains_m3u8);
            println!("  audio_streams[{sidx}].fallback_url_empty: {}", stream.fallback_url_empty);
            println!("  audio_streams[{sidx}].fallback_equals_url: {}", stream.fallback_equals_url);
        }
    }
    println!("Summary:");
    for (combo, count) in combos {
        println!("  {combo} => {count}");
    }
    Ok(())
}

fn run_probe_bytes(client: &reqwest::blocking::Client, token: &str) -> Result<()> {
    let response: AudioGetResponse = vk_call(
        client,
        token,
        "audio.get",
        &[("count", PROBE_TRACK_LIMIT.to_string().as_str())],
    )?;
    let items = response.items.unwrap_or_default();
    for (idx, item) in items.iter().take(PROBE_TRACK_LIMIT).enumerate() {
        let track_n = idx + 1;
        let artist = string_field(item, "artist");
        let title = string_field(item, "title");
        println!("track {track_n} | meta | {artist} - {title}");
        print_expires_remaining(item);

        if let Some(url) = item.get("url").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            probe_one(client, track_n, "main", url);
        } else {
            println!("track {track_n} | main | request failed (missing url)");
        }

        if let Some(streams) = item.get("audio_streams").and_then(Value::as_array) {
            for stream in streams {
                let stype = stream
                    .get("type")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("<missing>");
                if let Some(url) = stream.get("url").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    probe_one(client, track_n, &format!("{stype} url"), url);
                } else {
                    println!("track {track_n} | {stype} url | request failed (missing url)");
                }
                if let Some(fb) = stream.get("fallback_url").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    probe_one(client, track_n, &format!("{stype} fallback"), fb);
                } else {
                    println!("track {track_n} | {stype} fallback | request failed (missing url)");
                }
            }
        } else {
            println!("track {track_n} | audio_streams | request failed (missing audio_streams)");
        }
    }
    Ok(())
}

fn print_expires_remaining(item: &Value) {
    let expires = item
        .get("url")
        .and_then(Value::as_str)
        .and_then(|u| reqwest::Url::parse(u).ok())
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "expires")
                .map(|(_, v)| v.to_string())
        });
    let Some(raw) = expires else {
        println!("  expires_hours_remaining: <missing>");
        return;
    };
    let Ok(mut secs) = raw.parse::<i64>() else {
        println!("  expires_hours_remaining: <invalid>");
        return;
    };
    if secs > 1_000_000_000_000 {
        secs /= 1000;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let hours = ((secs - now) as f64 / 3600.0 * 100.0).round() / 100.0;
    println!("  expires_hours_remaining: {hours}");
}

fn probe_one(client: &reqwest::blocking::Client, track_n: usize, source: &str, url: &str) {
    let resp = match client.get(url).header(reqwest::header::RANGE, "bytes=0-15").send() {
        Ok(r) => r,
        Err(e) => {
            println!("track {track_n} | {source} | {}", sanitize_output(&short_reqwest_error(&e)));
            return;
        }
    };
    let status = resp.status().as_u16();
    let content_type = header_str(resp.headers(), "content-type");
    let content_length = header_str(resp.headers(), "content-length");
    let content_range = header_str(resp.headers(), "content-range");
    let accept_ranges = header_str(resp.headers(), "accept-ranges");
    let final_host = resp
        .url()
        .host_str()
        .unwrap_or("<missing>")
        .to_string();
    let mut buf = [0u8; PROBE_BYTES];
    let mut read_total = 0usize;
    let mut body = resp;
    loop {
        if read_total >= PROBE_BYTES {
            break;
        }
        match body.read(&mut buf[read_total..PROBE_BYTES]) {
            Ok(0) => break,
            Ok(n) => read_total += n,
            Err(e) => {
                println!(
                    "track {track_n} | {source} | {status} | {content_type} | {content_length} | {content_range} | {accept_ranges} | {final_host} | <read-error:{}> | unknown",
                    sanitize_output(&e.to_string())
                );
                return;
            }
        }
    }
    let hex = bytes_hex(&buf[..read_total]);
    let guess = format_guess(&buf[..read_total]);
    println!(
        "track {track_n} | {source} | {status} | {content_type} | {content_length} | {content_range} | {accept_ranges} | {final_host} | {hex} | {guess}"
    );
}

fn header_str(headers: &reqwest::header::HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .unwrap_or("<missing>")
        .to_string()
}

fn bytes_hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "<empty>".to_string();
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

fn format_guess(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"ID3") {
        return "mp3+id3";
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF {
        let b1 = bytes[1];
        if (b1 & 0xE0) == 0xE0 {
            if b1 == 0xF1 || b1 == 0xF9 {
                return "aac adts";
            }
            if b1 == 0xFB || b1 == 0xF3 || b1 == 0xF2 || b1 == 0xFA {
                return "mpeg audio frame";
            }
            return "mpeg audio frame";
        }
    }
    if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" {
        return "mp4/m4a";
    }
    if bytes.starts_with(b"fLaC") {
        return "flac";
    }
    if bytes.starts_with(b"OggS") {
        return "ogg";
    }
    if bytes.starts_with(b"#EXTM3U") {
        return "hls playlist";
    }
    if bytes.first() == Some(&0x47) {
        return "mpeg-ts";
    }
    if bytes.first() == Some(&b'<') {
        return "xml";
    }
    "unknown"
}

fn short_reqwest_error(err: &reqwest::Error) -> String {
    if let Some(status) = err.status() {
        format!("request failed (status {})", status.as_u16())
    } else if err.is_timeout() {
        "request failed (timeout)".to_string()
    } else if err.is_connect() {
        "request failed (connect)".to_string()
    } else if err.is_decode() {
        "request failed (decode)".to_string()
    } else {
        "request failed".to_string()
    }
}

fn run_search(client: &reqwest::blocking::Client, token: &str, query: &str) -> Result<()> {
    let count = SEARCH_LIMIT.to_string();
    let response: AudioGetResponse = vk_call(
        client,
        token,
        "audio.search",
        &[("q", query), ("count", count.as_str())],
    )?;
    let items = response.items.unwrap_or_default();
    for item in items.iter().take(SEARCH_LIMIT) {
        let artist = string_field(item, "artist");
        let title = string_field(item, "title");
        let duration = duration_field(item, "duration");
        let kind = url_kind(item);
        println!("{artist} - {title} [{duration}] ({kind})");
    }
    Ok(())
}

fn run_playlist_tracks(client: &reqwest::blocking::Client, token: &str, index_raw: &str) -> Result<()> {
    let index: usize = index_raw.parse().map_err(|_| anyhow!("invalid playlist index"))?;
    if index == 0 {
        return Err(anyhow!("playlist index must start at 1"));
    }
    let playlists = fetch_playlists(client, token)?;
    let playlist = playlists
        .get(index - 1)
        .ok_or_else(|| anyhow!("playlist index out of range"))?;
    let owner_id = playlist
        .get("owner_id")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("playlist missing numeric owner_id"))?
        .to_string();
    let album_id = playlist
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("playlist missing numeric id"))?
        .to_string();
    let access_key = match playlist.get("access_key") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(Value::String(_)) => return Err(anyhow!("playlist has empty access_key")),
        Some(_) => return Err(anyhow!("playlist access_key is not a string")),
        None => String::new(),
    };
    let count = PLAYLIST_TRACK_LIMIT.to_string();
    let mut params: Vec<(&str, &str)> = vec![
        ("owner_id", owner_id.as_str()),
        ("album_id", album_id.as_str()),
        ("count", count.as_str()),
    ];
    if !access_key.is_empty() {
        params.push(("access_key", access_key.as_str()));
    }
    let response: AudioGetResponse = vk_call(client, token, "audio.get", &params)?;
    let items = response.items.unwrap_or_default();
    for item in items.iter().take(PLAYLIST_TRACK_LIMIT) {
        let artist = string_field(item, "artist");
        let title = string_field(item, "title");
        let duration = duration_field(item, "duration");
        let kind = url_kind(item);
        println!("{artist} - {title} [{duration}] ({kind})");
    }
    Ok(())
}

fn vk_call<T: for<'de> Deserialize<'de>>(
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
        let code = err.error_code.map(|c| c.to_string()).unwrap_or_else(|| "<missing>".to_string());
        let msg = err.error_msg.unwrap_or_else(|| "<missing>".to_string());
        return Err(anyhow!("VK API error {}: {}", code, sanitize_output(&msg)));
    }
    parsed
        .response
        .ok_or_else(|| anyhow!("VK API response missing 'response' field for {method}"))
}

fn map_reqwest_error(err: reqwest::Error) -> anyhow::Error {
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

fn print_tracks(count: Option<u64>, items: &[Value]) {
    match count {
        Some(c) => println!("Total: {c}"),
        None => println!("Total: <missing>"),
    }
    for item in items.iter().take(TRACK_LIMIT) {
        let artist = string_field(item, "artist");
        let title = string_field(item, "title");
        let duration = duration_field(item, "duration");
        let kind = url_kind(item);
        println!("{artist} - {title} [{duration}]  ({kind})");
    }
}

fn print_shape(items: &[Value]) -> Result<()> {
    let Some(first) = items.first() else {
        println!("No tracks in response; nothing to show shape for.");
        return Ok(());
    };
    let Some(obj) = first.as_object() else {
        println!("First track is not a JSON object: {}", value_type(first));
        return Ok(());
    };
    println!("First track shape:");
    let mut keys: Vec<&String> = obj.keys().collect();
    keys.sort();
    for key in keys {
        let value = &obj[key];
        println!("{}: {}", key, describe_shape(value));
    }
    Ok(())
}

struct UrlShape {
    empty: bool,
    contains_m3u8: bool,
    host: String,
    last_path_ext: String,
    query_params: String,
    length: usize,
}
struct StreamShape {
    type_value: String,
    url_empty: bool,
    contains_m3u8: bool,
    fallback_url_empty: bool,
    fallback_equals_url: bool,
}
struct StreamsShape {
    count: usize,
    items: Vec<StreamShape>,
}

fn url_shape(value: Option<&Value>) -> UrlShape {
    let Some(Value::String(url)) = value else {
        return UrlShape { empty: true, contains_m3u8: false, host: "<missing>".to_string(), last_path_ext: "<missing>".to_string(), query_params: "<missing>".to_string(), length: 0 };
    };
    if url.is_empty() {
        return UrlShape { empty: true, contains_m3u8: false, host: "<empty>".to_string(), last_path_ext: "<empty>".to_string(), query_params: "<empty>".to_string(), length: 0 };
    }
    let lower = url.to_ascii_lowercase();
    let parsed = reqwest::Url::parse(url).ok();
    let host = parsed.as_ref().and_then(|u| u.host_str().map(|h| h.to_string())).unwrap_or_else(|| "<unparseable>".to_string());
    let last_path_ext = parsed
        .as_ref()
        .and_then(|u| u.path_segments().and_then(|mut segs| segs.rfind(|s| !s.is_empty()).map(|s| s.to_string())))
        .and_then(|seg| seg.rsplit_once('.').map(|(_, ext)| ext.to_string()))
        .filter(|ext| !ext.is_empty())
        .unwrap_or_else(|| "<none>".to_string());
    let mut query_names: Vec<String> = parsed.as_ref().map(|u| u.query_pairs().map(|(k, _)| k.to_string()).collect()).unwrap_or_else(Vec::new);
    query_names.sort();
    query_names.dedup();
    let query_params = if query_names.is_empty() { "<none>".to_string() } else { query_names.join(",") };
    UrlShape { empty: false, contains_m3u8: lower.contains("m3u8"), host, last_path_ext, query_params, length: url.len() }
}

fn audio_streams_shape(value: Option<&Value>) -> StreamsShape {
    let Some(Value::Array(arr)) = value else {
        return StreamsShape { count: 0, items: Vec::new() };
    };
    let mut items = Vec::new();
    for stream in arr {
        let obj = stream.as_object();
        let type_value = obj.and_then(|o| o.get("type")).and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("<missing>").to_string();
        let url_str = obj.and_then(|o| o.get("url")).and_then(Value::as_str);
        let fallback_str = obj.and_then(|o| o.get("fallback_url")).and_then(Value::as_str);
        let url_empty = url_str.map(|s| s.is_empty()).unwrap_or(true);
        let fallback_url_empty = fallback_str.map(|s| s.is_empty()).unwrap_or(true);
        let contains_m3u8 = url_str.map(|s| s.to_ascii_lowercase().contains("m3u8")).unwrap_or(false);
        let fallback_equals_url = match (url_str, fallback_str) { (Some(u), Some(f)) => u == f, _ => false };
        items.push(StreamShape { type_value, url_empty, contains_m3u8, fallback_url_empty, fallback_equals_url });
    }
    StreamsShape { count: arr.len(), items }
}

fn describe_shape(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Number(n) => if n.is_i64() { "number(i64)".to_string() } else if n.is_u64() { "number(u64)".to_string() } else { "number(f64)".to_string() },
        Value::String(_) => "string".to_string(),
        Value::Array(arr) => if let Some(first) = arr.first() { format!("array<{}>", describe_shape(first)) } else { "array<empty>".to_string() },
        Value::Object(obj) => {
            let mut keys: Vec<&String> = obj.keys().collect();
            keys.sort();
            let inner = keys.into_iter().map(|k| format!("{k}: {}", describe_shape(&obj[k]))).collect::<Vec<_>>().join(", ");
            format!("object{{{inner}}}")
        }
    }
}

fn string_field(item: &Value, key: &str) -> String {
    match item.get(key) {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(Value::String(_)) => "<empty>".to_string(),
        Some(_) => "<invalid>".to_string(),
        None => "<missing>".to_string(),
    }
}

fn count_field(item: &Value) -> String {
    match item.get("count") {
        Some(Value::Number(n)) => n.to_string(),
        Some(_) => "<invalid>".to_string(),
        None => "<missing>".to_string(),
    }
}

fn object_keys(item: &Value) -> String {
    let Some(obj) = item.as_object() else { return "<not-object>".to_string(); };
    let mut keys: Vec<&String> = obj.keys().collect();
    keys.sort();
    keys.into_iter().map(String::as_str).collect::<Vec<_>>().join(",")
}

fn duration_field(item: &Value, key: &str) -> String {
    match item.get(key) {
        Some(Value::Number(n)) => {
            if let Some(secs) = n.as_u64() { format_mm_ss(secs) }
            else if let Some(secs) = n.as_i64() { if secs >= 0 { format_mm_ss(secs as u64) } else { "<invalid>".to_string() } }
            else { "<invalid>".to_string() }
        }
        Some(_) => "<invalid>".to_string(),
        None => "<missing>".to_string(),
    }
}

fn format_mm_ss(total_secs: u64) -> String {
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn url_kind(item: &Value) -> &'static str {
    let Some(url_value) = item.get("url") else { return "no url"; };
    let Some(url) = url_value.as_str() else { return "no url"; };
    let lower = url.to_ascii_lowercase();
    if lower.is_empty() { "no url" } else if lower.contains(".m3u8") { "m3u8" } else if lower.contains(".mp3") { "mp3/other" } else { "mp3/other" }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null", Value::Bool(_) => "bool", Value::Number(_) => "number",
        Value::String(_) => "string", Value::Array(_) => "array", Value::Object(_) => "object",
    }
}

fn sanitize_output(input: &str) -> String {
    let mut out = redact_urls(input);
    if let Ok(token) = env::var("VK_TOKEN") {
        let trimmed = token.trim();
        if !trimmed.is_empty() { out = out.replace(trimmed, "<redacted>"); }
    }
    out
}

fn redact_urls(input: &str) -> String {
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
                if b.is_whitespace() || matches!(b, '"' | '\'' | '<' | '>' | ')' | ']' | '}' | ',') { break; }
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

fn parse_play_seconds(raw: &str) -> Result<u64> {
    let secs: u64 = raw.parse().map_err(|_| anyhow!("invalid --seconds value"))?;
    if secs == 0 {
        return Err(anyhow!("--seconds must be greater than 0"));
    }
    Ok(secs)
}

fn run_play(client: &reqwest::blocking::Client, token: &str, index_raw: &str, seconds: u64) -> Result<()> {
    let index: usize = index_raw.parse().map_err(|_| anyhow!("invalid --play value"))?;
    if index == 0 {
        return Err(anyhow!("--play index must start at 1"));
    }
    let response: AudioGetResponse = vk_call(
        client,
        token,
        "audio.get",
        &[("count", TRACK_LIMIT.to_string().as_str())],
    )?;
    let items = response.items.unwrap_or_default();
    let item = items
        .get(index - 1)
        .ok_or_else(|| anyhow!("--play index out of range"))?;
    let artist = string_field(item, "artist");
    let title = string_field(item, "title");
    let url = select_play_url(client, item)?;
    let bytes = download_track_bytes(client, &url)?;
    play_bytes(bytes, &artist, &title, seconds)
}

fn select_play_url(client: &reqwest::blocking::Client, item: &Value) -> Result<String> {
    let main_url = item
        .get("url")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("no playable mp3"))?;
    match probe_first_bytes(client, main_url) {
        Ok(bytes) if is_mp3_bytes(&bytes) => Ok(main_url.to_string()),
        Ok(_) | Err(_) => {
            let streams = item
                .get("audio_streams")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("no playable mp3"))?;
            for stream in streams {
                let stype = stream.get("type").and_then(Value::as_str).unwrap_or("");
                if stype != "mp3" {
                    continue;
                }
                let url = stream
                    .get("url")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty());
                if let Some(url) = url {
                    return Ok(url.to_string());
                }
            }
            Err(anyhow!("no playable mp3"))
        }
    }
}

fn probe_first_bytes(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
    let mut resp = client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-15")
        .send()
        .map_err(map_reqwest_error)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("request failed (status {})", status.as_u16()));
    }
    let mut buf = [0u8; 16];
    let mut read_total = 0usize;
    loop {
        if read_total >= buf.len() {
            break;
        }
        let n = resp.read(&mut buf[read_total..]).map_err(|_| anyhow!("request failed (decode)"))?;
        if n == 0 {
            break;
        }
        read_total += n;
    }
    Ok(buf[..read_total].to_vec())
}

fn is_mp3_bytes(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"ID3") {
        return true;
    }
    bytes.len() >= 2 && bytes[0] == 0xFF && matches!(bytes[1], 0xE2..=0xFF)
}

fn download_track_bytes(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
    println!("downloading...");
    let start = Instant::now();
    let mut resp = client.get(url).send().map_err(map_reqwest_error)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("request failed (status {})", status.as_u16()));
    }
    if let Some(len) = resp.content_length() {
        if len as usize > PLAY_MAX_BYTES {
            return Err(anyhow!("request failed (decode)"));
        }
    }
    let mut buf: Vec<u8> = Vec::with_capacity(
        resp.content_length()
            .map(|l| (l as usize).min(PLAY_MAX_BYTES))
            .unwrap_or(0),
    );
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = resp.read(&mut chunk).map_err(|_| anyhow!("request failed (decode)"))?;
        if n == 0 {
            break;
        }
        if buf.len().saturating_add(n) > PLAY_MAX_BYTES {
            return Err(anyhow!("request failed (decode)"));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let elapsed = start.elapsed().as_secs_f64();
    let mb = buf.len() as f64 / (1024.0 * 1024.0);
    println!("{mb:.2} MB in {elapsed:.2}s");
    Ok(buf)
}

fn play_bytes(bytes: Vec<u8>, artist: &str, title: &str, seconds: u64) -> Result<()> {
    let mut stream = DeviceSinkBuilder::open_default_sink().map_err(|_| anyhow!("request failed (decode)"))?;
    stream.log_on_drop(false);
    let player = Player::connect_new(&stream.mixer());
    let decoder = Decoder::try_from(Cursor::new(bytes)).map_err(|_| anyhow!("request failed (decode)"))?;
    player.append(decoder);
    println!("playing: {artist} - {title}");
    let start = Instant::now();
    let mut next_report = Duration::from_secs(5);
    let limit = Duration::from_secs(seconds);
    loop {
        let elapsed = start.elapsed();
        if elapsed >= limit || player.empty() {
            break;
        }
        if elapsed >= next_report {
            let pos = player.get_pos().as_secs();
            println!("{pos}s");
            next_report += Duration::from_secs(5);
        }
        let remaining = limit.saturating_sub(elapsed);
        let sleep_for = std::cmp::min(Duration::from_millis(200), remaining);
        if sleep_for.is_zero() {
            break;
        }
        std::thread::sleep(sleep_for);
    }
    player.stop();
    println!("done");
    Ok(())
}