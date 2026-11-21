\# vkmusic — project brief (read before every task)



Goal: a small native Windows app in Rust that replaces the browser for VK music

while gaming. Music only: no feed, no messages. One exe, low resources.



Features, in priority order:

1\. Login: the user pastes a VK access token + matching User-Agent once; the app

&#x20;  stores it locally and never logs or prints it. No password login.

2\. My music (audio.get), my playlists (audio.getPlaylists + tracks of a playlist),

&#x20;  search (audio.search). Every request sends the same User-Agent.

3\. Playback: play/pause/next/prev/seek/volume. Track URLs may be direct mp3 or

&#x20;  HLS (.m3u8, AES-128 segments) and expire; re-request the URL when it fails.

4\. Media keys, tray icon, remember last track and volume.



Hard limits: \~0% CPU while playing, RAM < 60 MB. No Electron/Tauri/webview.

Minimal dependencies. GUI only in Stage 3 (eframe/egui glow, repaint only on events).



Work in stages and STOP after each: 1) CLI that lists my audios; 2) play one track

from console; 3) GUI; 4) media keys/tray/saved state/resource check.

Do not skip ahead or plan later stages in detail.



Secrets: never read, print or echo VK\_TOKEN / VK\_UA, never run the app with them

set (the user runs it). Strip URLs and tokens from any logs or output.



Never invent API methods, fields or crate APIs. If unsure, say "not verified" and ask.

