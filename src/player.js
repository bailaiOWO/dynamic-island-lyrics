const { invoke } = window.__TAURI__.core;
const { open } = window.__TAURI__.dialog;
const { getCurrentWindow } = window.__TAURI__.window;

var titleEl = document.getElementById('title');
var artistEl = document.getElementById('artist');
var progressBar = document.getElementById('progressBar');
var progressWrap = document.getElementById('progressWrap');
var timeCur = document.getElementById('timeCur');
var timeTotal = document.getElementById('timeTotal');
var playBtn = document.getElementById('playBtn');
var playIcon = document.getElementById('playIcon');
var openBtn = document.getElementById('openBtn');
var volumeEl = document.getElementById('volume');
var volLabel = document.getElementById('volLabel');
var closeBtn = document.getElementById('closeBtn');
var minBtn = document.getElementById('minBtn');
var coverWrap = document.getElementById('coverWrap');
var lyricsScroll = document.getElementById('lyricsScroll');
var lyricsInner = document.getElementById('lyricsInner');
var fluidBg = document.getElementById('fluidBg');

var ICON_PLAY = '<path d="M8 5v14l11-7z"/>';
var ICON_PAUSE = '<path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/>';

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

function setPlayIcon(v) {
  playIcon.innerHTML = v ? ICON_PAUSE : ICON_PLAY;
}

// Extract dominant colors from image using canvas
function extractColors(imgSrc, callback) {
  var img = new Image();
  img.crossOrigin = 'anonymous';
  img.onload = function() {
    var canvas = document.createElement('canvas');
    var size = 50;
    canvas.width = size;
    canvas.height = size;
    var ctx = canvas.getContext('2d');
    ctx.drawImage(img, 0, 0, size, size);
    var data = ctx.getImageData(0, 0, size, size).data;
    // Sample 5 regions: center, corners
    var regions = [
      [size/2, size/2],
      [size*0.2, size*0.2],
      [size*0.8, size*0.2],
      [size*0.2, size*0.8],
      [size*0.8, size*0.8]
    ];
    var colors = [];
    for (var i = 0; i < regions.length; i++) {
      var x = Math.floor(regions[i][0]);
      var y = Math.floor(regions[i][1]);
      var idx = (y * size + x) * 4;
      colors.push([data[idx], data[idx+1], data[idx+2]]);
    }
    callback(colors);
  };
  img.onerror = function() {
    callback(null);
  };
  img.src = imgSrc;
}

function setCover(dataUrl) {
  if (dataUrl && dataUrl.length > 30) {
    coverWrap.innerHTML = '<img src="' + dataUrl + '" />';
    applyFluidBg(dataUrl);
    // Send theme color to island visualizer
    extractColors(dataUrl, function(colors) {
      if (colors && colors.length > 0) {
        var c = colors[0];
        var hex = '#' + ((1<<24)+(c[0]<<16)+(c[1]<<8)+c[2]).toString(16).slice(1);
        invoke('set_theme_color', { color: hex }).catch(function(){});
      }
    });
  } else {
    coverWrap.innerHTML = '';
    resetFluidBg();
    invoke('set_theme_color', { color: '#ffffff' }).catch(function(){});
  }
}

var fluidCanvas = null;
var fluidCtx = null;
var fluidImg = null;
var fluidAnim = null;

function applyFluidBg(dataUrl) {
  stopFluidBg();
  var img = new Image();
  img.onload = function() {
    fluidImg = img;
    if (!fluidCanvas) {
      fluidCanvas = document.createElement('canvas');
      fluidBg.innerHTML = '';
      fluidBg.appendChild(fluidCanvas);
    }
    fluidCanvas.width = 400;
    fluidCanvas.height = 400;
    fluidCtx = fluidCanvas.getContext('2d');
    var startTime = performance.now();
    // 4 copies: sizes 25%, 50%, 80%, 125%
    var copies = [
      { size: 0.25, orbit: 0.3, speed: 0.0004, rotSpeed: 0.001, phase: 0 },
      { size: 0.50, orbit: 0.2, speed: 0.0003, rotSpeed: 0.0008, phase: Math.PI * 0.5 },
      { size: 0.80, orbit: 0,   speed: 0,      rotSpeed: 0.0005, phase: Math.PI },
      { size: 1.25, orbit: 0,   speed: 0,      rotSpeed: 0.0003, phase: Math.PI * 1.5 }
    ];
    function draw() {
      var t = performance.now() - startTime;
      var w = fluidCanvas.width;
      var h = fluidCanvas.height;
      var cx = w / 2;
      var cy = h / 2;
      fluidCtx.fillStyle = '#000';
      fluidCtx.fillRect(0, 0, w, h);
      for (var i = 0; i < copies.length; i++) {
        var c = copies[i];
        var s = c.size * w;
        var ox = cx + Math.cos(t * c.speed + c.phase) * c.orbit * w;
        var oy = cy + Math.sin(t * c.speed + c.phase) * c.orbit * h;
        var rot = t * c.rotSpeed + c.phase;
        fluidCtx.save();
        fluidCtx.translate(ox, oy);
        fluidCtx.rotate(rot);
        fluidCtx.drawImage(fluidImg, -s/2, -s/2, s, s);
        fluidCtx.restore();
      }
      fluidAnim = requestAnimationFrame(draw);
    }
    draw();
  };
  img.src = dataUrl;
}

function stopFluidBg() {
  if (fluidAnim) { cancelAnimationFrame(fluidAnim); fluidAnim = null; }
}

function resetFluidBg() {
  stopFluidBg();
  fluidBg.innerHTML = '';
  fluidCanvas = null;
  fluidCtx = null;
  fluidImg = null;
}


function renderLyrics(lyricList) {
  lyrics = lyricList || [];
  lrcElements = [];
  lastActiveIdx = -1;
  lyricsInner.innerHTML = '';
  if (lyrics.length === 0) {
    lyricsInner.innerHTML = '<div class="no-lyrics">\u65e0\u6b4c\u8bcd</div>';
    return;
  }
  for (var i = 0; i < lyrics.length; i++) {
    var div = document.createElement('div');
    div.className = 'lrc-line';
    div.textContent = lyrics[i].text;
    div.setAttribute('data-idx', String(i));
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
    var dist = Math.abs(i - idx);
    if (i === idx) {
      lrcElements[i].className = 'lrc-line active';
    } else if (dist <= 2) {
      lrcElements[i].className = 'lrc-line near';
    } else {
      lrcElements[i].className = 'lrc-line';
    }
  }
  var el = lrcElements[idx];
  var scrollTop = el.offsetTop - lyricsScroll.clientHeight / 2 + el.offsetHeight / 2;
  lyricsScroll.scrollTo({ top: scrollTop, behavior: 'smooth' });
}

function updateLyricHighlight(posMs) {
  if (lyrics.length === 0) return;
  var idx = 0;
  for (var i = lyrics.length - 1; i >= 0; i--) {
    if (lyrics[i].time_ms <= posMs) { idx = i; break; }
  }
  scrollToLine(idx);
}

closeBtn.addEventListener('click', async function() {
  try { await invoke('quit_app'); } catch(e) {}
});

minBtn.addEventListener('click', async function() {
  try { await getCurrentWindow().minimize(); } catch(e) {}
});

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
    setCover(info.cover_path);
    renderLyrics(info.lyrics);
  } catch (e) {
    console.error('open_music error:', e);
    titleEl.textContent = 'Error';
    artistEl.textContent = String(e);
  }
});

playBtn.addEventListener('click', async function() {
  try {
    var nowPlaying = await invoke('play_pause');
    playing = nowPlaying;
    setPlayIcon(playing);
  } catch (e) { console.error(e); }
});

volumeEl.addEventListener('input', async function() {
  var v = volumeEl.value / 100;
  volLabel.textContent = volumeEl.value;
  try { await invoke('set_volume', { volume: v }); } catch (e) {}
});
invoke('set_volume', { volume: 0.8 }).catch(function() {});

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
