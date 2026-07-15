// Brutal Awake — frontend logic (Tauri IPC).

const T = window.__TAURI__;
const invoke = T && T.core ? T.core.invoke : null;
const appWindow = T && T.window && T.window.getCurrentWindow ? T.window.getCurrentWindow() : null;

const btn = document.getElementById('toggleBtn');
const statusTitle = document.getElementById('statusTitle');

let isOn = true;   // matches backend: keep-awake is ON at launch
let busy = false;

function render(on) {
  isOn = on;
  document.body.classList.toggle('on', on);
  btn.setAttribute('aria-checked', on ? 'true' : 'false');
  statusTitle.textContent = on ? 'AWAKE' : 'ASLEEP';
}

btn.addEventListener('click', async () => {
  if (busy) return;
  busy = true;
  const next = !isOn;
  try {
    if (invoke) await invoke('set_awake', { on: next });
    render(next);
  } catch (e) { /* keep state unchanged on failure */ }
  busy = false;
});

document.getElementById('btnMin').addEventListener('click', () => { if (appWindow) appWindow.minimize(); });
document.getElementById('btnMax').addEventListener('click', () => { if (appWindow) appWindow.toggleMaximize(); });
document.getElementById('btnClose').addEventListener('click', async () => {
  try { if (invoke) await invoke('quit'); } catch (e) {}   // restores power settings, then exits
});

render(true); // backend starts awake by default
