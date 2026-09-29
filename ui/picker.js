'use strict';

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const appWindow = window.__TAURI__.window.getCurrentWindow();

const MAX_COLUMNS = 6;
const isMac = document.documentElement.dataset.platform === 'macos';
// macOS fades the window out natively; elsewhere the CSS transition needs time to play.
const CLOSE_MS = isMac ? 0 : 130;
const CHOSEN_MS = isMac ? 160 : 170;

const $ = (id) => document.getElementById(id);
const panel = $('panel');
const grid = $('grid');
const linkEl = $('link');
const hostEl = $('host');
const restEl = $('rest');
const copyBtn = $('copy');

let browsers = [];
let items = [];
let highlight = null;
let columns = 1;
let selected = 0;
let url = '';
let isOpen = false;
let openedAt = 0;
let generation = 0; // invalidates stale open/close sequences
let pointerAnchor = null;

// The window appears under a resting cursor; only follow the mouse once it really moves.
function pointerMoved(e) {
  if (!pointerAnchor) {
    pointerAnchor = { x: e.screenX, y: e.screenY, moved: false };
  } else if (!pointerAnchor.moved) {
    pointerAnchor.moved = Math.hypot(e.screenX - pointerAnchor.x, e.screenY - pointerAnchor.y) > 3;
  }
  return pointerAnchor.moved;
}

async function loadBrowsers() {
  browsers = await invoke('get_browsers', { includeHidden: false });
}

function render() {
  grid.textContent = '';
  highlight = document.createElement('div');
  highlight.className = 'highlight';
  grid.append(highlight);

  columns = Math.max(1, Math.min(browsers.length, MAX_COLUMNS));
  grid.style.setProperty('--cols', columns);

  items = browsers.map((browser, i) => {
    const item = document.createElement('button');
    item.type = 'button';
    item.className = 'item';
    item.tabIndex = -1;
    item.setAttribute('role', 'option');
    item.style.setProperty('--i', String(i + 1));

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

    const name = document.createElement('span');
    name.className = 'name';
    name.textContent = browser.name;

    item.append(icon, name);
    if (i < 9) {
      const key = document.createElement('kbd');
      key.textContent = String(i + 1);
      item.append(key);
    }

    item.addEventListener('mousemove', (e) => {
      if (!pointerMoved(e)) return;
      document.body.classList.remove('keyboard');
      if (selected !== i) select(i);
    });
    item.addEventListener('click', () => choose(i));
    return item;
  });

  if (items.length) {
    grid.append(...items);
  } else {
    const empty = document.createElement('div');
    empty.className = 'empty';
    empty.textContent = 'No browsers found. Check BrowserPicker settings.';
    grid.append(empty);
    highlight.style.opacity = '0';
  }
}

function setLink(value) {
  let host = '';
  let rest = value;
  try {
    const parsed = new URL(value);
    if (parsed.protocol === 'file:') {
      host = 'file';
      rest = ' ' + decodeURIComponent(parsed.pathname);
    } else {
      host = parsed.host;
      rest = (parsed.pathname === '/' ? '' : parsed.pathname) + parsed.search + parsed.hash;
    }
  } catch {
    // Not a URL we can pretty-print; show it verbatim.
  }
  hostEl.textContent = host;
  restEl.textContent = rest;
  linkEl.title = value;
}

function select(index, { instant = false } = {}) {
  if (!items.length) return;
  selected = Math.max(0, Math.min(index, items.length - 1));
  items.forEach((item, i) => item.setAttribute('aria-selected', String(i === selected)));

  const item = items[selected];
  highlight.classList.toggle('instant', instant);
  highlight.style.width = `${item.offsetWidth}px`;
  highlight.style.height = `${item.offsetHeight}px`;
  highlight.style.transform = `translate(${item.offsetLeft}px, ${item.offsetTop}px)`;
  if (instant) {
    void highlight.offsetWidth;
    highlight.classList.remove('instant');
  }
}

async function open(value) {
  url = value;
  setLink(value);
  if (isOpen) return; // Already visible: just show the new link.

  const token = ++generation;
  document.body.dataset.state = 'idle';
  document.body.classList.remove('keyboard');
  copyBtn.classList.remove('done');
  pointerAnchor = null;
  render();
  select(0, { instant: true });

  const margin = parseFloat(getComputedStyle(document.body).paddingLeft) || 0;
  const width = Math.ceil(panel.offsetWidth + margin * 2);
  const height = Math.ceil(panel.offsetHeight + margin * 2);
  try {
    await invoke('present_picker', { width, height });
  } catch (err) {
    console.error(err);
    return;
  }
  if (token !== generation) return;

  isOpen = true;
  openedAt = performance.now();
  requestAnimationFrame(() => {
    if (token === generation) document.body.dataset.state = 'open';
  });
}

function close(state = 'closing') {
  if (!isOpen) return;
  isOpen = false;
  const token = ++generation;
  document.body.dataset.state = state;
  const delay = state === 'chosen' ? CHOSEN_MS : CLOSE_MS;
  setTimeout(() => {
    if (token !== generation) return;
    invoke('hide_picker');
    document.body.dataset.state = 'idle';
  }, delay);
}

function choose(index) {
  const browser = browsers[index];
  if (!isOpen || !browser) return;
  select(index);
  items[index].classList.add('launching');
  invoke('open_url', { id: browser.id, url }).catch((err) => console.error(err));
  close('chosen');
}

async function copyLink() {
  try {
    await navigator.clipboard.writeText(url);
    copyBtn.classList.add('done');
    setTimeout(() => copyBtn.classList.remove('done'), 1200);
  } catch (err) {
    console.error(err);
  }
}

function digitFromEvent(e) {
  const match = /^(?:Digit|Numpad)([1-9])$/.exec(e.code) || /^([1-9])$/.exec(e.key);
  return match ? Number(match[1]) : 0;
}

window.addEventListener('keydown', (e) => {
  if (!isOpen) return;
  const mod = e.metaKey || e.ctrlKey;

  if (mod && e.code === 'KeyC') {
    copyLink();
    e.preventDefault();
    return;
  }

  const moveTo = (index) => {
    document.body.classList.add('keyboard');
    select(index);
  };

  switch (e.key) {
    case 'Escape': close(); break;
    case 'Enter':
    case ' ': choose(selected); break;
    case 'ArrowRight': moveTo((selected + 1) % items.length); break;
    case 'ArrowLeft': moveTo((selected - 1 + items.length) % items.length); break;
    case 'ArrowDown': if (selected + columns < items.length) moveTo(selected + columns); break;
    case 'ArrowUp': if (selected - columns >= 0) moveTo(selected - columns); break;
    case 'Tab': moveTo((selected + (e.shiftKey ? -1 : 1) + items.length) % items.length); break;
    case 'Home': moveTo(0); break;
    case 'End': moveTo(items.length - 1); break;
    default: {
      const digit = digitFromEvent(e);
      if (!digit || mod) return;
      choose(digit - 1);
    }
  }
  e.preventDefault();
});

copyBtn.addEventListener('click', copyLink);

// Clicks in the transparent margin around the panel dismiss the picker.
document.addEventListener('mousedown', (e) => {
  if (isOpen && !panel.contains(e.target)) close();
});

(async () => {
  await loadBrowsers();
  await listen('browsers:changed', loadBrowsers);
  await listen('picker:open', (e) => open(e.payload));
  await appWindow.onFocusChanged(({ payload: focused }) => {
    // Ignore the focus shuffle that happens while the window appears.
    if (!focused && isOpen && performance.now() - openedAt > 150) close();
  });
  const init = await invoke('picker_loaded');
  document.documentElement.dataset.platform = init.platform;
  if (init.pending) open(init.pending);
})();
