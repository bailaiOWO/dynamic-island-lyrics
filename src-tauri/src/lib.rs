use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs::File;
use std::io::{BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::Mutex;
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

// ==================== Online Search ====================

fn fetch_lyrics_online(title: &str, artist: &str, duration_s: u64) -> Vec<LyricLine> {
    eprintln!("[online] searching lyrics: '{}' by '{}' dur={}s", title, artist, duration_s);
    // Try LRCLIB exact match first
    let url = format!(
        "https://lrclib.net/api/get?track_name={}&artist_name={}&duration={}",
        urlenc(title), urlenc(artist), duration_s
    );
    if let Ok(resp) = ureq::get(&url)
        .set("User-Agent", "DynamicIslandLyrics/0.1 (https://github.com/bailaiOWO/dynamic-island-lyrics)")
        .call()
    {
        if resp.status() == 200 {
            if let Ok(body) = resp.into_string() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                    if let Some(synced) = json["syncedLyrics"].as_str() {
                        let lyrics = parse_lrc(synced);
                        if !lyrics.is_empty() {
                            eprintln!("[online] LRCLIB exact: {} lines", lyrics.len());
                            return lyrics;
                        }
                    }
                    if let Some(plain) = json["plainLyrics"].as_str() {
                        eprintln!("[online] LRCLIB plain lyrics found (no sync)");
                        // Return plain lyrics with 0ms timestamps
                        return plain.lines()
                            .filter(|l| !l.trim().is_empty())
                            .enumerate()
                            .map(|(i, l)| LyricLine { time_ms: i as u64 * 5000, text: l.trim().to_string() })
                            .collect();
                    }
                }
            }
        }
    }
    // Fallback: LRCLIB search
    let url2 = format!(
        "https://lrclib.net/api/search?track_name={}&artist_name={}",
        urlenc(title), urlenc(artist)
    );
    if let Ok(resp) = ureq::get(&url2)
        .set("User-Agent", "DynamicIslandLyrics/0.1 (https://github.com/bailaiOWO/dynamic-island-lyrics)")
        .call()
    {
        if resp.status() == 200 {
            if let Ok(body) = resp.into_string() {
                if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&body) {
                    for item in &arr {
                        if let Some(synced) = item["syncedLyrics"].as_str() {
                            let lyrics = parse_lrc(synced);
                            if !lyrics.is_empty() {
                                eprintln!("[online] LRCLIB search: {} lines", lyrics.len());
                                return lyrics;
                            }
                        }
                    }
                }
            }
        }
    }
    eprintln!("[online] no lyrics found");
    Vec::new()
}

fn fetch_cover_online(title: &str, artist: &str) -> String {
    eprintln!("[online] searching cover: '{}' by '{}'", title, artist);
    let url = format!(
        "https://itunes.apple.com/search?term={}&media=music&entity=song&limit=3",
        urlenc(&format!("{} {}", artist, title))
    );
    if let Ok(resp) = ureq::get(&url).call() {
        if resp.status() == 200 {
            if let Ok(body) = resp.into_string() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                    if let Some(results) = json["results"].as_array() {
                        for r in results {
                            if let Some(art) = r["artworkUrl100"].as_str() {
                                // Replace 100x100 with 600x600 for high res
                                let hires = art.replace("100x100", "600x600");
                                eprintln!("[online] iTunes cover found");
                                // Download and convert to base64
                                if let Ok(img_resp) = ureq::get(&hires).call() {
                                    if img_resp.status() == 200 {
                                        let mut bytes = Vec::new();
                                        if img_resp.into_reader().read_to_end(&mut bytes).is_ok() && !bytes.is_empty() {
                                            let mut b64 = String::from("data:image/jpeg;base64,");
                                            b64.push_str(&base64_encode(&bytes));
                                            return b64;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    eprintln!("[online] no cover found");
    String::new()
}

fn urlenc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => { out.push('%'); out.push_str(&format!("{:02X}", b)); }
        }
    }
    out
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
// ==================== FFmpeg Pipe Source (fallback) ====================

struct FfmpegSource {
    reader: BufReader<std::process::ChildStdout>,
    child: std::process::Child,
    sample_rate: u32,
    channels: u16,
}

impl FfmpegSource {
    fn new(path: &str, sample_rate: u32, channels: u16) -> Result<Self, String> {
        let mut child = Command::new(ffmpeg_bin())
            .args(["-i", path,
                   "-f", "s16le", "-acodec", "pcm_s16le",
                   "-ar", &sample_rate.to_string(),
                   "-ac", &channels.to_string(),
                   "-"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| format!("ffmpeg spawn: {e}"))?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        Ok(Self {
            reader: BufReader::with_capacity(65536, stdout),
            child, sample_rate, channels,
        })
    }
}

impl Drop for FfmpegSource {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

impl Iterator for FfmpegSource {
    type Item = i16;
    fn next(&mut self) -> Option<i16> {
        let mut buf = [0u8; 2];
        self.reader.read_exact(&mut buf).ok()?;
        Some(i16::from_le_bytes(buf))
    }
}

impl Source for FfmpegSource {
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
}

unsafe impl Send for PlayerInner {}

pub struct AudioPlayer {
    inner: Option<PlayerInner>,
    current_path: String,
    use_ffmpeg: bool,
    lyrics: Vec<LyricLine>,
    start_time: Option<std::time::Instant>,
    paused_elapsed: Duration,
    is_paused: bool,
    song_title: String,
    song_artist: String,
    duration_ms: u64,
}

impl AudioPlayer {
    fn new() -> Self {
        Self {
            inner: None, current_path: String::new(), use_ffmpeg: false,
            lyrics: Vec::new(),
            start_time: None, paused_elapsed: Duration::ZERO,
            is_paused: false,
            song_title: String::new(), song_artist: String::new(),
            duration_ms: 0,
        }
    }

    fn elapsed_ms(&self) -> u64 {
        match (self.is_paused, self.start_time) {
            (true, _) => self.paused_elapsed.as_millis() as u64,
            (false, Some(t)) => (self.paused_elapsed + t.elapsed()).as_millis() as u64,
            _ => 0,
        }
    }
}

pub struct AppState {
    player: Mutex<AudioPlayer>,
    island_width: Mutex<f64>,
    theme_color: Mutex<String>,
    island_font: Mutex<String>,
    island_height: Mutex<f64>,
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
    let mut p = state.player.lock().map_err(|e| e.to_string())?;

    if let Some(old) = p.inner.take() { old.sink.stop(); drop(old); }

    let (stream, handle) = OutputStream::try_default().map_err(|e| e.to_string())?;
    let sink = Sink::try_new(&handle).map_err(|e| e.to_string())?;

    // Try rodio/symphonia first
    let mut dur_ms = 0u64;
    let mut use_ffmpeg = false;
    match File::open(&path) {
        Ok(file) => match Decoder::new(BufReader::new(file)) {
            Ok(source) => {
                dur_ms = source.total_duration().map(|d| d.as_millis() as u64).unwrap_or(0);
                sink.append(source);
                std::thread::sleep(Duration::from_millis(50));
                if sink.empty() {
                    eprintln!("[open_music] rodio produced no audio, falling back to ffmpeg");
                    use_ffmpeg = true;
                } else {
                    eprintln!("[open_music] rodio streaming ok");
                }
            }
            Err(e) => {
                eprintln!("[open_music] rodio decode error: {}, falling back to ffmpeg", e);
                use_ffmpeg = true;
            }
        },
        Err(e) => return Err(format!("Cannot open: {e}")),
    }

    if use_ffmpeg {
        sink.stop();
        let sink2 = Sink::try_new(&handle).map_err(|e| e.to_string())?;
        let meta = ffprobe_meta(&path);
        dur_ms = meta.duration_ms;
        let ff_source = FfmpegSource::new(&path, meta.sample_rate, meta.channels)?;
        sink2.append(ff_source);
        eprintln!("[open_music] ffmpeg streaming ok, dur={}ms", dur_ms);

        let fp = PathBuf::from(&path);
        let fname = fp.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let (artist, title) = match fname.split_once(" - ") {
            Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
            None => ("Unknown".to_string(), fname),
        };
        let lyrics = load_lyrics(&fp);
        let cover_path = extract_cover_embedded(&path);
        let dur = if dur_ms > 0 { dur_ms }
            else if !lyrics.is_empty() { lyrics.last().map(|l| l.time_ms + 10000).unwrap_or(0) }
            else { 0 };

        p.song_title = title.clone();
        p.song_artist = artist.clone();
        p.lyrics = lyrics.clone();
        p.duration_ms = dur;
        p.current_path = path.clone();
        p.use_ffmpeg = true;
        p.inner = Some(PlayerInner { sink: sink2, _stream: stream, _handle: handle });
        p.start_time = Some(std::time::Instant::now());
        p.paused_elapsed = Duration::ZERO;
        p.is_paused = false;
        return Ok(SongInfo { title, artist, has_lyrics: !lyrics.is_empty(), lyrics, duration_ms: dur, cover_path });
    }

    let fp = PathBuf::from(&path);
    let fname = fp.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let (artist, title) = match fname.split_once(" - ") {
        Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
        None => ("Unknown".to_string(), fname),
    };
    let lyrics = load_lyrics(&fp);
    let cover_path = extract_cover_embedded(&path);
    let dur = if dur_ms > 0 { dur_ms }
        else if !lyrics.is_empty() { lyrics.last().map(|l| l.time_ms + 10000).unwrap_or(0) }
        else { 0 };

    p.song_title = title.clone();
    p.song_artist = artist.clone();
    p.lyrics = lyrics.clone();
    p.duration_ms = dur;
    p.current_path = path.clone();
    p.use_ffmpeg = false;
    p.inner = Some(PlayerInner { sink, _stream: stream, _handle: handle });
    p.start_time = Some(std::time::Instant::now());
    p.paused_elapsed = Duration::ZERO;
    p.is_paused = false;

    eprintln!("[open_music] playing! title={} lyrics={} dur={}ms", title, lyrics.len(), dur);
    Ok(SongInfo { title, artist, has_lyrics: !lyrics.is_empty(), lyrics, duration_ms: dur, cover_path })
}


fn extract_cover_embedded(path: &str) -> String {
    let tmp = std::env::temp_dir().join("dil_cover.jpg");
    let _ = std::fs::remove_file(&tmp);
    // Try ffmpeg but with a short timeout
    let result = Command::new(ffmpeg_bin())
        .args(["-i", path, "-an", "-vcodec", "copy", "-y", &tmp.to_string_lossy()])
        .stdout(Stdio::null()).stderr(Stdio::null()).stdin(Stdio::null())
        .output();
    if result.is_ok() && tmp.exists() {
        if let Ok(bytes) = std::fs::read(&tmp) {
            if !bytes.is_empty() {
                let mut b64 = String::from("data:image/jpeg;base64,");
                b64.push_str(&base64_encode(&bytes));
                return b64;
            }
        }
    }
    String::new()
}



#[tauri::command]
fn play_pause(state: State<'_, AppState>) -> Result<bool, String> {
    let mut p = state.player.lock().map_err(|e| e.to_string())?;
    let was_paused = p.inner.as_ref().ok_or("No music loaded")?.sink.is_paused();
    if was_paused {
        if let Some(ref inner) = p.inner { inner.sink.play(); }
        p.start_time = Some(std::time::Instant::now());
        p.is_paused = false;
    } else {
        if let Some(t) = p.start_time { p.paused_elapsed += t.elapsed(); }
        p.start_time = None;
        if let Some(ref inner) = p.inner { inner.sink.pause(); }
        p.is_paused = true;
    }
    Ok(!was_paused)
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
    let mut p = state.player.lock().map_err(|e| e.to_string())?;
    let dur = Duration::from_millis(position_ms);

    if p.use_ffmpeg {
        // FFmpeg mode: restart process with -ss
        if let Some(old) = p.inner.take() { old.sink.stop(); drop(old); }
        let path = p.current_path.clone();
        let meta = ffprobe_meta(&path);
        let (stream, handle) = OutputStream::try_default().map_err(|e| e.to_string())?;
        let sink = Sink::try_new(&handle).map_err(|e| e.to_string())?;
        let ss = format!("{:.3}", position_ms as f64 / 1000.0);
        let mut child = Command::new(ffmpeg_bin())
            .args(["-ss", &ss, "-i", &path,
                   "-f", "s16le", "-acodec", "pcm_s16le",
                   "-ar", &meta.sample_rate.to_string(),
                   "-ac", &meta.channels.to_string(),
                   "-"])
            .stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null())
            .spawn().map_err(|e| format!("ffmpeg: {e}"))?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let source = FfmpegSource {
            reader: BufReader::with_capacity(65536, stdout),
            child, sample_rate: meta.sample_rate, channels: meta.channels,
        };
        sink.append(source);
        p.inner = Some(PlayerInner { sink, _stream: stream, _handle: handle });
    } else {
        // Rodio mode: use try_seek
        if let Some(ref inner) = p.inner {
            let _ = inner.sink.try_seek(dur);
        }
    }
    p.start_time = Some(std::time::Instant::now());
    p.paused_elapsed = dur;
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
fn set_island_font(font: String, state: State<'_, AppState>) {
    if let Ok(mut f) = state.island_font.lock() { *f = font; }
}

#[tauri::command]
fn get_island_font(state: State<'_, AppState>) -> String {
    state.island_font.lock().map(|f| f.clone()).unwrap_or_default()
}

#[tauri::command]
fn set_island_height(height: f64, state: State<'_, AppState>, app: tauri::AppHandle) {
    if let Ok(mut h) = state.island_height.lock() { *h = height; }
    if let Some(w) = app.get_webview_window("island") {
        let sw = get_screen_width(&w).unwrap_or(1920.0);
        let _ = w.set_size(tauri::LogicalSize::new(sw, height));
    }
}

#[tauri::command]
fn get_island_height(state: State<'_, AppState>) -> f64 {
    state.island_height.lock().map(|h| *h).unwrap_or(44.0)
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

#[derive(Serialize)]
pub struct TrackItem {
    path: String,
    title: String,
    artist: String,
    has_local_lyrics: bool,
}

#[tauri::command]
fn scan_folder(folder: String) -> Vec<TrackItem> {
    let exts = ["mp3", "wav", "flac", "ogg", "m4a", "aac", "wma", "opus"];
    let mut tracks = Vec::new();
    let Ok(entries) = std::fs::read_dir(&folder) else { return tracks };
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() { continue; }
        let ext = p.extension().unwrap_or_default().to_string_lossy().to_lowercase();
        if !exts.contains(&ext.as_str()) { continue; }
        let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let fname = p.file_name().unwrap_or_default().to_string_lossy().to_string();
        let (artist, title) = match stem.split_once(" - ") {
            Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
            None => ("Unknown".to_string(), stem),
        };
        // Check if local lyrics exist
        let dir = p.parent().unwrap_or(std::path::Path::new("."));
        let has_lyrics = ["lrc", "vtt", "srt", "ass", "ssa"].iter().any(|e| {
            dir.join(format!("{}.{}", p.file_stem().unwrap_or_default().to_string_lossy(), e)).exists()
            || dir.join(format!("{}.{}", fname, e)).exists()
        });
        tracks.push(TrackItem {
            path: p.to_string_lossy().to_string(),
            title, artist, has_local_lyrics: has_lyrics,
        });
    }
    tracks.sort_by(|a, b| a.title.cmp(&b.title));
    tracks
}

#[tauri::command]
fn match_lyrics_for_track(path: String) -> Result<String, String> {
    let fp = PathBuf::from(&path);
    let stem = fp.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let (artist, title) = match stem.split_once(" - ") {
        Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
        None => return Err("Cannot parse artist - title from filename".into()),
    };
    let meta = ffprobe_meta(&path);
    let dur_s = meta.duration_ms / 1000;
    let lyrics = fetch_lyrics_online(&title, &artist, dur_s);
    if lyrics.is_empty() {
        return Err("No lyrics found online".into());
    }
    // Save as .lrc next to audio file
    let lrc_path = fp.with_extension("lrc");
    let mut content = String::new();
    for l in &lyrics {
        let min = l.time_ms / 60000;
        let sec = (l.time_ms % 60000) / 1000;
        let ms = (l.time_ms % 1000) / 10;
        content.push_str(&format!("[{:02}:{:02}.{:02}]{}", min, sec, ms, l.text));
        content.push('\n');
    }
    std::fs::write(&lrc_path, &content).map_err(|e| e.to_string())?;
    eprintln!("[match] saved {} lines to {:?}", lyrics.len(), lrc_path);
    Ok(format!("Matched {} lines", lyrics.len()))
}


#[tauri::command]
// ==================== Playlist Management ====================

fn playlists_dir() -> PathBuf {
    let mut p = ffmpeg_path();
    p.pop(); // remove "ffmpeg"
    p.push("playlists");
    let _ = std::fs::create_dir_all(&p);
    p
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Playlist {
    name: String,
    cover_path: String,
    tracks: Vec<String>, // file paths
}

#[derive(Serialize)]
pub struct PlaylistInfo {
    id: String, // filename without .json
    name: String,
    cover_path: String,
    track_count: usize,
}

fn load_playlist(id: &str) -> Option<Playlist> {
    let path = playlists_dir().join(format!("{}.json", id));
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

fn save_playlist(id: &str, pl: &Playlist) -> Result<(), String> {
    let path = playlists_dir().join(format!("{}.json", id));
    let data = serde_json::to_string_pretty(pl).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_playlists() -> Vec<PlaylistInfo> {
    let dir = playlists_dir();
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().map(|e| e == "json").unwrap_or(false) {
                let id = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
                if let Some(pl) = load_playlist(&id) {
                    out.push(PlaylistInfo {
                        id, name: pl.name, cover_path: pl.cover_path,
                        track_count: pl.tracks.len(),
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[tauri::command]
fn create_playlist(name: String) -> Result<String, String> {
    let id = format!("{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis());
    let pl = Playlist { name, cover_path: String::new(), tracks: Vec::new() };
    save_playlist(&id, &pl)?;
    Ok(id)
}

#[tauri::command]
fn get_playlist_tracks(id: String) -> Result<Vec<TrackItem>, String> {
    let pl = load_playlist(&id).ok_or("Playlist not found")?;
    let mut tracks = Vec::new();
    for path in &pl.tracks {
        let fp = PathBuf::from(path);
        if !fp.exists() { continue; }
        let stem = fp.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let fname = fp.file_name().unwrap_or_default().to_string_lossy().to_string();
        let (artist, title) = match stem.split_once(" - ") {
            Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
            None => ("Unknown".to_string(), stem),
        };
        let dir = fp.parent().unwrap_or(std::path::Path::new("."));
        let has_lyrics = ["lrc", "vtt", "srt", "ass", "ssa"].iter().any(|e| {
            dir.join(format!("{}.{}", fp.file_stem().unwrap_or_default().to_string_lossy(), e)).exists()
            || dir.join(format!("{}.{}", fname, e)).exists()
        });
        tracks.push(TrackItem { path: path.clone(), title, artist, has_local_lyrics: has_lyrics });
    }
    Ok(tracks)
}

#[tauri::command]
fn add_tracks_to_playlist(id: String, paths: Vec<String>) -> Result<(), String> {
    let mut pl = load_playlist(&id).ok_or("Playlist not found")?;
    for p in paths {
        if !pl.tracks.contains(&p) {
            pl.tracks.push(p);
        }
    }
    save_playlist(&id, &pl)
}

#[tauri::command]
fn rename_playlist(id: String, name: String) -> Result<(), String> {
    let mut pl = load_playlist(&id).ok_or("Playlist not found")?;
    pl.name = name;
    save_playlist(&id, &pl)
}

#[tauri::command]
fn set_playlist_cover(id: String, cover_path: String) -> Result<(), String> {
    let mut pl = load_playlist(&id).ok_or("Playlist not found")?;
    pl.cover_path = cover_path;
    save_playlist(&id, &pl)
}

#[tauri::command]
fn delete_playlist(id: String) -> Result<(), String> {
    let path = playlists_dir().join(format!("{}.json", id));
    std::fs::remove_file(&path).map_err(|e| e.to_string())
}


#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn hide_to_tray(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("player") {
        let _ = w.hide();
    }
}

#[tauri::command]
fn show_player(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("player") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

// ==================== Entry ====================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState { player: Mutex::new(AudioPlayer::new()), island_width: Mutex::new(380.0), theme_color: Mutex::new("#ffffff".to_string()), island_font: Mutex::new(String::new()), island_height: Mutex::new(44.0) })
        .invoke_handler(tauri::generate_handler![
            open_music, play_pause, get_position, get_current_lyric,
            get_is_playing, set_volume, seek_to, set_click_through,
            get_mouse_in_zone, set_island_width,
            set_theme_color, get_theme_color,

            center_island, resize_island,
            set_island_font, get_island_font,
            set_island_height, get_island_height,
            scan_folder, match_lyrics_for_track,
            list_playlists, create_playlist, get_playlist_tracks,
            add_tracks_to_playlist, rename_playlist,
            set_playlist_cover, delete_playlist,
            hide_to_tray, show_player, quit_app,
        ])

        .setup(|app| {
            // Island window
            let w = app.get_webview_window("island").expect("island window");
            let sw = get_screen_width(&w).unwrap_or(1920.0);
            let _ = w.set_size(tauri::LogicalSize::new(sw, 44.0));
            let _ = w.set_position(tauri::LogicalPosition::new(0.0, 0.0));
            let _ = w.set_shadow(false);
            let _ = w.set_skip_taskbar(true);
            let _ = w.set_ignore_cursor_events(true);

            // Player window
            if let Some(pw) = app.get_webview_window("player") {
                let _ = pw.set_shadow(false);
            }

            // System tray
            use tauri::tray::{TrayIconBuilder, MouseButton, MouseButtonState};
            use tauri::menu::{MenuBuilder, MenuItemBuilder};

            let show = MenuItemBuilder::with_id("show", "Show Player").build(app)?;
            let prev = MenuItemBuilder::with_id("prev", "Previous").build(app)?;
            let playpause = MenuItemBuilder::with_id("playpause", "Play/Pause").build(app)?;
            let next = MenuItemBuilder::with_id("next", "Next").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

            let menu = MenuBuilder::new(app)
                .item(&show)
                .separator()
                .item(&prev)
                .item(&playpause)
                .item(&next)
                .separator()
                .item(&quit)
                .build()?;

            let _tray = TrayIconBuilder::new()
                .tooltip("Dynamic Island Lyrics")
                .menu(&menu)
                .on_menu_event(move |app, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("player") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "prev" => {
                            // Handled by JS via polling
                        }
                        "playpause" => {
                            if let Ok(mut p) = app.state::<AppState>().player.lock() {
                                if let Some(ref inner) = p.inner {
                                    if inner.sink.is_paused() {
                                        inner.sink.play();
                                        p.is_paused = false;
                                    } else {
                                        inner.sink.pause();
                                        p.is_paused = true;
                                    }
                                }
                            }
                        }
                        "next" => {
                            // Handled by JS via polling
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event {
                        if let Some(w) = tray.app_handle().get_webview_window("player") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
