#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use anyhow::{anyhow, Result};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

mod api;
mod gui;
mod player;
mod storage;
mod theme;

const CRASH_LOG_MAX_BYTES: u64 = 200 * 1024;
const FPS_LOG_MAX_BYTES: u64 = 100 * 1024;

fn install_crash_logging() {
    prepare_crash_log();
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current().name().unwrap_or("<unnamed>").to_string();
        let location = match info.location() {
            Some(loc) => format!("{}:{}", loc.file(), loc.line()),
            None => "unknown:0".to_string(),
        };
        append_crash_log_line(&format!("panic thread={thread} location={location}"));
    }));
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn log_path(name: &str) -> Option<PathBuf> {
    let appdata = env::var_os("APPDATA")?;
    let dir = PathBuf::from(appdata).join("vkmusic");
    fs::create_dir_all(&dir).ok()?;
    Some(dir.join(name))
}

fn crash_log_path() -> Option<PathBuf> {
    log_path("crash.log")
}

fn fps_log_path() -> Option<PathBuf> {
    log_path("fps.log")
}

fn prepare_crash_log() {
    if let Some(path) = crash_log_path() {
        trim_log(&path, CRASH_LOG_MAX_BYTES);
    }
}

fn trim_log(path: &Path, max_bytes: u64) {
    let mut file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(_) => return,
    };
    let len = match file.metadata() {
        Ok(meta) => meta.len(),
        Err(_) => return,
    };
    if len <= max_bytes { return; }
    let keep_from = len.saturating_sub(max_bytes / 2);
    if file.seek(SeekFrom::Start(keep_from)).is_err() { return; }
    let mut tail = Vec::new();
    if file.read_to_end(&mut tail).is_err() { return; }
    if let Some(pos) = tail.iter().position(|&b| b == b'\n') {
        tail.drain(..=pos);
    }
    if file.set_len(0).is_err() { return; }
    if file.seek(SeekFrom::Start(0)).is_err() { return; }
    let _ = file.write_all(&tail);
}

pub(crate) fn log_status_error(msg: &str) {
    let safe = api::sanitize_output(msg);
    append_crash_log_line(&format!("status {safe}"));
}

fn append_crash_log_line(line: &str) {
    let path = match crash_log_path() {
        Some(path) => path,
        None => return,
    };
    let one_line: String = api::redact_urls(line)
        .chars()
        .take(240)
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    if one_line.trim().is_empty() { return; }
    let ts = now_secs();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{ts} {one_line}");
    }
    trim_log(&path, CRASH_LOG_MAX_BYTES);
}

pub(crate) fn log_fps_line(line: &str) {
    let path = match fps_log_path() {
        Some(path) => path,
        None => return,
    };
    let one_line: String = line
        .chars()
        .take(240)
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    if one_line.trim().is_empty() { return; }
    let ts = now_secs();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{ts} {one_line}");
    }
    trim_log(&path, FPS_LOG_MAX_BYTES);
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            log_status_error(&format!("fatal: {err:#}"));
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<()> {
    install_crash_logging();
    #[cfg(windows)]
    let guard = match single_instance::acquire("Local\\trueVK-single-instance") {
        single_instance::State::Acquired(guard) => Some(guard),
        single_instance::State::AlreadyRunning => {
            notify_existing_instance();
            return Ok(());
        }
        single_instance::State::Unavailable => None,
    };
    #[cfg(not(windows))]
    let guard: Option<()> = None;
    let single_instance = single_instance_listener();
    let env_token = env::var("VK_TOKEN").ok().filter(|v| !v.trim().is_empty());
    let env_ua = env::var("VK_UA").ok().filter(|v| !v.trim().is_empty());
    gui::run_gui(env_token, env_ua, single_instance).map_err(|e| anyhow!("{}", e))?;
    drop(guard);
    Ok(())
}

const SINGLE_INSTANCE_PORT: u16 = 47613;

fn single_instance_listener() -> Option<TcpListener> {
    TcpListener::bind(("127.0.0.1", SINGLE_INSTANCE_PORT)).ok()
}

fn notify_existing_instance() {
    if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", SINGLE_INSTANCE_PORT)) {
        let _ = stream.write_all(b"show\n");
        let _ = stream.flush();
    }
}

#[cfg(windows)]
mod single_instance {
    use std::ffi::c_void;
    use std::ptr;

    type Handle = *mut c_void;
    const ERROR_ALREADY_EXISTS: u32 = 183;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(attrs: *mut c_void, initial_owner: i32, name: *const u16) -> Handle;
        fn GetLastError() -> u32;
        fn SetLastError(error: u32);
        fn CloseHandle(handle: Handle) -> i32;
    }

    pub struct Guard(Handle);

    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    pub enum State {
        Acquired(Guard),
        AlreadyRunning,
        Unavailable,
    }

    pub fn acquire(name: &str) -> State {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            SetLastError(0);
        }
        let handle = unsafe { CreateMutexW(ptr::null_mut(), 0, wide.as_ptr()) };
        let error = unsafe { GetLastError() };
        if handle.is_null() {
            return State::Unavailable;
        }
        if error == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(handle);
            }
            State::AlreadyRunning
        } else {
            State::Acquired(Guard(handle))
        }
    }
}

#[cfg(not(windows))]
mod single_instance {
    pub enum State {
        Acquired(()),
        AlreadyRunning,
        Unavailable,
    }

    pub fn acquire(_name: &str) -> State {
        State::Unavailable
    }
}
