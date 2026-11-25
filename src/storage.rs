use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const SERVICE: &str = "vkmusic";
const TOKEN_USER: &str = "vk_token";
const UA_USER: &str = "vk_ua";

static REDACTION_TOKEN: OnceLock<Mutex<Option<Option<String>>>> = OnceLock::new();

fn redaction_cache() -> &'static Mutex<Option<Option<String>>> {
    REDACTION_TOKEN.get_or_init(|| Mutex::new(None))
}

fn set_redaction_token(token: Option<String>) {
    let mut guard = redaction_cache().lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(token);
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SavedTrack {
    pub owner_id: i64,
    pub audio_id: i64,
    #[serde(default)]
    pub access_key: Option<String>,
    pub artist: String,
    pub title: String,
    #[serde(default)]
    pub duration: Option<u64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Settings {
    #[serde(default)]
    volume: Option<f32>,
    #[serde(default)]
    last_track: Option<SavedTrack>,
    #[serde(default)]
    repeat: Option<String>,
    #[serde(default)]
    shuffle: bool,
    #[serde(default)]
    window_x: Option<f32>,
    #[serde(default)]
    window_y: Option<f32>,
    #[serde(default)]
    window_w: Option<f32>,
    #[serde(default)]
    window_h: Option<f32>,
}

pub fn load_saved_credentials() -> (Option<String>, Option<String>) {
    let token = read_secret(TOKEN_USER);
    let ua = read_secret(UA_USER);
    let result = match (token, ua) {
        (Some(t), Some(u)) if !t.trim().is_empty() && !u.trim().is_empty() => (Some(t), Some(u)),
        _ => (None, None),
    };
    set_redaction_token(result.0.clone());
    result
}

pub fn saved_token_for_redaction() -> Option<String> {
    {
        let guard = redaction_cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = guard.as_ref() {
            return cached.clone();
        }
    }
    let value = read_secret(TOKEN_USER);
    set_redaction_token(value.clone());
    value
}

pub fn save_credentials(token: &str, ua: &str) -> Result<()> {
    if token.trim().is_empty() || ua.trim().is_empty() {
        return Ok(());
    }
    write_secret(TOKEN_USER, token)?;
    write_secret(UA_USER, ua)?;
    set_redaction_token(Some(token.to_string()));
    Ok(())
}

pub fn delete_credentials() {
    let _ = delete_secret(TOKEN_USER);
    let _ = delete_secret(UA_USER);
    set_redaction_token(None);
}

pub fn load_volume() -> f32 {
    load_settings().volume.unwrap_or(1.0).clamp(0.0, 1.0)
}

pub fn save_volume(volume: f32) {
    let volume = volume.clamp(0.0, 1.0);
    let mut settings = load_settings();
    settings.volume = Some(volume);
    let _ = save_settings(&settings);
}

pub fn load_last_track() -> Option<SavedTrack> {
    load_settings().last_track
}

pub fn save_last_track(track: &SavedTrack) {
    let mut settings = load_settings();
    settings.last_track = Some(track.clone());
    let _ = save_settings(&settings);
}

pub fn load_repeat() -> String {
    load_settings().repeat.unwrap_or_else(|| "off".to_string())
}

pub fn save_repeat(repeat: &str) {
    let mut settings = load_settings();
    settings.repeat = Some(repeat.to_string());
    let _ = save_settings(&settings);
}

pub fn load_shuffle() -> bool {
    load_settings().shuffle
}

pub fn save_shuffle(shuffle: bool) {
    let mut settings = load_settings();
    settings.shuffle = shuffle;
    let _ = save_settings(&settings);
}

pub fn load_window_geometry() -> Option<(f32, f32, f32, f32)> {
    let s = load_settings();
    Some((s.window_x?, s.window_y?, s.window_w?, s.window_h?))
}

pub fn save_window_geometry(x: f32, y: f32, w: f32, h: f32) {
    let mut settings = load_settings();
    settings.window_x = Some(x);
    settings.window_y = Some(y);
    settings.window_w = Some(w);
    settings.window_h = Some(h);
    let _ = save_settings(&settings);
}

fn cache_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("vkmusic").join("cache.json"))
}

pub fn load_cache() -> Option<serde_json::Value> {
    let path = cache_path()?;
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice::<serde_json::Value>(&bytes).ok()
}

pub fn save_cache(value: &serde_json::Value) {
    let Some(path) = cache_path() else { return; };
    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    if let Ok(data) = serde_json::to_vec(value) {
        let _ = fs::write(path, data);
    }
}

fn read_secret(user: &str) -> Option<String> {
    let entry = keyring::Entry::new(SERVICE, user).ok()?;
    match entry.get_password() {
        Ok(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    }
}

fn write_secret(user: &str, value: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, user).context("keyring entry")?;
    entry.set_password(value).context("keyring set")?;
    Ok(())
}

fn delete_secret(user: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, user).context("keyring entry")?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).context("keyring delete"),
    }
}

fn settings_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("vkmusic").join("settings.json"))
}

fn load_settings() -> Settings {
    let Some(path) = settings_path() else { return Settings::default(); };
    load_settings_from(&path)
}

fn load_settings_from(path: &Path) -> Settings {
    let Ok(bytes) = fs::read(path) else { return Settings::default(); };
    match serde_json::from_slice::<Settings>(&bytes) {
        Ok(settings) => settings,
        Err(_) => {
            let backup = path.with_extension("json.bak");
            let _ = fs::rename(path, &backup);
            Settings::default()
        }
    }
}

fn save_settings(settings: &Settings) -> Result<()> {
    let Some(path) = settings_path() else { return Ok(()); };
    save_settings_to(&path, settings)
}

fn save_settings_to(path: &Path, settings: &Settings) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("create settings dir")?;
    }
    let data = serde_json::to_vec(settings).context("serialize settings")?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, data).context("write settings temp")?;
    if let Err(e) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(e).context("replace settings");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("vkmusic-test-{}-{}", std::process::id(), n))
    }

    fn sample_settings() -> Settings {
        Settings {
            volume: Some(0.42),
            last_track: Some(SavedTrack {
                owner_id: 1,
                audio_id: 2,
                access_key: Some("key".to_string()),
                artist: "Artist".to_string(),
                title: "Title".to_string(),
                duration: Some(123),
            }),
            repeat: Some("one".to_string()),
            shuffle: true,
            window_x: Some(10.0),
            window_y: Some(20.0),
            window_w: Some(800.0),
            window_h: Some(600.0),
        }
    }

    #[test]
    fn settings_roundtrip() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        save_settings_to(&path, &sample_settings()).unwrap();
        let loaded = load_settings_from(&path);
        assert_eq!(loaded.volume, Some(0.42));
        assert_eq!(loaded.repeat.as_deref(), Some("one"));
        assert!(loaded.shuffle);
        assert_eq!(loaded.window_w, Some(800.0));
        let track = loaded.last_track.unwrap();
        assert_eq!(track.owner_id, 1);
        assert_eq!(track.audio_id, 2);
        assert_eq!(track.access_key.as_deref(), Some("key"));
        assert_eq!(track.title, "Title");
        assert_eq!(track.duration, Some(123));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn corrupt_settings_are_backed_up() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, b"{not json").unwrap();
        let loaded = load_settings_from(&path);
        assert!(loaded.volume.is_none());
        assert!(loaded.last_track.is_none());
        assert!(!path.exists());
        assert!(path.with_extension("json.bak").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
