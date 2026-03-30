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
// ==================== List View (Playlists <-> Tracks) ====================

var listContent = document.getElementById('listContent');
var listTitle = document.getElementById('listTitle');
var listActionBtn = document.getElementById('listActionBtn');
var contextMenu = document.getElementById('contextMenu');

var VIEW_PLAYLISTS = 'playlists';
var VIEW_TRACKS = 'tracks';
var currentView = VIEW_PLAYLISTS;
var playlists = [];
var activePlaylistId = null;
var trackList = [];
var currentTrackIdx = -1;
var contextTargetId = null;

function escHtml(s) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

// ---- Playlist view ----
async function showPlaylistView() {
  currentView = VIEW_PLAYLISTS;
  listTitle.textContent = '\u6b4c\u5355';
  listActionBtn.textContent = '\u65b0\u5efa\u6b4c\u5355';
  listActionBtn.onclick = createNewPlaylist;
  try { playlists = await invoke('list_playlists'); } catch(e) {}
  renderPlaylistList();
}

function renderPlaylistList() {
  listContent.innerHTML = '';
  for (var i = 0; i < playlists.length; i++) {
    var pl = playlists[i];
    var div = document.createElement('div');
    div.className = 'pl-item';
    div.setAttribute('data-plid', pl.id);
    var iconHtml = pl.cover_path
      ? '<img src="' + escHtml(pl.cover_path) + '" />'
      : '<svg viewBox="0 0 24 24"><path d="M12 3v10.55c-.59-.34-1.27-.55-2-.55C7.79 13 6 14.79 6 17s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"/></svg>';
    div.innerHTML = '<div class="pl-icon">' + iconHtml + '</div>'
      + '<div class="pl-info"><div class="pl-name">' + escHtml(pl.name) + '</div>'
      + '<div class="pl-count">' + pl.track_count + ' tracks</div></div>';
    div.addEventListener('dblclick', function() {
      openPlaylist(this.getAttribute('data-plid'));
    });
    div.addEventListener('contextmenu', function(e) {
      e.preventDefault();
      contextTargetId = this.getAttribute('data-plid');
      contextMenu.style.left = e.clientX + 'px';
      contextMenu.style.top = e.clientY + 'px';
      contextMenu.classList.add('show');
    });
    // Drag & drop: accept audio files
    div.addEventListener('dragover', function(e) {
      e.preventDefault(); this.classList.add('dragover');
    });
    div.addEventListener('dragleave', function() {
      this.classList.remove('dragover');
    });
    div.addEventListener('drop', function(e) {
      e.preventDefault(); this.classList.remove('dragover');
      var id = this.getAttribute('data-plid');
      var files = e.dataTransfer.files;
      var paths = [];
      for (var j = 0; j < files.length; j++) paths.push(files[j].path);
      if (paths.length > 0) {
        invoke('add_tracks_to_playlist', { id: id, paths: paths }).then(function() {
          showPlaylistView();
        });
      }
    });
    listContent.appendChild(div);
  }
}

async function createNewPlaylist() {
  try {
    await invoke('create_playlist', { name: 'New Playlist' });
    await showPlaylistView();
  } catch(e) {}
}

async function openPlaylist(id) {
  activePlaylistId = id;
  currentView = VIEW_TRACKS;
  var pl = playlists.find(function(p) { return p.id === id; });
  listTitle.textContent = pl ? pl.name : 'Playlist';
  listActionBtn.textContent = '\u2190 \u8fd4\u56de\u6b4c\u5355';
  listActionBtn.onclick = showPlaylistView;
  try {
    trackList = await invoke('get_playlist_tracks', { id: id });
    currentTrackIdx = -1;
    renderTrackList();
  } catch(e) { console.error(e); }
}

// ---- Track view ----
function renderTrackList() {
  listContent.innerHTML = '';
  if (trackList.length === 0) {
    listContent.innerHTML = '<div style="padding:20px;text-align:center;color:rgba(255,255,255,0.2);font-size:12px">\u62d6\u52a8\u97f3\u9891\u6587\u4ef6\u5230\u6b4c\u5355\u6dfb\u52a0\u6b4c\u66f2</div>';
    return;
  }
  for (var i = 0; i < trackList.length; i++) {
    var t = trackList[i];
    var div = document.createElement('div');
    div.className = 'track-item' + (i === currentTrackIdx ? ' active' : '');
    div.setAttribute('data-tidx', String(i));
    div.innerHTML = '<div class="track-info">'
      + '<div class="track-name">' + escHtml(t.title) + '</div>'
      + '<div class="track-artist">' + escHtml(t.artist) + '</div></div>'
      + '<button class="track-match-btn' + (t.has_local_lyrics ? ' track-has-lyrics' : '') + '" data-midx="' + i + '">'
      + '<svg viewBox="0 0 24 24"><path d="M14 2H6c-1.1 0-2 .9-2 2v16c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V8l-6-6zm-1 9h-2v2H9v-2H7v-2h2V7h2v2h2v2zm0-6V3.5L18.5 9H13z"/></svg></button>';
    div.addEventListener('click', function(e) {
      if (e.target.closest('.track-match-btn')) return;
      playTrackByIndex(parseInt(this.getAttribute('data-tidx')));
    });
    listContent.appendChild(div);
  }
  // Match buttons
  listContent.querySelectorAll('.track-match-btn').forEach(function(btn) {
    btn.addEventListener('click', function(e) {
      e.stopPropagation();
      matchSingleTrack(parseInt(this.getAttribute('data-midx')), this);
    });
  });
}

async function playTrackByIndex(idx) {
  if (idx < 0 || idx >= trackList.length) return;
  currentTrackIdx = idx;
  var t = trackList[idx];
  try {
    var info = await invoke('open_music', { path: t.path });
    titleEl.textContent = info.title;
    artistEl.textContent = info.artist;
    playing = true; setPlayIcon(true);
    totalMs = info.duration_ms > 0 ? info.duration_ms : 0;
    timeTotal.textContent = totalMs > 0 ? fmt(totalMs) : '--:--';
    setCover(info.cover_path);
    renderLyrics(info.lyrics);
    renderTrackList();
    renderNowPlaylist();
  } catch(e) { console.error(e); }
}

async function matchSingleTrack(idx, btnEl) {
  if (idx < 0 || idx >= trackList.length) return;
  btnEl.style.opacity = '1';
  try {
    await invoke('match_lyrics_for_track', { path: trackList[idx].path });
    trackList[idx].has_local_lyrics = true;
    btnEl.className = 'track-match-btn track-has-lyrics';
    btnEl.innerHTML = '<svg viewBox="0 0 24 24"><path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z"/></svg>';
  } catch(e) {
    btnEl.innerHTML = '<svg viewBox="0 0 24 24"><path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"/></svg>';
    setTimeout(function() {
      btnEl.innerHTML = '<svg viewBox="0 0 24 24"><path d="M14 2H6c-1.1 0-2 .9-2 2v16c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V8l-6-6zm-1 9h-2v2H9v-2H7v-2h2V7h2v2h2v2zm0-6V3.5L18.5 9H13z"/></svg>';
      btnEl.style.opacity = '';
    }, 2000);
  }
}

// Prev/Next
document.getElementById('prevBtn').addEventListener('click', function() {
  if (trackList.length > 0 && currentTrackIdx > 0) playTrackByIndex(currentTrackIdx - 1);
});
document.getElementById('nextBtn').addEventListener('click', function() {
  if (trackList.length > 0 && currentTrackIdx < trackList.length - 1) playTrackByIndex(currentTrackIdx + 1);
});

// ---- Context menu ----
document.addEventListener('click', function() { contextMenu.classList.remove('show'); });

contextMenu.querySelectorAll('.ctx-item').forEach(function(item) {
  item.addEventListener('click', async function() {
    var action = this.getAttribute('data-action');
    if (!contextTargetId) return;
    contextMenu.classList.remove('show');
    if (action === 'rename') {
      var n = prompt('Playlist name:');
      if (n) { await invoke('rename_playlist', { id: contextTargetId, name: n }); await showPlaylistView(); }
    } else if (action === 'cover') {
      var r = await open({ filters: [{ name: 'Image', extensions: ['jpg','jpeg','png','webp'] }] });
      if (r) {
        var p = typeof r === 'string' ? r : (r.path || r.toString());
        if (p) { await invoke('set_playlist_cover', { id: contextTargetId, coverPath: p }); await showPlaylistView(); }
      }
    } else if (action === 'addfiles') {
      var r2 = await open({ multiple: true, filters: [{ name: 'Audio', extensions: ['mp3','wav','flac','ogg','m4a','aac','wma'] }] });
      if (r2) {
        var paths = Array.isArray(r2) ? r2.map(function(x) { return typeof x === 'string' ? x : (x.path || x.toString()); }) : [typeof r2 === 'string' ? r2 : (r2.path || r2.toString())];
        await invoke('add_tracks_to_playlist', { id: contextTargetId, paths: paths });
        await showPlaylistView();
      }
    } else if (action === 'addfolder') {
      var r3 = await open({ directory: true });
      if (r3) {
        var folder = typeof r3 === 'string' ? r3 : (r3.path || r3.toString());
        var tracks = await invoke('scan_folder', { folder: folder });
        var paths2 = tracks.map(function(t) { return t.path; });
        if (paths2.length > 0) {
          await invoke('add_tracks_to_playlist', { id: contextTargetId, paths: paths2 });
          await showPlaylistView();
        }
      }
    } else if (action === 'delete') {
      await invoke('delete_playlist', { id: contextTargetId });
      if (activePlaylistId === contextTargetId) { activePlaylistId = null; }
      await showPlaylistView();
    }
  });
});

// ---- Open folder = create playlist from folder ----
openBtn.addEventListener('click', async function() {
  try {
    var result = await open({
      filters: [{ name: 'Audio', extensions: ['mp3','wav','flac','ogg','m4a','aac','wma'] }]
    });
    if (!result) return;
    var path = typeof result === 'string' ? result : (result.path || result.toString());
    if (!path) return;
    var info = await invoke('open_music', { path: path });
    titleEl.textContent = info.title;
    artistEl.textContent = info.artist;
    playing = true; setPlayIcon(true);
    totalMs = info.duration_ms > 0 ? info.duration_ms : 0;
    timeTotal.textContent = totalMs > 0 ? fmt(totalMs) : '--:--';
    setCover(info.cover_path);
    renderLyrics(info.lyrics);
  } catch(e) { console.error(e); }
});

document.getElementById('openFolderBtn').addEventListener('click', async function() {
  try {
    var result = await open({ directory: true });
    if (!result) return;
    var folder = typeof result === 'string' ? result : (result.path || result.toString());
    if (!folder) return;
    // Get folder name
    var parts = folder.replace(/\\/g, '/').split('/');
    var folderName = parts[parts.length - 1] || parts[parts.length - 2] || 'Folder';
    // Scan tracks
    var tracks = await invoke('scan_folder', { folder: folder });
    if (tracks.length === 0) return;
    // Create playlist with folder name
    var id = await invoke('create_playlist', { name: folderName });
    // Add all tracks
    var paths = tracks.map(function(t) { return t.path; });
    await invoke('add_tracks_to_playlist', { id: id, paths: paths });
    // Open it
    await showPlaylistView();
    openPlaylist(id);
  } catch(e) { console.error(e); }
});

// ==================== Sidebar Tab Switch ====================

var tabPlay = document.getElementById('tabPlay');
var tabLibrary = document.getElementById('tabLibrary');
var pagePlay = document.getElementById('pagePlay');
var pageLibrary = document.getElementById('pageLibrary');

function switchTab(tab) {
  if (tab === 'play') {
    pagePlay.classList.remove('hidden');
    pageLibrary.classList.add('hidden');
    tabPlay.classList.add('active');
    tabLibrary.classList.remove('active');
  } else {
    pagePlay.classList.add('hidden');
    pageLibrary.classList.remove('hidden');
    tabPlay.classList.remove('active');
    tabLibrary.classList.add('active');
    showPlaylistView();
  }
}

tabPlay.addEventListener('click', function() { switchTab('play'); });
tabLibrary.addEventListener('click', function() { switchTab('library'); });

// Init
showPlaylistView();

// ==================== Now Playing List (in play page) ====================

var nowPlaylistEl = document.getElementById('nowPlaylist');

function renderNowPlaylist() {
  nowPlaylistEl.innerHTML = '';
  if (trackList.length === 0) return;
  for (var i = 0; i < trackList.length; i++) {
    var t = trackList[i];
    var div = document.createElement('div');
    div.className = 'now-pl-item' + (i === currentTrackIdx ? ' active' : '');
    div.setAttribute('data-nidx', String(i));
    div.textContent = t.title;
    div.addEventListener('click', function() {
      playTrackByIndex(parseInt(this.getAttribute('data-nidx')));
    });
    nowPlaylistEl.appendChild(div);
  }
  // Scroll active into view
  var activeEl = nowPlaylistEl.querySelector('.active');
  if (activeEl) activeEl.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
}



// ==================== Polling ====================

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
