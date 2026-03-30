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
const lyricsScroll = document.getElementById('lyricsScroll');
const lyricsInner = document.getElementById('lyricsInner');
const noLyrics = document.getElementById('noLyrics');

const ICON_PLAY = '<path d="M8 5v14l11-7z"/>';
const ICON_PAUSE = '<path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/>';

var playing = false;
var totalMs = 0;
var seeking = false;
var lyrics = [];
var lrcElements = [];
var lastActiveIdx = -1;

function fmt(ms) {
  var s = Math.floor(ms / 1000);
  var m = Math.floor(s / 60);
  return m + ':' + String(s % 60).padStart(2, '0');
}

function setPlayIcon(isPlaying) {
  playIcon.innerHTML = isPlaying ? ICON_PAUSE : ICON_PLAY;
}

function renderLyrics(lyricList) {
  lyrics = lyricList || [];
  lrcElements = [];
  lastActiveIdx = -1;
  lyricsInner.innerHTML = '';

  if (lyrics.length === 0) {
    lyricsInner.innerHTML = '<div class="no-lyrics">无歌词</div>';
    return;
  }

  for (var i = 0; i < lyrics.length; i++) {
    var div = document.createElement('div');
    div.className = 'lrc-line';
    div.textContent = lyrics[i].text;
    div.setAttribute('data-idx', i);
    div.addEventListener('click', function() {
      var idx = parseInt(this.getAttribute('data-idx'));
      if (idx >= 0 && idx < lyrics.length) {
        invoke('seek_to', { positionMs: lyrics[idx].time_ms }).catch(function() {});
      }
    });
    lyricsInner.appendChild(div);
    lrcElements.push(div);
  }
}

function scrollToLine(idx) {
  if (idx < 0 || idx >= lrcElements.length) return;
  if (idx === lastActiveIdx) return;
  lastActiveIdx = idx;

  for (var i = 0; i < lrcElements.length; i++) {
    var el = lrcElements[i];
    var dist = Math.abs(i - idx);
    if (i === idx) {
      el.className = 'lrc-line active';
    } else if (dist <= 2) {
      el.className = 'lrc-line near';
    } else {
      el.className = 'lrc-line';
    }
  }

  var scrollH = lyricsScroll.clientHeight;
  var lineEl = lrcElements[idx];
  var lineTop = lineEl.offsetTop;
  var lineH = lineEl.offsetHeight;
  var targetY = -(lineTop - scrollH / 2 + lineH / 2);
  lyricsInner.style.transform = 'translateY(' + targetY + 'px)';
}

function updateLyricHighlight(posMs) {
  if (lyrics.length === 0) return;
  var idx = 0;
  for (var i = lyrics.length - 1; i >= 0; i--) {
    if (lyrics[i].time_ms <= posMs) { idx = i; break; }
  }
  scrollToLine(idx);
}

// Close = quit app
closeBtn.addEventListener('click', async function() {
  try { await invoke('quit_app'); } catch(e) {}
});

// Minimize
minBtn.addEventListener('click', async function() {
  try { await getCurrentWindow().minimize(); } catch(e) {}
});

// Open file
openBtn.addEventListener('click', async function() {
  try {
    var result = await open({
      filters: [{ name: 'Audio', extensions: ['mp3', 'wav', 'flac', 'ogg', 'm4a', 'aac', 'wma'] }]
    });
    if (!result) return;
    var path = typeof result === 'string' ? result : (result.path || result.toString());
    if (!path) return;
    var info = await invoke('open_music', { path: path });
    titleEl.textContent = info.title;
    artistEl.textContent = info.artist;
    playing = true;
    setPlayIcon(true);
    if (info.duration_ms > 0) {
      totalMs = info.duration_ms;
    } else if (info.lyrics && info.lyrics.length > 0) {
      totalMs = info.lyrics[info.lyrics.length - 1].time_ms + 10000;
    } else {
      totalMs = 0;
    }
    timeTotal.textContent = totalMs > 0 ? fmt(totalMs) : '--:--';
    renderLyrics(info.lyrics);
  } catch (e) {
    console.error('open_music error:', e);
    titleEl.textContent = 'Error';
    artistEl.textContent = String(e);
  }
});

// Play/pause
playBtn.addEventListener('click', async function() {
  try {
    var nowPlaying = await invoke('play_pause');
    playing = nowPlaying;
    setPlayIcon(playing);
  } catch (e) { console.error(e); }
});

// Volume
volumeEl.addEventListener('input', async function() {
  var v = volumeEl.value / 100;
  volLabel.textContent = volumeEl.value;
  try { await invoke('set_volume', { volume: v }); } catch (e) {}
});
invoke('set_volume', { volume: 0.8 }).catch(function() {});

// Progress seek
var dragActive = false;
progressWrap.addEventListener('mousedown', function(e) {
  dragActive = true; seeking = true; doSeek(e);
});
document.addEventListener('mousemove', function(e) { if (dragActive) doSeek(e); });
document.addEventListener('mouseup', function() {
  if (dragActive) { dragActive = false; setTimeout(function() { seeking = false; }, 300); }
});

function doSeek(e) {
  if (totalMs <= 0) return;
  var rect = progressWrap.getBoundingClientRect();
  var ratio = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
  var ms = Math.floor(ratio * totalMs);
  progressBar.style.width = (ratio * 100) + '%';
  timeCur.textContent = fmt(ms);
  invoke('seek_to', { positionMs: ms }).catch(function() {});
}

// Poll update
setInterval(async function() {
  if (seeking) return;
  try {
    var pos = await invoke('get_position');
    timeCur.textContent = fmt(pos);
    if (totalMs > 0) {
      progressBar.style.width = Math.min(100, pos / totalMs * 100) + '%';
    }
    var isP = await invoke('get_is_playing');
    if (isP !== playing) { playing = isP; setPlayIcon(playing); }
    updateLyricHighlight(pos);
  } catch (e) {}
}, 100);
