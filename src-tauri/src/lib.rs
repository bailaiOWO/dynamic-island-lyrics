use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{Manager, State};

// ==================== Lyrics Parser ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

fn parse_lrc(content: &str) -> Vec<LyricLine> {
    let re = regex::Regex::new(r"\[(\d{2}):(\d{2})\.(\d{2,3})\](.*)").unwrap();
    let mut lines = Vec::new();
    for line in content.lines() {
        let Some(caps) = re.captures(line) else { continue };
        let min: u64 = caps[1].parse().unwrap_or(0);
        let sec: u64 = caps[2].parse().unwrap_or(0);
        let ms_str = &caps[3];
        let ms: u64 = if ms_str.len() == 2 {
            ms_str.parse::<u64>().unwrap_or(0) * 10
        } else {
            ms_str.parse().unwrap_or(0)
        };
        let text = caps[4].trim().to_string();
        if !text.is_empty() {
            lines.push(LyricLine { time_ms: min * 60000 + sec * 1000 + ms, text });
        }
    }
    lines.sort_by_key(|l| l.time_ms);
    lines
}

fn parse_timestamp(s: &str) -> Option<u64> {
    let s = s.replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        2 => {
            let min: f64 = parts[0].trim().parse().ok()?;
            let sec: f64 = parts[1].trim().parse().ok()?;
            Some((min * 60000.0 + sec * 1000.0) as u64)
        }
        3 => {
            let hr: f64 = parts[0].trim().parse().ok()?;
            let min: f64 = parts[1].trim().parse().ok()?;
            let sec: f64 = parts[2].trim().parse().ok()?;
            Some((hr * 3600000.0 + min * 60000.0 + sec * 1000.0) as u64)
        }
        _ => None,
    }
}

fn parse_vtt_srt(content: &str) -> Vec<LyricLine> {
    let re = regex::Regex::new(r"(\d[\d:.,]+)\s*-->\s*(\d[\d:.,]+)").unwrap();
    let tag_re = regex::Regex::new(r"<[^>]+>").unwrap();
    let mut lines = Vec::new();
    let content_lines: Vec<&str> = content.lines().collect();
    for (i, line) in content_lines.iter().enumerate() {
        if let Some(caps) = re.captures(line) {
            if let Some(time_ms) = parse_timestamp(&caps[1]) {
                let mut text_parts = Vec::new();
                for j in (i + 1)..content_lines.len() {
                    let tl = content_lines[j].trim();
                    if tl.is_empty() { break; }
                    let clean = tag_re.replace_all(tl, "").to_string();
                    if !clean.is_empty() { text_parts.push(clean); }
                }
                let text = text_parts.join(" ");
                if !text.is_empty() {
                    lines.push(LyricLine { time_ms, text });
                }
            }
        }
    }
    lines.sort_by_key(|l| l.time_ms);
    lines
}

fn parse_ass(content: &str) -> Vec<LyricLine> {
    let tag_re = regex::Regex::new(r"\{[^}]*\}").unwrap();
    let mut lines = Vec::new();
    for line in content.lines() {
        if !line.starts_with("Dialogue:") { continue; }
        let parts: Vec<&str> = line.splitn(10, ',').collect();
        if parts.len() < 10 { continue; }
        if let Some(time_ms) = parse_timestamp(parts[1].trim()) {
            let clean = tag_re.replace_all(parts[9], "")
                .replace("\\N", " ").replace("\\n", " ");
            let text = clean.trim().to_string();
            if !text.is_empty() {
                lines.push(LyricLine { time_ms, text });
            }
        }
    }
    lines.sort_by_key(|l| l.time_ms);
    lines
}

fn load_lyrics(audio_path: &PathBuf) -> Vec<LyricLine> {
    let stem = audio_path.file_stem().unwrap_or_default().to_string_lossy();
    let dir = audio_path.parent().unwrap_or(std::path::Path::new("."));
    let fname_str = audio_path.file_name().unwrap_or_default().to_string_lossy();

    let candidates: Vec<PathBuf> = vec![
        dir.join(format!("{}.lrc", stem)),
        dir.join(format!("{}.vtt", stem)),
        dir.join(format!("{}.srt", stem)),
        dir.join(format!("{}.ass", stem)),
        dir.join(format!("{}.ssa", stem)),
        dir.join(format!("{}.vtt", fname_str)),
        dir.join(format!("{}.srt", fname_str)),
        dir.join(format!("{}.lrc", fname_str)),
    ];

    for path in candidates {
        if !path.exists() { continue; }
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        let ext = path.extension().unwrap_or_default().to_string_lossy().to_lowercase();
        let lyrics = match ext.as_str() {
            "lrc" => parse_lrc(&content),
            "vtt" | "srt" => parse_vtt_srt(&content),
            "ass" | "ssa" => parse_ass(&content),
            _ => continue,
        };
        if !lyrics.is_empty() {
            eprintln!("[lyrics] loaded {} lines from {:?}", lyrics.len(), path);
            return lyrics;
        }
    }
    Vec::new()
}

// ==================== FFmpeg Decode to Memory ====================
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len() * 4 / 3 + 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((n >> 18) & 63) as usize] as char);
        result.push(CHARS[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 { result.push(CHARS[((n >> 6) & 63) as usize] as char); } else { result.push('='); }
        if chunk.len() > 2 { result.push(CHARS[(n & 63) as usize] as char); } else { result.push('='); }
    }
    result
}



fn ffmpeg_path() -> PathBuf {
    // Search: exe_dir/ffmpeg, then walk up parents to find ffmpeg/ dir
    let exe = std::env::current_exe().unwrap_or_default();
    let mut dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    loop {
        let candidate = dir.join("ffmpeg");
        if candidate.join("ffmpeg.exe").exists() {
            return candidate;
        }
        if !dir.pop() { break; }
    }
    // Fallback: just try exe_dir/ffmpeg
    exe.parent().unwrap_or(std::path::Path::new(".")).join("ffmpeg")
}

fn ffmpeg_bin() -> PathBuf { ffmpeg_path().join("ffmpeg.exe") }
fn ffprobe_bin() -> PathBuf { ffmpeg_path().join("ffprobe.exe") }

fn log_ffmpeg_path() {
    eprintln!("[ffmpeg] exe={:?}", std::env::current_exe().unwrap_or_default());
    eprintln!("[ffmpeg] resolved dir={:?}", ffmpeg_path());
    eprintln!("[ffmpeg] ffmpeg={:?} exists={}", ffmpeg_bin(), ffmpeg_bin().exists());
    eprintln!("[ffmpeg] ffprobe={:?} exists={}", ffprobe_bin(), ffprobe_bin().exists());
}

struct AudioMeta {
    duration_ms: u64,
    sample_rate: u32,
    channels: u16,
}

fn ffprobe_meta(path: &str) -> AudioMeta {
    let out = Command::new(ffprobe_bin())
        .args(["-v", "error", "-select_streams", "a:0",
               "-show_entries", "stream=sample_rate,channels:format=duration",
               "-of", "default=noprint_wrappers=1:nokey=0", path])
        .stdout(Stdio::piped()).stderr(Stdio::null())
        .output().ok();
    let s = out.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let mut sr = 44100u32;
    let mut ch = 2u16;
    let mut dur = 0u64;
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("sample_rate=") {
            sr = v.trim().parse().unwrap_or(44100);
        } else if let Some(v) = line.strip_prefix("channels=") {
            ch = v.trim().parse().unwrap_or(2);
        } else if let Some(v) = line.strip_prefix("duration=") {
            if let Ok(f) = v.trim().parse::<f64>() { dur = (f * 1000.0) as u64; }
        }
    }
    AudioMeta { duration_ms: dur, sample_rate: sr, channels: ch }
}

/// Decode entire audio file to memory via ffmpeg, preserving original sample rate.
/// Returns (samples_i16, sample_rate, channels)
fn ffmpeg_decode_full(path: &str, sr: u32, ch: u16) -> Result<Vec<i16>, String> {
    let out = Command::new(ffmpeg_bin())
        .args(["-i", path,
               "-f", "s16le", "-acodec", "pcm_s16le",
               "-ar", &sr.to_string(),
               "-ac", &ch.to_string(),
               "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    if out.stdout.is_empty() {
        return Err("ffmpeg produced no output".into());
    }
    let samples: Vec<i16> = out.stdout
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    Ok(samples)
}

// ==================== Memory Buffer Source ====================

struct BufferSource {
    data: Arc<Vec<i16>>,
    pos: Arc<Mutex<usize>>,
    sample_rate: u32,
    channels: u16,
}

impl BufferSource {
    fn new(data: Arc<Vec<i16>>, pos: Arc<Mutex<usize>>, sample_rate: u32, channels: u16) -> Self {
        Self { data, pos, sample_rate, channels }
    }
}

impl Iterator for BufferSource {
    type Item = i16;
    fn next(&mut self) -> Option<i16> {
        let mut p = self.pos.lock().ok()?;
        if *p >= self.data.len() { return None; }
        let sample = self.data[*p];
        *p += 1;
        Some(sample)
    }
}

impl Source for BufferSource {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { self.channels }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn total_duration(&self) -> Option<Duration> { None }
}

// ==================== Audio Player ====================

struct PlayerInner {
    sink: Sink,
    _stream: OutputStream,
    _handle: OutputStreamHandle,
    pcm_data: Arc<Vec<i16>>,
    pcm_pos: Arc<Mutex<usize>>,
    sample_rate: u32,
    channels: u16,
}

unsafe impl Send for PlayerInner {}

pub struct AudioPlayer {
    inner: Option<PlayerInner>,
    lyrics: Vec<LyricLine>,
    is_paused: bool,
    song_title: String,
    song_artist: String,
    duration_ms: u64,
}

impl AudioPlayer {
    fn new() -> Self {
        Self {
            inner: None, lyrics: Vec::new(),
            is_paused: false,
            song_title: String::new(), song_artist: String::new(),
            duration_ms: 0,
        }
    }

    fn elapsed_ms(&self) -> u64 {
        if let Some(ref inner) = self.inner {
            let pos = inner.pcm_pos.lock().map(|p| *p).unwrap_or(0);
            let samples_per_ms = (inner.sample_rate as u64 * inner.channels as u64) / 1000;
            if samples_per_ms > 0 { pos as u64 / samples_per_ms } else { 0 }
        } else {
            0
        }
    }
}

pub struct AppState {
    player: Mutex<AudioPlayer>,
    island_width: Mutex<f64>,
    theme_color: Mutex<String>,
}

// ==================== Helper ====================

fn get_window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    app.get_webview_window("island").ok_or_else(|| "window not found".into())
}

fn get_screen_width(w: &tauri::WebviewWindow) -> Result<f64, String> {
    let monitor = w.current_monitor().map_err(|e| e.to_string())?
        .ok_or("no monitor")?;
    Ok(monitor.size().width as f64 / monitor.scale_factor())
}

// ==================== Commands ====================

#[derive(Serialize)]
pub struct SongInfo {
    title: String,
    artist: String,
    has_lyrics: bool,
    lyrics: Vec<LyricLine>,
    duration_ms: u64,
    cover_path: String,
}

#[derive(Serialize)]
pub struct CurrentLyric {
    current: String,
    next: String,
    progress: f64,
    position_ms: u64,
}

#[tauri::command]
fn open_music(path: String, state: State<'_, AppState>) -> Result<SongInfo, String> {
    eprintln!("[open_music] path={}", path);
    log_ffmpeg_path();
    let mut p = state.player.lock().map_err(|e| e.to_string())?;

    // Stop old
    if let Some(old) = p.inner.take() { old.sink.stop(); drop(old); }

    // Probe metadata
    let meta = ffprobe_meta(&path);
    eprintln!("[open_music] sr={} ch={} dur={}ms", meta.sample_rate, meta.channels, meta.duration_ms);

    // Decode entire file to memory (original sample rate, no quality loss)
    eprintln!("[open_music] decoding to memory...");
    let samples = ffmpeg_decode_full(&path, meta.sample_rate, meta.channels)?;
    eprintln!("[open_music] decoded {} samples ({:.1}MB)", samples.len(), samples.len() as f64 * 2.0 / 1048576.0);

    let pcm_data = Arc::new(samples);
    let pcm_pos = Arc::new(Mutex::new(0usize));

    let source = BufferSource::new(pcm_data.clone(), pcm_pos.clone(), meta.sample_rate, meta.channels);

    let (stream, handle) = OutputStream::try_default().map_err(|e| e.to_string())?;
    let sink = Sink::try_new(&handle).map_err(|e| e.to_string())?;
    sink.append(source);

    let fp = PathBuf::from(&path);
    let fname = fp.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let (artist, title) = match fname.split_once(" - ") {
        Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
        None => ("Unknown".to_string(), fname),
    };

    let lyrics = load_lyrics(&fp);

    // Compute real duration from PCM if ffprobe failed
    let dur = if meta.duration_ms > 0 {
        meta.duration_ms
    } else {
        let total_samples = pcm_data.len() as u64;
        let sps = meta.sample_rate as u64 * meta.channels as u64;
        if sps > 0 { total_samples * 1000 / sps } else { 0 }
    };

    p.song_title = title.clone();
    p.song_artist = artist.clone();
    p.lyrics = lyrics.clone();
    p.duration_ms = dur;
    p.inner = Some(PlayerInner {
        sink, _stream: stream, _handle: handle,
        pcm_data, pcm_pos, sample_rate: meta.sample_rate, channels: meta.channels,
    });
    p.is_paused = false;

    // Extract cover art as base64
    let cover_path = {
        let tmp = std::env::temp_dir().join("dil_cover.jpg");
        let _ = std::fs::remove_file(&tmp);
        let _ = Command::new(ffmpeg_bin())
            .args(["-i", &path, "-an", "-vcodec", "copy", "-y",
                   &tmp.to_string_lossy()])
            .stdout(Stdio::null()).stderr(Stdio::null()).stdin(Stdio::null())
            .output();
        if tmp.exists() {
            if let Ok(bytes) = std::fs::read(&tmp) {
                let mut b64 = String::from("data:image/jpeg;base64,");
                b64.push_str(&base64_encode(&bytes));
                b64
            } else { String::new() }
        } else { String::new() }
    };


    eprintln!("[open_music] playing! title={} lyrics={} dur={}ms", title, lyrics.len(), dur);
    Ok(SongInfo { title, artist, has_lyrics: !lyrics.is_empty(), lyrics, duration_ms: dur, cover_path })
}

#[tauri::command]
fn play_pause(state: State<'_, AppState>) -> Result<bool, String> {
    let mut p = state.player.lock().map_err(|e| e.to_string())?;
    let inner = p.inner.as_ref().ok_or("No music loaded")?;
    let was_paused = inner.sink.is_paused();

    if was_paused {
        inner.sink.play();
        p.is_paused = false;
    } else {
        inner.sink.pause();
        p.is_paused = true;
    }
    Ok(!was_paused) // true = now playing
}

#[tauri::command]
fn get_position(state: State<'_, AppState>) -> Result<u64, String> {
    Ok(state.player.lock().map_err(|e| e.to_string())?.elapsed_ms())
}

#[tauri::command]
fn get_current_lyric(state: State<'_, AppState>) -> Result<CurrentLyric, String> {
    let p = state.player.lock().map_err(|e| e.to_string())?;
    let elapsed = p.elapsed_ms();

    if p.lyrics.is_empty() {
        return Ok(CurrentLyric {
            current: p.song_title.clone(),
            next: p.song_artist.clone(),
            progress: 0.0,
            position_ms: elapsed,
        });
    }

    let idx = p.lyrics.iter().rposition(|l| l.time_ms <= elapsed).unwrap_or(0);
    let current = p.lyrics[idx].text.clone();
    let next = p.lyrics.get(idx + 1).map(|l| l.text.clone()).unwrap_or_default();
    let ct = p.lyrics[idx].time_ms;
    let nt = p.lyrics.get(idx + 1).map(|l| l.time_ms).unwrap_or(ct + 5000);
    let progress = if nt > ct && elapsed >= ct {
        ((elapsed - ct) as f64 / (nt - ct) as f64).min(1.0)
    } else {
        0.0
    };

    Ok(CurrentLyric { current, next, progress, position_ms: elapsed })
}

#[tauri::command]
fn get_is_playing(state: State<'_, AppState>) -> Result<bool, String> {
    let p = state.player.lock().map_err(|e| e.to_string())?;
    Ok(p.inner.as_ref().map(|i| !i.sink.is_paused() && !i.sink.empty()).unwrap_or(false))
}

#[tauri::command]
fn set_volume(volume: f32, state: State<'_, AppState>) -> Result<(), String> {
    let p = state.player.lock().map_err(|e| e.to_string())?;
    if let Some(ref inner) = p.inner { inner.sink.set_volume(volume); }
    Ok(())
}

#[tauri::command]
fn seek_to(position_ms: u64, state: State<'_, AppState>) -> Result<(), String> {
    let p = state.player.lock().map_err(|e| e.to_string())?;
    if let Some(ref inner) = p.inner {
        let samples_per_ms = (inner.sample_rate as u64 * inner.channels as u64) / 1000;
        let target = (position_ms * samples_per_ms) as usize;
        let clamped = target.min(inner.pcm_data.len());
        if let Ok(mut pos) = inner.pcm_pos.lock() {
            *pos = clamped;
        }
    }
    Ok(())
}

#[tauri::command]
fn set_click_through(app: tauri::AppHandle, through: bool) -> Result<(), String> {
    get_window(&app)?.set_ignore_cursor_events(through).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_mouse_in_zone(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    let w = get_window(&app)?;
    let monitor = w.current_monitor().map_err(|e| e.to_string())?.ok_or("no monitor")?;
    let scale = monitor.scale_factor();
    let screen_w = monitor.size().width as f64;
    let iw = state.island_width.lock().map(|g| *g).unwrap_or(380.0) * scale;
    let zone_left = (screen_w - iw) / 2.0;
    let zone_right = (screen_w + iw) / 2.0;
    let zone_bottom = 60.0 * scale;

    #[cfg(target_os = "windows")]
    {
        #[repr(C)] struct POINT { x: i32, y: i32 }
        extern "system" { fn GetCursorPos(lp: *mut POINT) -> i32; }
        let mut pt = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut pt) } != 0 {
            let (mx, my) = (pt.x as f64, pt.y as f64);
            return Ok(mx >= zone_left && mx <= zone_right && my >= 0.0 && my <= zone_bottom);
        }
    }
    Ok(false)
}

#[tauri::command]
fn set_island_width(width: f64, state: State<'_, AppState>) {
    if let Ok(mut w) = state.island_width.lock() {
        *w = width;
    }
}

#[tauri::command]
fn set_theme_color(color: String, state: State<'_, AppState>) {
    if let Ok(mut c) = state.theme_color.lock() {
        *c = color;
    }
}

#[tauri::command]
fn get_theme_color(state: State<'_, AppState>) -> String {
    state.theme_color.lock().map(|c| c.clone()).unwrap_or_else(|_| "#ffffff".to_string())
}



#[tauri::command]
fn center_island(app: tauri::AppHandle) -> Result<(), String> {
    let w = get_window(&app)?;
    let sw = get_screen_width(&w)?;
    w.set_size(tauri::LogicalSize::new(sw, 44.0)).map_err(|e| e.to_string())?;
    w.set_position(tauri::LogicalPosition::new(0.0, 0.0)).map_err(|e| e.to_string())
}

#[tauri::command]
fn resize_island(app: tauri::AppHandle, width: f64) -> Result<(), String> {
    let w = get_window(&app)?;
    w.set_size(tauri::LogicalSize::new(width, 44.0)).map_err(|e| e.to_string())?;
    let sw = get_screen_width(&w)?;
    w.set_position(tauri::LogicalPosition::new((sw - width) / 2.0, 0.0)).map_err(|e| e.to_string())
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

// ==================== Entry ====================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState { player: Mutex::new(AudioPlayer::new()), island_width: Mutex::new(380.0), theme_color: Mutex::new("#ffffff".to_string()) })
        .invoke_handler(tauri::generate_handler![
            open_music, play_pause, get_position, get_current_lyric,
            get_is_playing, set_volume, seek_to, set_click_through,
            get_mouse_in_zone, set_island_width, set_theme_color, get_theme_color, center_island, resize_island, quit_app,
        ])
        .setup(|app| {
            let w = app.get_webview_window("island").expect("island window");
            let sw = get_screen_width(&w).unwrap_or(1920.0);
            let _ = w.set_size(tauri::LogicalSize::new(sw, 44.0));
            let _ = w.set_position(tauri::LogicalPosition::new(0.0, 0.0));
            let _ = w.set_shadow(false);
            let _ = w.set_ignore_cursor_events(true);
            if let Some(pw) = app.get_webview_window("player") {
                let _ = pw.set_shadow(false);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
