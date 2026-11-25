# AGENTS.md - vkmusic

## Project
Small native Windows player for VK Music (Rust). Own VK account, music only. Meant to replace the browser while gaming (CS2): must be light on CPU and RAM. No Electron/Tauri/webview.
Path: C:\dev\vkmusic. Shell: PowerShell on Windows 11. The user writes in Russian and is not a programmer: keep reports short, plain, no jargon walls.

## Hard rules
1. SECRETS: never print, log, store in files, or ask for the VK token. Never print track URLs. Token and UA live only in the user's env (VK_TOKEN, VK_UA) or in Windows Credential Manager (keyring). Never use setx. Never save a token that failed.
2. AUDIO ONLY IN RAM: no automatic downloads to disk, no cache, no temp files. Track bytes are Arc<Vec<u8>> (40 MB cap). The only disk write of audio is the explicit "Сохранить MP3" command for the currently playing track.
3. NEW CRATES: do not add any crate without telling the user which one and why. Read its source in ~\.cargo\registry\src first. Never guess crate APIs or feature names. Use default-features = false where possible.
4. NEVER run the GUI (the user tests it). NEVER kill processes. NEVER touch git config.
5. REPAINT RULES: no polling, no timers, no continuous repainting. Repaint only on events, plus at most once per second while the position text changes. 16 ms frame limiter stays. A hidden/minimized window must sleep until an event arrives.
6. DONE MEANS PROOF: never report a task as done without pasting the raw output of `git status --short` and `git log --oneline -3` after the commit. Never claim code exists without showing the Select-String/grep proof when asked. If you only added a dependency, say so. Do the work, do not only describe it.
7. If something does not fit (crate, API, approach): STOP and report. Do not silently fall back (e.g. no plaintext token file).
8. Unsure = say "not verified". Never invent VK API parameters; give the source of each.
9. Commit after each stage with the message given in the task, then STOP.
10. CYRILLIC: write every Russian UI string in Rust source with \u{...} escapes, or verify with `Select-String -Path src\*.rs -Pattern '"\?{3,}'` after writing. Literal "???" in source is a bug.

## Files
- src/main.rs: entry point only. No CLI flags and no probe modes: any launch starts the GUI. Named WinAPI mutex guards a single instance; a second launch notifies the first over local TCP port 47613. Logs live in %APPDATA%\vkmusic and are trimmed at startup and every 200 writes.
- src/api.rs: VK API client, Track model (owner_id, id, access_key, identity()), typed VkApiError, auth detection by error code 5/15, token extraction, output sanitizing.
- src/player.rs: asynchronous download + playback worker (rodio). Network download runs in its own cancellable thread, so Stop/Pause/Seek/next never wait for it. One dedicated preload worker coalesces preload requests. mpsc commands/events, generation counter against stale events, seekable decoder via SharedBytes, quadratic volume curve, 40 ms fades, output recovery by recreating the sink, SaveCurrent writes playing bytes to a chosen path.
- src/gui.rs: eframe/egui app: tabs, lists, bottom panel, queue logic, tray, hotkeys, context menu ("Похожие треки", "Сохранить MP3").
- src/storage.rs: Credential Manager (keyring) for token+UA; %APPDATA%\vkmusic\settings.json for volume, last track, repeat/shuffle and window geometry; cache.json for track/playlist metadata. Atomic settings write, corrupt file backed up as .json.bak. No secrets, no URLs in settings.json.

## Architecture rules
- App::logic runs on EVERY pass, including when the window is minimized/hidden (eframe logic-only branch). ALL non-drawing event handling lives there (handle_events: player events, Ended -> next track, media keys, tray events, network results, restore of last track). ui() only draws and reads input. Never move event handling back into ui().
- Hotkey manager (global-hotkey) and tray icon (tray-icon) are created on the MAIN thread in the eframe app creator closure and kept alive in the App struct. Their handlers only forward a UiEvent and call ctx.request_repaint().
- Queue is the list the playing track was started from (My Music, playlist, search, recommendations, filtered list). It is tied to track identity (owner_id + audio_id), never to list indexes. Prev/Next/autoplay skip tracks without url. Switching tabs or searching again does not change the queue. The playing-row highlight shows only in the list the track belongs to.
- Stale network responses are dropped with a request id. At most one request in flight per list.
- Network work runs off the UI thread; results trigger request_repaint.
- Close button (X) hides the window to the tray; "Выход" in the tray menu quits for real (stop player, drop tray icon, Close).
- Volume: slider value is clamped 0.0..1.0 and saved as shown; the player receives slider.powf(2.0), so the low end is finer. Read-only percent label, slider only.
- The completion monitor sleeps on a channel while paused/stopped and wakes only on commands or a 750 ms timeout while playing. Do not add polling loops.
- Audio output failures and default-device changes are recovered by opening a new sink and seeking to the last position; the track must not silently die.
- "Похожие треки" sets rec_target and loads recommendations with target_audio={owner_id}_{id}; the normal Recommendations refresh clears rec_target.
- Hotkeys Space and Ctrl+Left/Right must not fire while a text field has focus.

## VK facts (verified)
- Host https://api.vk.ru/method/, v 5.207, User-Agent from VK_UA (default Android UA: VKAndroidApp/8.154-99999 (Android 12; SDK 32; arm64-v8a; Pixel 6; ru; 2960x1440)).
- Methods in use: users.get, audio.get (also with owner_id + album_id + access_key for playlists), audio.getPlaylists, audio.search, audio.getById, audio.getRecommendations, audio.add, audio.delete.
- audio.getRecommendations: use count=50. offset does NOT work (returns 0 items). Each call returns a partly new set (~30-50% overlap). Response has only `count` and `items`. Dedupe by (owner_id, audio_id). target_audio={owner_id}_{id} was requested for "Похожие треки"; it is not independently verified against VK docs.
- Track `url` is a plain mp3 (~5-10 MB, Range supported). It expires, so fetch a fresh url via audio.getById before playing a saved track. Ignore HLS/DASH/AAC/FLAC streams. Some tracks have no url (restricted): grey them out.
- audio.add / audio.delete parameter names came from the VkNet library (not VK docs). access_key for audio.add was added by us, not confirmed.

## Dependencies (current)
anyhow, eframe 0.37.0 (glow, default_fonts, clipboard), keyring 4.2.0 (v1, windows-native-keyring-store), reqwest 0.13.5 (blocking, json, rustls), rodio 0.23.0 (playback, mp3), serde, serde_json, global-hotkey 0.10.0, tray-icon 0.26.1 (pulls muda). No windows-sys.

## Stages
Done: 1 CLI, 1.5 probes, 2a play from RAM, 3a GUI skeleton, 3b queue/volume/seek, UI layout, 3d saved login + volume, 3c playlists and search, add/remove music ("+" and "×" with confirmation), 3e recommendations tab, 4a media keys + last track, 4b tray, 4c single instance/shuffle/repeat/filter/tray tooltip, 5 async cancellable player + code-based auth errors + reliable single instance + tests, 6 sound polish (volume curve, fades, device recovery), similar tracks, save MP3, log/monitor/font optimizations.
Next: final polish: texts and colors in one place, look of "+" and "×", English "Log out" label to Russian, window/tray/exe icon, hide the console window, CS2 resource measurement.

## Diagnostics
VKMUSIC_FPS=1 prints "passes: N, frames: M, logic: K" to stderr every 10 s. Idle target: about 10 passes per 10 s. Resource targets: about 0% CPU while playing/idle, RAM soft target under 150 MB.

## Known and accepted
- Cyrillic shows as ????? in some console output (encoding only).
- CLI playback/probe modes were removed in stage 5; there is no console interface.