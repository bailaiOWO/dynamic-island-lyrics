const { invoke } = window.__TAURI__.core;

const island = document.getElementById("island");
const lyricCurrent = document.getElementById("lyricCurrent");
const lyricNext = document.getElementById("lyricNext");
const visBars = document.querySelectorAll(".visualizer-bar");

let isPlaying = false;
let lastLyricText = "";
let currentThemeColor = "#30d158";
let currentIslandHeight = 44;
let hidden = false;

// Poll mouse position - hide when cursor is near top
setInterval(async () => {
  try {
    const inZone = await invoke("get_mouse_in_zone");
    if (inZone && !hidden) {
      hidden = true;
      island.classList.add("hidden");
    } else if (!inZone && hidden) {
      hidden = false;
      island.classList.remove("hidden");
    }
  } catch(e) {}
}, 150);

function switchLyric(current, next) {
  if (current === lastLyricText) return;
  lastLyricText = current;
  lyricCurrent.classList.add("out");
  lyricNext.classList.add("out");
  setTimeout(() => {
    lyricCurrent.textContent = current;
    lyricNext.textContent = next;
    lyricCurrent.classList.remove("out");
    lyricNext.classList.remove("out");
    requestAnimationFrame(() => {
      const w = island.getBoundingClientRect().width;
      invoke("set_island_width", { width: w }).catch(() => {});
    });
  }, 180);
}

function updateVisualizer() {
  visBars.forEach(b => b.style.background = currentThemeColor);
  if (!isPlaying) { visBars.forEach(b => b.style.height = '4px'); return; }
  visBars.forEach(b => b.style.height = (4 + Math.random() * 14) + 'px');
}

// Main playback loop
setInterval(async () => {
  try {
    const playing = await invoke("get_is_playing");
    if (playing !== isPlaying) isPlaying = playing;
    updateVisualizer();
    if (isPlaying) {
      const lyric = await invoke("get_current_lyric");
      switchLyric(lyric.current, lyric.next);
    }
  } catch(e) {}
}, 80);

// Settings sync loop (always runs, even when not playing)
setInterval(async () => {
  try {
    const tc = await invoke("get_theme_color");
    if (tc && tc !== currentThemeColor) currentThemeColor = tc;
  } catch(e) {}
  try {
    const font = await invoke("get_island_font");
    if (font && font.length > 0) {
      document.querySelector('.lyrics-area').style.fontFamily = font;
    }
  } catch(e) {}
  try {
    const h = await invoke("get_island_height");
    if (h && h !== currentIslandHeight) {
      currentIslandHeight = h;
      const scale = h / 44;
      island.style.height = h + 'px';
      island.style.borderRadius = '0 0 ' + Math.round(20 * scale) + 'px ' + Math.round(20 * scale) + 'px';
      lyricCurrent.style.fontSize = Math.round(13 * scale) + 'px';
      lyricNext.style.fontSize = Math.round(10.5 * scale) + 'px';
    }
  } catch(e) {}
}, 500);

window.addEventListener("DOMContentLoaded", async () => {
  try { await invoke("center_island"); } catch(e) {}
});
