use anyhow::{anyhow, Result};
use serde_json::Value;
use std::env;
use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod api;
mod gui;
mod player;
mod storage;

use api::{AudioGetResponse, PlaylistsGetResponse, Track};

const TRACK_LIMIT: usize = 20;
const PLAYLIST_LIMIT: usize = 20;
const SEARCH_LIMIT: usize = 10;
const PLAYLIST_TRACK_LIMIT: usize = 10;
const PROBE_TRACK_LIMIT: usize = 2;
const PROBE_BYTES: usize = 16;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {}", api::sanitize_output(&format!("{err:#}")));
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let gui_mode = args.iter().any(|a| a == "--gui");
    let shape_mode = args.iter().any(|arg| arg == "--shape");
    let playlists_mode = args.iter().any(|arg| arg == "--playlists");
    let probe_mode = args.iter().any(|arg| arg == "--probe");
    let probe_bytes_mode = args.iter().any(|arg| arg == "--probe-bytes");
    let probe_mix_mode = args.iter().any(|arg| arg == "--probe-mix");
    let search_query = arg_value(&args, "--search");
    let playlist_index = arg_value(&args, "--playlist");
    let play_index = arg_value(&args, "--play");
    let play_seconds = arg_value(&args, "--seconds");

    let cli_mode = shape_mode
        || playlists_mode
        || probe_mode
        || probe_bytes_mode
        || probe_mix_mode
        || search_query.is_some()
        || playlist_index.is_some()
        || play_index.is_some();

    if gui_mode || !cli_mode {
        let single_instance = try_single_instance_gui()?;
        if single_instance.is_none() {
            return Ok(());
        }
        let env_token = env::var("VK_TOKEN").ok().filter(|v| !v.trim().is_empty());
        let env_ua = env::var("VK_UA").ok().filter(|v| !v.trim().is_empty());
        gui::run_gui(env_token, env_ua, single_instance).map_err(|e| anyhow!("{}", e))?;
        return Ok(());
    }

    let token = env::var("VK_TOKEN")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| anyhow!("VK_TOKEN is not set or empty"))?;
    let user_agent = env::var("VK_UA")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| anyhow!("VK_UA is not set or empty"))?;

    let client = api::build_client(&user_agent)?;

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
    if probe_mix_mode {
        run_probe_mix(&client, &token)?;
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
        match play_seconds {
            Some(raw) => {
                let seconds = parse_play_seconds(&raw)?;
                run_play(&client, &token, &index_raw, Some(seconds))?;
            }
            None => {
                run_play(&client, &token, &index_raw, None)?;
            }
        }
        return Ok(());
    }

    let response: AudioGetResponse = api::vk_call(
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

const SINGLE_INSTANCE_PORT: u16 = 47613;

fn try_single_instance_gui() -> Result<Option<TcpListener>> {
    match TcpListener::bind(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
        Ok(listener) => Ok(Some(listener)),
        Err(_) => {
            if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
                let _ = stream.write_all(b"show\n");
                let _ = stream.flush();
            }
            Ok(None)
        }
    }
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
        let title = api::string_field(item, "title");
        let track_count = count_field(item);
        let keys = object_keys(item);
        println!("{title} | tracks: {track_count} | keys: {keys}");
    }
    Ok(())
}

fn fetch_playlists(client: &reqwest::blocking::Client, token: &str) -> Result<Vec<Value>> {
    let users: Vec<Value> = api::vk_call(client, token, "users.get", &[])?;
    let user = users.first().ok_or_else(|| anyhow!("users.get returned no user"))?;
    let user_id = user.get("id").and_then(Value::as_i64).ok_or_else(|| anyhow!("users.get response missing numeric id"))?;
    let owner_id = user_id.to_string();
    let count = PLAYLIST_LIMIT.to_string();
    let response: PlaylistsGetResponse = api::vk_call(
        client,
        token,
        "audio.getPlaylists",
        &[("owner_id", owner_id.as_str()), ("count", count.as_str())],
    )?;
    Ok(response.items.unwrap_or_default())
}

fn run_probe(client: &reqwest::blocking::Client, token: &str) -> Result<()> {
    println!("Classifier: m3u8 if url contains \".m3u8\" (case-insensitive); mp3/other if url contains \".mp3\"; otherwise mp3/other; no url if url missing/empty/not string.");
    let response: AudioGetResponse = api::vk_call(
        client,
        token,
        "audio.get",
        &[("count", TRACK_LIMIT.to_string().as_str())],
    )?;
    let items = response.items.unwrap_or_default();
    let mut combos: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for (idx, item) in items.iter().take(TRACK_LIMIT).enumerate() {
        let artist = api::string_field(item, "artist");
        let title = api::string_field(item, "title");
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
    let response: AudioGetResponse = api::vk_call(
        client,
        token,
        "audio.get",
        &[("count", PROBE_TRACK_LIMIT.to_string().as_str())],
    )?;
    let items = response.items.unwrap_or_default();
    for (idx, item) in items.iter().take(PROBE_TRACK_LIMIT).enumerate() {
        let track_n = idx + 1;
        let artist = api::string_field(item, "artist");
        let title = api::string_field(item, "title");
        println!("track {track_n} | meta | {artist} - {title}");
        print_expires_remaining(item);

        if let Some(url) = item.get("url").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            probe_one(client, track_n, "main", url);
        } else {
            println!("track {track_n} | main | request failed (missing url)");
        }

        if let Some(streams) = item.get("audio_streams").and_then(Value::as_array) {
            for stream in streams {
                let stype = stream.get("type").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("<missing>");
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

fn run_probe_mix(client: &reqwest::blocking::Client, token: &str) -> Result<()> {
    use std::collections::HashSet;

    #[cfg(windows)]
    unsafe {
        // Set console output to UTF-8 for readable Cyrillic in "artist - title".
        unsafe extern "system" {
            fn SetConsoleOutputCP(wCodePageID: u32) -> i32;
        }
        let _ = SetConsoleOutputCP(65001);
    }

    println!("Probe-mix: read-only audio.getRecommendations checks. No token/URLs printed.");
    println!("Source: vodka2/vk-audio-token src/examples/usage/example_kate.php shows audio.getRecommendations with count,v. dev.vk.com pages JS-rendered; no reliable extra params extracted. Not inventing params.");

    let base: AudioGetResponse = api::vk_call(
        client,
        token,
        "audio.get",
        &[("count", TRACK_LIMIT.to_string().as_str())],
    )?;
    let base_items = base.items.unwrap_or_default();
    let base_ids: HashSet<String> = base_items.iter().filter_map(track_id).collect();
    println!("audio.get first page: count={} ids={}", base_items.len(), base_ids.len());

    // 1) Three calls in a row, count=10, overlap 1-2,2-3,1-3.
    println!("--- three_calls_count_10 ---");
    let mut calls: Vec<Vec<Value>> = Vec::new();
    for i in 0..3 {
        let resp = api::vk_call::<Value>(client, token, "audio.getRecommendations", &[("count", "10")]);
        match resp {
            Ok(v) => {
                let items = extract_items(&v);
                println!("call{} ok=true items={}", i + 1, items.len());
                calls.push(items);
            }
            Err(e) => {
                let msg = api::sanitize_output(&format!("{e:#}"));
                let (code, emsg) = parse_vk_error_fields(&msg);
                println!("call{} ok=false error_code={} error_msg={}", i + 1, code, emsg);
                calls.push(Vec::new());
            }
        }
    }
    let ids0: HashSet<String> = calls.get(0).into_iter().flat_map(|v| v.iter()).filter_map(track_id).collect();
    let ids1: HashSet<String> = calls.get(1).into_iter().flat_map(|v| v.iter()).filter_map(track_id).collect();
    let ids2: HashSet<String> = calls.get(2).into_iter().flat_map(|v| v.iter()).filter_map(track_id).collect();
    println!("overlap_call1_call2: {}", ids0.intersection(&ids1).count());
    println!("overlap_call2_call3: {}", ids1.intersection(&ids2).count());
    println!("overlap_call1_call3: {}", ids0.intersection(&ids2).count());

    // 2) count=50, items count and unique ids.
    println!("--- count_50 ---");
    match api::vk_call::<Value>(client, token, "audio.getRecommendations", &[("count", "50")]) {
        Ok(v) => {
            let items = extract_items(&v);
            let unique: HashSet<String> = items.iter().filter_map(track_id).collect();
            println!("ok=true items={} unique_ids={}", items.len(), unique.len());
        }
        Err(e) => {
            let msg = api::sanitize_output(&format!("{e:#}"));
            let (code, emsg) = parse_vk_error_fields(&msg);
            println!("ok=false error_code={} error_msg={} items=0 unique_ids=0", code, emsg);
        }
    }

    // 3) count=10 offset=10, overlap with call 1.
    println!("--- count_10_offset_10 ---");
    match api::vk_call::<Value>(client, token, "audio.getRecommendations", &[("count", "10"), ("offset", "10")]) {
        Ok(v) => {
            let items = extract_items(&v);
            let off_ids: HashSet<String> = items.iter().filter_map(track_id).collect();
            println!("ok=true items={} overlap_with_call1={}", items.len(), off_ids.intersection(&ids0).count());
        }
        Err(e) => {
            let msg = api::sanitize_output(&format!("{e:#}"));
            let (code, emsg) = parse_vk_error_fields(&msg);
            println!("ok=false error_code={} error_msg={}", code, emsg);
        }
    }

    // 4) Top-level keys of response object (not only items).
    println!("--- response_top_level_keys ---");
    match api::vk_call::<Value>(client, token, "audio.getRecommendations", &[("count", "10")]) {
        Ok(v) => {
            match &v {
                Value::Object(obj) => {
                    let mut keys: Vec<&String> = obj.keys().collect();
                    keys.sort();
                    let joined = keys.into_iter().map(String::as_str).collect::<Vec<_>>().join(",");
                    println!("ok=true top_level_keys={}", joined);
                }
                Value::Array(_) => {
                    println!("ok=true top_level_keys=<array_response>");
                }
                _ => {
                    println!("ok=true top_level_keys=<non_object>");
                }
            }
            let items = extract_items(&v);
            for (i, item) in items.iter().take(5).enumerate() {
                let artist = api::string_field(item, "artist");
                let title = api::string_field(item, "title");
                println!("top{}: {} - {}", i + 1, artist, title);
            }
        }
        Err(e) => {
            let msg = api::sanitize_output(&format!("{e:#}"));
            let (code, emsg) = parse_vk_error_fields(&msg);
            println!("ok=false error_code={} error_msg={}", code, emsg);
        }
    }

    Ok(())
}

fn extract_items(resp: &Value) -> Vec<Value> {
    match resp {
        Value::Array(arr) => arr.clone(),
        Value::Object(obj) => match obj.get("items") {
            Some(Value::Array(arr)) => arr.clone(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn track_id(item: &Value) -> Option<String> {
    let owner = item.get("owner_id").and_then(Value::as_i64)?;
    let id = item.get("id").and_then(Value::as_i64)?;
    Some(format!("{owner}_{id}"))
}

fn parse_vk_error_fields(msg: &str) -> (String, String) {
    if let Some(rest) = msg.strip_prefix("VK API error ") {
        if let Some((code, emsg)) = rest.split_once(": ") {
            return (code.to_string(), emsg.to_string());
        }
        return (rest.to_string(), "<missing>".to_string());
    }
    ("<none>".to_string(), msg.to_string())
}

fn print_expires_remaining(item: &Value) {
    let expires = item
        .get("url")
        .and_then(Value::as_str)
        .and_then(|u| reqwest::Url::parse(u).ok())
        .and_then(|u| u.query_pairs().find(|(k, _)| k == "expires").map(|(_, v)| v.to_string()));
    let Some(raw) = expires else { println!("  expires_hours_remaining: <missing>"); return; };
    let Ok(mut secs) = raw.parse::<i64>() else { println!("  expires_hours_remaining: <invalid>"); return; };
    if secs > 1_000_000_000_000 { secs /= 1000; }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let hours = ((secs - now) as f64 / 3600.0 * 100.0).round() / 100.0;
    println!("  expires_hours_remaining: {hours}");
}

fn probe_one(client: &reqwest::blocking::Client, track_n: usize, source: &str, url: &str) {
    let resp = match client.get(url).header(reqwest::header::RANGE, "bytes=0-15").send() {
        Ok(r) => r,
        Err(e) => { println!("track {track_n} | {source} | {}", api::sanitize_output(&short_reqwest_error(&e))); return; }
    };
    let status = resp.status().as_u16();
    let content_type = header_str(resp.headers(), "content-type");
    let content_length = header_str(resp.headers(), "content-length");
    let content_range = header_str(resp.headers(), "content-range");
    let accept_ranges = header_str(resp.headers(), "accept-ranges");
    let final_host = resp.url().host_str().unwrap_or("<missing>").to_string();
    let mut buf = [0u8; PROBE_BYTES];
    let mut read_total = 0usize;
    let mut body = resp;
    loop {
        if read_total >= PROBE_BYTES { break; }
        match body.read(&mut buf[read_total..PROBE_BYTES]) {
            Ok(0) => break,
            Ok(n) => read_total += n,
            Err(e) => {
                println!("track {track_n} | {source} | {status} | {content_type} | {content_length} | {content_range} | {accept_ranges} | {final_host} | <read-error:{}> | unknown", api::sanitize_output(&e.to_string()));
                return;
            }
        }
    }
    let hex = bytes_hex(&buf[..read_total]);
    let guess = format_guess(&buf[..read_total]);
    println!("track {track_n} | {source} | {status} | {content_type} | {content_length} | {content_range} | {accept_ranges} | {final_host} | {hex} | {guess}");
}

fn header_str(headers: &reqwest::header::HeaderMap, name: &str) -> String {
    headers.get(name).and_then(|v| v.to_str().ok()).filter(|s| !s.is_empty()).unwrap_or("<missing>").to_string()
}

fn bytes_hex(bytes: &[u8]) -> String {
    if bytes.is_empty() { return "<empty>".to_string(); }
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

fn format_guess(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"ID3") { return "mp3+id3"; }
    if bytes.len() >= 2 && bytes[0] == 0xFF {
        let b1 = bytes[1];
        if (b1 & 0xE0) == 0xE0 {
            if b1 == 0xF1 || b1 == 0xF9 { return "aac adts"; }
            if b1 == 0xFB || b1 == 0xF3 || b1 == 0xF2 || b1 == 0xFA { return "mpeg audio frame"; }
            return "mpeg audio frame";
        }
    }
    if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" { return "mp4/m4a"; }
    if bytes.starts_with(b"fLaC") { return "flac"; }
    if bytes.starts_with(b"OggS") { return "ogg"; }
    if bytes.starts_with(b"#EXTM3U") { return "hls playlist"; }
    if bytes.first() == Some(&0x47) { return "mpeg-ts"; }
    if bytes.first() == Some(&b'<') { return "xml"; }
    "unknown"
}

fn short_reqwest_error(err: &reqwest::Error) -> String {
    if let Some(status) = err.status() { format!("request failed (status {})", status.as_u16()) }
    else if err.is_timeout() { "request failed (timeout)".to_string() }
    else if err.is_connect() { "request failed (connect)".to_string() }
    else if err.is_decode() { "request failed (decode)".to_string() }
    else { "request failed".to_string() }
}

fn run_search(client: &reqwest::blocking::Client, token: &str, query: &str) -> Result<()> {
    let count = SEARCH_LIMIT.to_string();
    let response: AudioGetResponse = api::vk_call(
        client, token, "audio.search", &[("q", query), ("count", count.as_str())],
    )?;
    for item in response.items.unwrap_or_default().iter().take(SEARCH_LIMIT) {
        let artist = api::string_field(item, "artist");
        let title = api::string_field(item, "title");
        let duration = duration_field(item, "duration");
        let kind = url_kind(item);
        println!("{artist} - {title} [{duration}] ({kind})");
    }
    Ok(())
}

fn run_playlist_tracks(client: &reqwest::blocking::Client, token: &str, index_raw: &str) -> Result<()> {
    let index: usize = index_raw.parse().map_err(|_| anyhow!("invalid playlist index"))?;
    if index == 0 { return Err(anyhow!("playlist index must start at 1")); }
    let playlists = fetch_playlists(client, token)?;
    let playlist = playlists.get(index - 1).ok_or_else(|| anyhow!("playlist index out of range"))?;
    let owner_id = playlist.get("owner_id").and_then(Value::as_i64).ok_or_else(|| anyhow!("playlist missing numeric owner_id"))?.to_string();
    let album_id = playlist.get("id").and_then(Value::as_i64).ok_or_else(|| anyhow!("playlist missing numeric id"))?.to_string();
    let access_key = match playlist.get("access_key") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(Value::String(_)) => return Err(anyhow!("playlist has empty access_key")),
        Some(_) => return Err(anyhow!("playlist access_key is not a string")),
        None => String::new(),
    };
    let count = PLAYLIST_TRACK_LIMIT.to_string();
    let mut params: Vec<(&str, &str)> = vec![("owner_id", owner_id.as_str()), ("album_id", album_id.as_str()), ("count", count.as_str())];
    if !access_key.is_empty() { params.push(("access_key", access_key.as_str())); }
    let response: AudioGetResponse = api::vk_call(client, token, "audio.get", &params)?;
    for item in response.items.unwrap_or_default().iter().take(PLAYLIST_TRACK_LIMIT) {
        let artist = api::string_field(item, "artist");
        let title = api::string_field(item, "title");
        let duration = duration_field(item, "duration");
        let kind = url_kind(item);
        println!("{artist} - {title} [{duration}] ({kind})");
    }
    Ok(())
}

fn print_tracks(count: Option<u64>, items: &[Value]) {
    match count { Some(c) => println!("Total: {c}"), None => println!("Total: <missing>") }
    for item in items.iter().take(TRACK_LIMIT) {
        let artist = api::string_field(item, "artist");
        let title = api::string_field(item, "title");
        let duration = duration_field(item, "duration");
        let kind = url_kind(item);
        println!("{artist} - {title} [{duration}]  ({kind})");
    }
}

fn print_shape(items: &[Value]) -> Result<()> {
    let Some(first) = items.first() else { println!("No tracks in response; nothing to show shape for."); return Ok(()); };
    let Some(obj) = first.as_object() else { println!("First track is not a JSON object: {}", value_type(first)); return Ok(()); };
    println!("First track shape:");
    let mut keys: Vec<&String> = obj.keys().collect();
    keys.sort();
    for key in keys { let value = &obj[key]; println!("{}: {}", key, describe_shape(value)); }
    Ok(())
}

struct UrlShape { empty: bool, contains_m3u8: bool, host: String, last_path_ext: String, query_params: String, length: usize }
struct StreamShape { type_value: String, url_empty: bool, contains_m3u8: bool, fallback_url_empty: bool, fallback_equals_url: bool }
struct StreamsShape { count: usize, items: Vec<StreamShape> }

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
    let last_path_ext = parsed.as_ref().and_then(|u| u.path_segments().and_then(|mut segs| segs.rfind(|s| !s.is_empty()).map(|s| s.to_string()))).and_then(|seg| seg.rsplit_once('.').map(|(_, ext)| ext.to_string())).filter(|ext| !ext.is_empty()).unwrap_or_else(|| "<none>".to_string());
    let mut query_names: Vec<String> = parsed.as_ref().map(|u| u.query_pairs().map(|(k, _)| k.to_string()).collect()).unwrap_or_else(Vec::new);
    query_names.sort(); query_names.dedup();
    let query_params = if query_names.is_empty() { "<none>".to_string() } else { query_names.join(",") };
    UrlShape { empty: false, contains_m3u8: lower.contains("m3u8"), host, last_path_ext, query_params, length: url.len() }
}

fn audio_streams_shape(value: Option<&Value>) -> StreamsShape {
    let Some(Value::Array(arr)) = value else { return StreamsShape { count: 0, items: Vec::new() }; };
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

fn count_field(item: &Value) -> String {
    match item.get("count") { Some(Value::Number(n)) => n.to_string(), Some(_) => "<invalid>".to_string(), None => "<missing>".to_string() }
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
fn format_mm_ss(total_secs: u64) -> String { let minutes = total_secs / 60; let seconds = total_secs % 60; format!("{minutes:02}:{seconds:02}") }
fn url_kind(item: &Value) -> &'static str {
    let Some(url_value) = item.get("url") else { return "no url"; };
    let Some(url) = url_value.as_str() else { return "no url"; };
    let lower = url.to_ascii_lowercase();
    if lower.is_empty() { "no url" } else if lower.contains(".m3u8") { "m3u8" } else if lower.contains(".mp3") { "mp3/other" } else { "mp3/other" }
}
fn value_type(value: &Value) -> &'static str {
    match value { Value::Null => "null", Value::Bool(_) => "bool", Value::Number(_) => "number", Value::String(_) => "string", Value::Array(_) => "array", Value::Object(_) => "object" }
}

fn parse_play_seconds(raw: &str) -> Result<u64> {
    let secs: u64 = raw.parse().map_err(|_| anyhow!("invalid --seconds value"))?;
    if secs == 0 { return Err(anyhow!("--seconds must be greater than 0")); }
    Ok(secs)
}

fn run_play(client: &reqwest::blocking::Client, token: &str, index_raw: &str, seconds: Option<u64>) -> Result<()> {
    let index: usize = index_raw.parse().map_err(|_| anyhow!("invalid --play value"))?;
    if index == 0 { return Err(anyhow!("--play index must start at 1")); }
    let response: AudioGetResponse = api::vk_call(client, token, "audio.get", &[("count", TRACK_LIMIT.to_string().as_str())])?;
    let items = response.items.unwrap_or_default();
    let item = items.get(index - 1).ok_or_else(|| anyhow!("--play index out of range"))?;
    let track = Track::from_value(item);
    let url = select_play_url_cli(client, item)?;
    println!("playing: {} - {}", track.artist, track.title);
    let bytes = download_track_bytes_cli(client, &url)?;
    match seconds {
        Some(s) => play_bytes_cli(bytes, s),
        None => play_interactive_cli(bytes),
    }
}

fn select_play_url_cli(client: &reqwest::blocking::Client, item: &Value) -> Result<String> {
    let main_url = item.get("url").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("no playable mp3"))?;
    match probe_first_bytes_cli(client, main_url) {
        Ok(bytes) if is_mp3_bytes_cli(&bytes) => Ok(main_url.to_string()),
        Ok(_) | Err(_) => {
            let streams = item.get("audio_streams").and_then(Value::as_array).ok_or_else(|| anyhow!("no playable mp3"))?;
            for stream in streams {
                let stype = stream.get("type").and_then(Value::as_str).unwrap_or("");
                if stype != "mp3" { continue; }
                let url = stream.get("url").and_then(Value::as_str).filter(|s| !s.is_empty());
                if let Some(url) = url { return Ok(url.to_string()); }
            }
            Err(anyhow!("no playable mp3"))
        }
    }
}

fn probe_first_bytes_cli(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
    let mut resp = client.get(url).header(reqwest::header::RANGE, "bytes=0-15").send().map_err(api::map_reqwest_error)?;
    let status = resp.status();
    if !status.is_success() { return Err(anyhow!("request failed (status {})", status.as_u16())); }
    let mut buf = [0u8; 16];
    let mut read_total = 0usize;
    loop {
        if read_total >= buf.len() { break; }
        let n = resp.read(&mut buf[read_total..]).map_err(|_| anyhow!("request failed (decode)"))?;
        if n == 0 { break; }
        read_total += n;
    }
    Ok(buf[..read_total].to_vec())
}

fn is_mp3_bytes_cli(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"ID3") { return true; }
    bytes.len() >= 2 && bytes[0] == 0xFF && matches!(bytes[1], 0xE2..=0xFF)
}

fn download_track_bytes_cli(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
    println!("downloading...");
    let start = Instant::now();
    let mut resp = client.get(url).send().map_err(api::map_reqwest_error)?;
    let status = resp.status();
    if !status.is_success() { return Err(anyhow!("request failed (status {})", status.as_u16())); }
    let content_length = resp.content_length();
    println!("Content-Length: {}", content_length.map(|v| v.to_string()).unwrap_or_else(|| "<missing>".to_string()));
    if let Some(len) = content_length { if len as usize > api::PLAY_MAX_BYTES { return Err(anyhow!("request failed (decode)")); } }
    let mut buf: Vec<u8> = Vec::with_capacity(content_length.map(|l| (l as usize).min(api::PLAY_MAX_BYTES)).unwrap_or(0));
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = resp.read(&mut chunk).map_err(|_| anyhow!("request failed (decode)"))?;
        if n == 0 { break; }
        if buf.len().saturating_add(n) > api::PLAY_MAX_BYTES { return Err(anyhow!("request failed (decode)")); }
        buf.extend_from_slice(&chunk[..n]);
    }
    let bytes_read = buf.len();
    println!("bytes read: {bytes_read}");
    println!("Vec len: {bytes_read}");
    if let Some(len) = content_length { if bytes_read != len as usize { return Err(anyhow!("request failed (decode)")); } }
    let elapsed = start.elapsed().as_secs_f64();
    let mb = buf.len() as f64 / (1024.0 * 1024.0);
    println!("{mb:.2} MB in {elapsed:.2}s");
    Ok(buf)
}

fn play_bytes_cli(bytes: Vec<u8>, seconds: u64) -> Result<()> {
    use rodio::{Decoder, DeviceSinkBuilder, Player};
    use std::io::Cursor;
    let mut stream = DeviceSinkBuilder::open_default_sink().map_err(|_| anyhow!("request failed (decode)"))?;
    stream.log_on_drop(false);
    let player = Player::connect_new(&stream.mixer());
    let decoder = Decoder::try_from(Cursor::new(bytes)).map_err(|_| anyhow!("request failed (decode)"))?;
    player.append(decoder);
    let start = Instant::now();
    let mut next_report = Duration::from_secs(5);
    let limit = Duration::from_secs(seconds);
    loop {
        let elapsed = start.elapsed();
        if elapsed >= limit || player.empty() { break; }
        if elapsed >= next_report {
            let pos = player.get_pos().as_secs();
            println!("{pos}s");
            next_report += Duration::from_secs(5);
        }
        let remaining = limit.saturating_sub(elapsed);
        let sleep_for = std::cmp::min(Duration::from_millis(200), remaining);
        if sleep_for.is_zero() { break; }
        std::thread::sleep(sleep_for);
    }
    player.stop();
    println!("done");
    Ok(())
}

fn play_interactive_cli(bytes: Vec<u8>) -> Result<()> {
    use rodio::{Decoder, DeviceSinkBuilder, Player, Source};
    use std::io::Cursor;
    use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
    let total_duration = Decoder::try_from(Cursor::new(bytes.clone())).ok().and_then(|d| d.total_duration());
    let mut stream = DeviceSinkBuilder::open_default_sink().map_err(|_| anyhow!("request failed (decode)"))?;
    stream.log_on_drop(false);
    let player = Arc::new(Player::connect_new(&stream.mixer()));
    let bytes = Arc::new(bytes);
    let decoder = Decoder::try_from(Cursor::new(bytes.as_ref().clone())).map_err(|_| anyhow!("request failed (decode)"))?;
    player.append(decoder);
    print_state_cli(true, player.get_pos(), total_duration, player.volume());

    let stop = Arc::new(AtomicBool::new(false));
    let reporter_player = Arc::clone(&player);
    let reporter_stop = Arc::clone(&stop);
    let reporter = std::thread::spawn(move || {
        while !reporter_stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(200));
            if reporter_stop.load(Ordering::Relaxed) { break; }
            let pos = reporter_player.get_pos();
            let pos_s = pos.as_secs();
            if pos_s > 0 && pos_s % 5 == 0 && pos.subsec_millis() < 250 {
                let dur = total_duration.map(|d| d.as_secs().to_string()).unwrap_or_else(|| "?".to_string());
                println!("{pos_s} / {dur}");
                std::thread::sleep(Duration::from_millis(260));
            }
            if reporter_player.empty() { break; }
        }
    });

    let stdin = std::io::stdin();
    let mut playing = true;
    for line in stdin.lock().lines() {
        let line = match line { Ok(l) => l, Err(_) => break };
        let line = line.trim();
        if line.is_empty() { continue; }
        let mut parts = line.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        match cmd {
            "p" => {
                if playing { player.pause(); playing = false; } else { player.play(); playing = true; }
                print_state_cli(playing, player.get_pos(), total_duration, player.volume());
            }
            "s" => {
                let Some(arg) = parts.next() else { println!("unknown"); continue; };
                let Ok(sec) = arg.parse::<u64>() else { println!("unknown"); continue; };
                let target = Duration::from_secs(sec);
                let decoder = match Decoder::try_from(Cursor::new(bytes.as_ref().clone())) { Ok(d) => d, Err(_) => { println!("request failed (decode)"); continue; } };
                let source = decoder.skip_duration(target);
                player.clear();
                player.append(source);
                if !playing { player.pause(); }
                print_state_cli(playing, player.get_pos(), total_duration, player.volume());
            }
            "v" => {
                let Some(arg) = parts.next() else { println!("unknown"); continue; };
                let Ok(vol) = arg.parse::<f32>() else { println!("unknown"); continue; };
                if !(0.0..=1.5).contains(&vol) { println!("unknown"); continue; }
                player.set_volume(vol);
                print_state_cli(playing, player.get_pos(), total_duration, player.volume());
            }
            "h" => {
                println!("p pause/resume | s <sec> seek | v <x> volume 0.0..1.5 | h help | q quit");
                print_state_cli(playing, player.get_pos(), total_duration, player.volume());
            }
            "q" => break,
            _ => println!("unknown"),
        }
    }
    stop.store(true, Ordering::Relaxed);
    player.stop();
    let _ = reporter.join();
    println!("done");
    Ok(())
}

fn print_state_cli(playing: bool, pos: Duration, total: Option<Duration>, volume: f32) {
    let state = if playing { "playing" } else { "paused" };
    let pos_s = pos.as_secs();
    let dur_s = total.map(|d| d.as_secs().to_string()).unwrap_or_else(|| "?".to_string());
    println!("state: {state}, pos: {pos_s}s, duration: {dur_s}s, volume: {volume:.2}");
}
