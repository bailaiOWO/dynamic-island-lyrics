const { invoke } = window.__TAURI__.core;
const { open } = window.__TAURI__.dialog;
const { getCurrentWindow } = window.__TAURI__.window;

const titleEl = document.getElementById('title');
const artistEl = document.getElementById('artist');
const progressBar = document.getElementById('progressBar');
const progressWrap = document.getElementById('progressWrap');
const timeCur = document.getElementById('timeCur');
const timeTotal = document.getElementById('timeTotal');
const playBtn = document.getElementById('playBtn');
const playIcon = document.getElementById('playIcon');
const openBtn = document.getElementById('openBtn');
const volumeEl = document.getElementById('volume');
const volLabel = document.getElementById('volLabel');
const closeBtn = document.getElementById('closeBtn');
const minBtn = document.getElementById('minBtn');

const ICON_PLAY = '<path d="M8 5v14l11-7z"/>';
const ICON_PAUSE = '<path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/>';

let playing = false;
let totalMs = 0;
let seeking = false;

function fmt(ms) {
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  return m + ':' + String(s % 60).padStart(2, '0');
}

function setPlayIcon(isPlaying) {
  playIcon.innerHTML = isPlaying ? ICON_PAUSE : ICON_PLAY;
}

// 关闭 = 退出整个 app
closeBtn.addEventListener('click', async () => {
  try { await invoke('quit_app'); } catch(e) {}
});

// 最小化
minBtn.addEventListener('click', async () => {
  try { await getCurrentWindow().minimize(); } catch(e) {}
});

// 打开文件
openBtn.addEventListener('click', async () => {
  try {
    const result = await open({
      filters: [{ name: 'Audio', extensions: ['mp3', 'wav', 'flac', 'ogg', 'm4a', 'aac', 'wma'] }]
    });
    if (!result) return;
    const path = typeof result === 'string' ? result : (result.path || result.toString());
    if (!path) return;
    const info = await invoke('open_music', { path });
    titleEl.textContent = info.title;
    artistEl.textContent = info.artist;
    playing = true;
    setPlayIcon(true);
    // 用真实时长，没有则用歌词估算
    if (info.duration_ms > 0) {
      totalMs = info.duration_ms;
    } else if (info.lyrics && info.lyrics.length > 0) {
      totalMs = info.lyrics[info.lyrics.length - 1].time_ms + 10000;
    } else {
      totalMs = 0;
    }
    timeTotal.textContent = totalMs > 0 ? fmt(totalMs) : '--:--';
  } catch (e) {
    console.error('open_music error:', e);
    titleEl.textContent = '加载失败';
    artistEl.textContent = String(e);
  }
});

// 播放/暂停
playBtn.addEventListener('click', async () => {
  try {
    const nowPlaying = await invoke('play_pause');
    playing = nowPlaying;
    setPlayIcon(playing);
  } catch (e) { console.error(e); }
});

// 音量
volumeEl.addEventListener('input', async () => {
  const v = volumeEl.value / 100;
  volLabel.textContent = volumeEl.value;
  try { await invoke('set_volume', { volume: v }); } catch (e) {}
});
invoke('set_volume', { volume: 0.8 }).catch(() => {});

// 进度条拖拽
let dragActive = false;

progressWrap.addEventListener('mousedown', (e) => {
  dragActive = true;
  seeking = true;
  doSeek(e);
});
document.addEventListener('mousemove', (e) => { if (dragActive) doSeek(e); });
document.addEventListener('mouseup', () => {
  if (dragActive) {
    dragActive = false;
    setTimeout(() => { seeking = false; }, 300);
  }
});

function doSeek(e) {
  if (totalMs <= 0) return;
  const rect = progressWrap.getBoundingClientRect();
  const ratio = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
  const ms = Math.floor(ratio * totalMs);
  progressBar.style.width = (ratio * 100) + '%';
  timeCur.textContent = fmt(ms);
  invoke('seek_to', { positionMs: ms }).catch(() => {});
}

// 轮询更新进度
setInterval(async () => {
  if (seeking) return;
  try {
    const pos = await invoke('get_position');
    timeCur.textContent = fmt(pos);
    if (totalMs > 0) {
      progressBar.style.width = Math.min(100, pos / totalMs * 100) + '%';
    }
    const isPlaying = await invoke('get_is_playing');
    if (isPlaying !== playing) {
      playing = isPlaying;
      setPlayIcon(playing);
    }
  } catch (e) {}
}, 200);
