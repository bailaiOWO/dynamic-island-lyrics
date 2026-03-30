const { invoke } = window.__TAURI__.core;

const island = document.getElementById("island");
const lyricCurrent = document.getElementById("lyricCurrent");
const lyricNext = document.getElementById("lyricNext");
const visBars = document.querySelectorAll(".visualizer-bar");

let isPlaying = false;
let lastLyricText = "";
let hidden = false;
let recoverTimer = null;

// mouse enter -> hide + click-through
island.addEventListener("mouseenter", async () => {
  if (hidden) return;
  hidden = true;
  island.classList.add("hidden");
  setTimeout(async () => {
    try { await invoke("set_click_through", { through: true }); } catch(e) {}
    startRecoverCheck();
  }, 280);
});

function startRecoverCheck() {
  if (recoverTimer) { clearInterval(recoverTimer); recoverTimer = null; }
  recoverTimer = setInterval(async () => {
    try {
      const inZone = await invoke("get_mouse_in_zone");
      if (inZone) return;
      clearInterval(recoverTimer);
      recoverTimer = null;
      try { await invoke("set_click_through", { through: false }); } catch(e) {}
      island.classList.remove("hidden");
      hidden = false;
    } catch(e) {}
  }, 150);
}

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
  }, 180);
}

function updateVisualizer() {
  if (!isPlaying) { visBars.forEach(b => b.style.height = '4px'); return; }
  visBars.forEach(b => b.style.height = (4 + Math.random() * 14) + 'px');
}

setInterval(async () => {
  try {
    const playing = await invoke("get_is_playing");
    if (playing !== isPlaying) isPlaying = playing;
    updateVisualizer();
    if (!isPlaying) return;
    const lyric = await invoke("get_current_lyric");
    switchLyric(lyric.current, lyric.next);
  } catch(e) {}
}, 80);

window.addEventListener("DOMContentLoaded", async () => {
  try { await invoke("center_island"); } catch(e) {}
});
