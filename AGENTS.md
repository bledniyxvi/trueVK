# vkmusic - project brief (read before every task)

Goal: a small native Windows app in Rust that replaces the browser for VK music
while gaming. Music only: no feed, no messages. One exe, low resources.

Features, in priority order:
1. Login: the user pastes a VK access token + matching User-Agent once; the app
   stores it locally and never logs or prints it. No password login.
2. My music (audio.get), my playlists (audio.getPlaylists; tracks of a playlist
   via audio.get with owner_id + album_id + access_key), search (audio.search).
   Every request sends the same User-Agent. API host api.vk.ru, version 5.207.
3. Playback: play/pause/next/prev/seek/volume.
4. Media keys, tray icon, remember last track and volume.

Hard limits: ~0% CPU while playing, RAM < 60 MB. No Electron/Tauri/webview.
Minimal dependencies. GUI only in Stage 3 (eframe/egui glow, repaint only on events).

Stages (STOP after each): 1) CLI list [done]; 1.5) probes [done]; 2a) play one mp3
from memory for N seconds; 2b) pause/seek/volume; 3) GUI; 4) media keys/tray/saved state.
Do not skip ahead or plan later stages in detail.

Secrets: never read, print or echo VK_TOKEN / VK_UA. Run the app only through the
redaction pipeline (redact token, replace URLs with <url>). Strip URLs and tokens
from all logs and errors.

Never invent API methods, fields or crate APIs. Read the crate source in
~/.cargo/registry/src if unsure. If still unsure, say "not verified" and ask.
Do not add crates that were not requested.

Verified facts (probe): audio.get `url` is a plain mp3 (ID3, audio/mpeg, ~320 kbps,
5-8 MB), supports Range, expires in ~5 days. No HLS/AES/TS needed. Ignore
audio_streams except as a fallback (type "mp3"). Never use flac/aac/dash/hls.

Memory and disk: audio is streamed into memory only. Never write audio (or partial
data) to disk: no cache, no temp files. Keep only compressed mp3 bytes in RAM, at
most the current and next track. Never decode a whole track into a PCM buffer;
decode as a stream.