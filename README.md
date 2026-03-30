# Dynamic Island Lyrics / 灵动岛歌词

一个受 iPhone 灵动岛启发的桌面歌词显示器，悬浮在屏幕顶部，实时同步显示歌词。

A desktop lyrics display inspired by iPhone's Dynamic Island. Sits at the top of your screen, showing real-time synced lyrics with a minimal dark UI.

## Features

- **Dynamic Island style** — black rounded bar at top center, auto-resizes to fit lyrics
- **Real-time lyrics** — syncs with playback
- **Multi-format lyrics** — LRC, VTT, SRT, ASS/SSA
- **Universal audio** — powered by FFmpeg, supports MP3, FLAC, WAV, OGG, M4A, AAC, WMA, and more
- **Zero quality loss** — decodes at original sample rate and channels
- **Instant seek & scrubbing** — full audio decoded to memory, seek by moving pointer
- **Mouse hover to hide** — island disappears when cursor enters top zone, reappears when it leaves
- **Separate player window** — open files, play/pause, seek, volume

## Tech Stack

- [Tauri 2](https://tauri.app/) — desktop app framework (MIT)
- [Rust](https://www.rust-lang.org/) — backend
- [rodio](https://github.com/RustAudio/rodio) — audio output (Apache-2.0 / MIT)
- [FFmpeg](https://ffmpeg.org/) — audio decoding (GPL v2+, see below)
- HTML/CSS/JS — frontend UI

## Setup

### 1. FFmpeg

Download the latest **win64 GPL static build** from [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds/releases) and place `ffmpeg.exe` and `ffprobe.exe` in the `ffmpeg/` directory:

```
lyric-island/
  ffmpeg/
    ffmpeg.exe
    ffprobe.exe
    LICENSE.txt
```

### 2. Build

```bash
cd src-tauri
cargo build
```

## License

This project is licensed under the **GNU General Public License v3.0 (GPL-3.0)**.

This software uses [FFmpeg](https://ffmpeg.org/) licensed under the GPL. FFmpeg source code and build instructions can be found at [github.com/BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds).

See [LICENSE](LICENSE) for details.
