use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const SERVICE: &str = "vkmusic";
const TOKEN_USER: &str = "vk_token";
const UA_USER: &str = "vk_ua";

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
}

pub fn load_saved_credentials() -> (Option<String>, Option<String>) {
    let token = read_secret(TOKEN_USER);
    let ua = read_secret(UA_USER);
    match (token, ua) {
        (Some(t), Some(u)) if !t.trim().is_empty() && !u.trim().is_empty() => (Some(t), Some(u)),
        _ => (None, None),
    }
}

pub fn saved_token_for_redaction() -> Option<String> {
    read_secret(TOKEN_USER)
}

pub fn save_credentials(token: &str, ua: &str) {
    if token.trim().is_empty() || ua.trim().is_empty() {
        return;
    }
    let _ = write_secret(TOKEN_USER, token);
    let _ = write_secret(UA_USER, ua);
}

pub fn delete_credentials() {
    let _ = delete_secret(TOKEN_USER);
    let _ = delete_secret(UA_USER);
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
    let Ok(bytes) = fs::read(path) else { return Settings::default(); };
    serde_json::from_slice::<Settings>(&bytes).unwrap_or_default()
}

fn save_settings(settings: &Settings) -> Result<()> {
    let Some(path) = settings_path() else { return Ok(()); };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("create settings dir")?;
    }
    let data = serde_json::to_vec(settings).context("serialize settings")?;
    fs::write(path, data).context("write settings")?;
    Ok(())
}
