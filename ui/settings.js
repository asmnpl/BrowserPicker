'use strict';

const { invoke } = window.__TAURI__.core;

const $ = (id) => document.getElementById(id);
const list = $('browsers');
const HANDLE_SVG =
  '<svg viewBox="0 0 10 16" aria-hidden="true"><circle cx="3" cy="3" r="1.3"/><circle cx="7" cy="3" r="1.3"/>' +
  '<circle cx="3" cy="8" r="1.3"/><circle cx="7" cy="8" r="1.3"/><circle cx="3" cy="13" r="1.3"/><circle cx="7" cy="13" r="1.3"/></svg>';

// ---- Default browser status ----

function showDefault(isDefault) {
  $('status').classList.toggle('ok', isDefault);
  $('statusTitle').textContent = isDefault ? 'BrowserPicker is your default browser' : 'Not your default browser';
  $('statusText').textContent = isDefault
    ? 'Links from other apps will open the picker.'
    : 'Make BrowserPicker the default browser so links from other apps open the picker.';
}

async function refreshDefault() {
  showDefault(await invoke('is_default_browser'));
}

$('makeDefault').addEventListener('click', async () => {
  await invoke('make_default_browser');
  // The system asks the user to confirm; check back a few times.
  for (const delay of [1000, 2500, 5000]) setTimeout(refreshDefault, delay);
});
window.addEventListener('focus', refreshDefault);

// ---- General ----

function setPlacement(value) {
  const buttons = [...$('placement').querySelectorAll('button')];
  buttons.forEach((b, i) => {
    const on = b.dataset.value === value;
    b.setAttribute('aria-checked', String(on));
    if (on) $('placement').style.setProperty('--index', String(i));
  });
}

$('placement').addEventListener('click', (e) => {
  const button = e.target.closest('button');
  if (!button) return;
  setPlacement(button.dataset.value);
  invoke('set_placement', { placement: button.dataset.value });
});

$('launchAtLogin').addEventListener('change', async (e) => {
  try {
    e.target.checked = await invoke('set_launch_at_login', { enabled: e.target.checked });
  } catch (err) {
    console.error(err);
    e.target.checked = !e.target.checked;
  }
});

$('showTray').addEventListener('change', (e) => invoke('set_show_tray', { visible: e.target.checked }));
$('quit').addEventListener('click', () => invoke('quit'));

// ---- Browsers ----

function renderBrowsers(browsers) {
  list.textContent = '';
  if (!browsers.length) {
    const row = document.createElement('div');
    row.className = 'row empty-row';
    row.textContent = 'No browsers found.';
    list.append(row);
    return;
  }
  for (const browser of browsers) {
    const row = document.createElement('div');
    row.className = 'row browser-row';
    row.classList.toggle('off', browser.hidden);
    row.dataset.id = browser.id;
    row.setAttribute('role', 'listitem');

    const handle = document.createElement('span');
    handle.className = 'handle';
    handle.innerHTML = HANDLE_SVG;
    handle.title = 'Drag to reorder';

    let icon;
    if (browser.icon) {
      icon = document.createElement('img');
      icon.src = browser.icon;
      icon.alt = '';
      icon.draggable = false;
    } else {
      icon = document.createElement('span');
      icon.className = 'placeholder';
      icon.textContent = browser.name.slice(0, 1).toUpperCase();
    }

    const text = document.createElement('span');
    text.className = 'row-text';
    const title = document.createElement('span');
    title.className = 'title';
    title.textContent = browser.name;
    text.append(title);

    const toggle = document.createElement('input');
    toggle.type = 'checkbox';
    toggle.className = 'switch';
    toggle.checked = !browser.hidden;
    toggle.setAttribute('aria-label', `Show ${browser.name}`);
    toggle.addEventListener('change', () => {
      row.classList.toggle('off', !toggle.checked);
      invoke('set_browser_hidden', { id: browser.id, hidden: !toggle.checked });
    });

    row.append(handle, icon, text, toggle);
    list.append(row);
  }
}

function saveOrder() {
  const ids = [...list.querySelectorAll('.browser-row')].map((row) => row.dataset.id);
  invoke('set_browser_order', { ids });
}

// Drag-to-reorder: the grabbed row follows the pointer, the others slide out of its way.
list.addEventListener('pointerdown', (e) => {
  const handle = e.target.closest('.handle');
  if (!handle || e.button !== 0) return;
  e.preventDefault();

  const row = handle.closest('.browser-row');
  const rows = [...list.querySelectorAll('.browser-row')];
  const from = rows.indexOf(row);
  const step = row.offsetHeight;
  const startY = e.clientY;
  let to = from;

  row.classList.add('dragging');
  list.classList.add('sorting');
  handle.setPointerCapture(e.pointerId);

  const onMove = (ev) => {
    const minDy = -from * step;
    const maxDy = (rows.length - 1 - from) * step;
    const dy = Math.max(minDy, Math.min(maxDy, ev.clientY - startY));
    row.style.transform = `translateY(${dy}px) scale(1.01)`;
    to = Math.max(0, Math.min(rows.length - 1, from + Math.round(dy / step)));
    rows.forEach((other, i) => {
      if (other === row) return;
      let shift = 0;
      if (from < to && i > from && i <= to) shift = -step;
      if (from > to && i < from && i >= to) shift = step;
      other.style.transform = shift ? `translateY(${shift}px)` : '';
    });
  };

  const onUp = () => {
    handle.removeEventListener('pointermove', onMove);
    handle.removeEventListener('pointerup', onUp);
    handle.removeEventListener('pointercancel', onUp);
    list.classList.remove('sorting');

    // FLIP: move the row in the DOM, then animate it from where it was dropped.
    const before = row.getBoundingClientRect().top;
    list.classList.add('no-anim');
    rows.forEach((r) => { r.style.transform = ''; });
    if (to !== from) {
      list.insertBefore(row, to > from ? rows[to].nextSibling : rows[to]);
    }
    const after = row.getBoundingClientRect().top;
    row.style.transform = `translateY(${before - after}px) scale(1.01)`;
    void row.offsetHeight;
    list.classList.remove('no-anim');
    row.classList.remove('dragging');
    row.style.transform = '';
    if (to !== from) saveOrder();
  };

  handle.addEventListener('pointermove', onMove);
  handle.addEventListener('pointerup', onUp);
  handle.addEventListener('pointercancel', onUp);
});

$('rescan').addEventListener('click', async (e) => {
  const button = e.currentTarget;
  button.classList.remove('spinning');
  void button.offsetWidth;
  button.classList.add('spinning');
  renderBrowsers(await invoke('rescan_browsers'));
});

// ---- Startup ----

(async () => {
  const [settings, browsers] = await Promise.all([
    invoke('get_settings'),
    invoke('get_browsers', { includeHidden: true }),
  ]);
  const mac = settings.platform === 'macos';
  document.documentElement.dataset.platform = settings.platform;
  document.body.classList.toggle('material', settings.material);
  $('version').textContent = `Version ${settings.version}`;
  $('trayTitle').textContent = mac ? 'Show icon in menu bar' : 'Show icon in system tray';
  $('copyKey').textContent = mac ? '⌘C' : 'Ctrl C';
  $('launchAtLogin').checked = settings.launchAtLogin;
  $('showTray').checked = settings.showTray;
  setPlacement(settings.placement);
  showDefault(settings.isDefault);
  renderBrowsers(browsers);
  // Not in requestAnimationFrame: hidden windows may never get a frame.
  invoke('settings_ready');
})();
