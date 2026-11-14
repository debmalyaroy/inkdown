'use strict';
/* Core: IPC bridge, DOM helpers, icons, popovers, dialogs, toasts. */

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));

// ---------------------------------------------------------------- IPC
const host = (() => {
  let seq = 0;
  const pending = new Map();
  window.__inkReply = (id, ok, payload) => {
    const p = pending.get(id);
    if (!p) return;
    pending.delete(id);
    ok ? p.resolve(payload) : p.reject(new Error(payload));
  };
  return {
    invoke(cmd, args = {}) {
      return new Promise((resolve, reject) => {
        const id = ++seq;
        pending.set(id, { resolve, reject });
        window.ipc.postMessage(JSON.stringify({ id, cmd, args }));
      });
    },
    send(cmd, args = {}) {
      window.ipc.postMessage(JSON.stringify({ id: 0, cmd, args }));
    },
  };
})();

// -------------------------------------------------------------- icons
const ICONS = {
  menu: '<path d="M4 6h16M4 12h16M4 18h16"/>',
  sidebar: '<rect x="3" y="3" width="18" height="18" rx="2.5"/><path d="M9 3v18"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  x: '<path d="M18 6 6 18M6 6l12 12"/>',
  read: '<path d="M2 4h6a4 4 0 0 1 4 4v13a3 3 0 0 0-3-3H2zM22 4h-6a4 4 0 0 0-4 4v13a3 3 0 0 1 3-3h7z"/>',
  split: '<rect x="3" y="3" width="18" height="18" rx="2.5"/><path d="M12 3v18"/>',
  edit: '<path d="M12 20h9M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M6.3 17.7l-1.4 1.4M19.1 4.9l-1.4 1.4"/>',
  moon: '<path d="M21 12.8A9 9 0 1 1 11.2 3 7 7 0 0 0 21 12.8z"/>',
  bold: '<path d="M6 4h8a4 4 0 0 1 0 8H6zM6 12h9a4 4 0 0 1 0 8H6z"/>',
  italic: '<path d="M19 4h-9M14 20H5M15 4 9 20"/>',
  strike: '<path d="M16 4H9a3 3 0 0 0-2.8 4M14 12a4 4 0 0 1 0 8H6M4 12h16"/>',
  highlight: '<path d="m9 11-6 6v3h9l3-3"/><path d="m22 12-4.6 4.6a2 2 0 0 1-2.8 0l-5.2-5.2a2 2 0 0 1 0-2.8L14 4"/>',
  heading: '<path d="M6 4v16M18 4v16M6 12h12"/>',
  link: '<path d="M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.800 1.700M14 11a5 5 0 0 0-7.500-.5l-3 3a5 5 0 0 0 7 7l1.700-1.700"/>',
  image: '<rect x="3" y="3" width="18" height="18" rx="2.5"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.100-3.100a2 2 0 0 0-2.800 0L6 21"/>',
  code: '<path d="m16 18 6-6-6-6M8 6l-6 6 6 6"/>',
  codeblock: '<rect x="3" y="3" width="18" height="18" rx="2.5"/><path d="m10 9-3 3 3 3M14 9l3 3-3 3"/>',
  quote: '<path d="M17 6H3M21 12H8M21 18H8M3 12v6"/>',
  ul: '<path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"/>',
  ol: '<path d="M10 6h11M10 12h11M10 18h11M4 6h1v4M4 10h2M6 18H4c0-1 2-2 2-3s-1-1.500-2-1"/>',
  task: '<path d="m9 11 3 3L22 4M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/>',
  table: '<rect x="3" y="3" width="18" height="18" rx="2.5"/><path d="M3 9h18M3 15h18M12 3v18"/>',
  hr: '<path d="M5 12h14"/>',
  undo: '<path d="M3 7v6h6M21 17a9 9 0 0 0-9-9 9 9 0 0 0-6 2.300L3 13"/>',
  redo: '<path d="M21 7v6h-6M3 17a9 9 0 0 1 9-9 9 9 0 0 1 6 2.300l3 2.700"/>',
  search: '<circle cx="11" cy="11" r="7.500"/><path d="m21 21-4.300-4.300"/>',
  copy: '<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  expand: '<path d="M8 3H5a2 2 0 0 0-2 2v3M21 8V5a2 2 0 0 0-2-2h-3M3 16v3a2 2 0 0 0 2 2h3M16 21h3a2 2 0 0 0 2-2v-3"/>',
  download: '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3"/>',
  file: '<path d="M14.500 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.500zM14 2v6h6"/>',
  filePlus: '<path d="M14.500 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.500zM14 2v6h6M12 12v6M9 15h6"/>',
  folder: '<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.900a2 2 0 0 1-1.700-.9l-.8-1.200A2 2 0 0 0 7.900 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z"/>',
  up: '<path d="m18 15-6-6-6 6"/>',
  down: '<path d="m6 9 6 6 6-6"/>',
  left: '<path d="m15 18-6-6 6-6"/>',
  right: '<path d="m9 18 6-6-6-6"/>',
  sliders: '<path d="M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M2 14h4M10 8h4M18 16h4"/>',
  sigma: '<path d="M18 7V4H6l6 8-6 8h12v-3"/>',
  diagram: '<rect x="3" y="3" width="8" height="8" rx="2"/><path d="M7 11v4a2 2 0 0 0 2 2h4"/><rect x="13" y="13" width="8" height="8" rx="2"/>',
  save: '<path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2zM17 21v-8H7v8M7 3v5h8"/>',
  print: '<path d="M6 9V3h12v6M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2"/><rect x="6" y="14" width="12" height="8" rx="1"/>',
  external: '<path d="M15 3h6v6M10 14 21 3M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>',
  window: '<rect x="3" y="4" width="18" height="16" rx="2.500"/><path d="M3 9h18"/>',
  sparkles: '<path d="M12 3l1.900 5.100L19 10l-5.100 1.900L12 17l-1.900-5.100L5 10l5.100-1.900zM19 15l.9 2.100L22 18l-2.100.9L19 21l-.9-2.100L16 18l2.100-.9z"/>',
  zoomIn: '<circle cx="11" cy="11" r="7.500"/><path d="m21 21-4.300-4.300M11 8v6M8 11h6"/>',
  zoomOut: '<circle cx="11" cy="11" r="7.500"/><path d="m21 21-4.300-4.300M8 11h6"/>',
  reset: '<path d="M3 12a9 9 0 1 0 3-6.700L3 8M3 3v5h5"/>',
  command: '<path d="M15 6v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3V6a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3"/>',
  info: '<circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/>',
  chart: '<path d="M3 3v18h18M7 16v-5M12 16V8M17 16v-8"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
};
function icon(name, cls = '') {
  return `<svg class="i ${cls}" viewBox="0 0 24 24" aria-hidden="true">${ICONS[name] || ''}</svg>`;
}
function hydrateIcons(root = document) {
  for (const el of $$('[data-icon]', root)) {
    if (!el.firstElementChild) el.insertAdjacentHTML('afterbegin', icon(el.dataset.icon));
  }
}

// ------------------------------------------------------------ helpers
function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k === 'html') el.innerHTML = v;
    else if (k === 'text') el.textContent = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (k === 'dataset') Object.assign(el.dataset, v);
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const c of children.flat()) {
    if (c == null || c === false) continue;
    el.append(c.nodeType ? c : document.createTextNode(c));
  }
  return el;
}
const debounce = (fn, ms) => {
  let t;
  const d = (...a) => { clearTimeout(t); t = setTimeout(() => fn(...a), ms); };
  d.cancel = () => clearTimeout(t);
  d.flush = (...a) => { clearTimeout(t); fn(...a); };
  return d;
};
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const escapeHtml = (s) => s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const escapeRegex = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

// Windows path helpers (everything is kept with backslashes internally)
const pathx = {
  /** '\\' on Windows, '/' elsewhere; set from the host at start-up. */
  sep: '\\',
  norm(p) {
    const s = pathx.sep;
    const parts = (s === '\\' ? p.replace(/\//g, '\\') : p).split(s);
    const out = [];
    for (const part of parts) {
      if (part === '.' || (part === '' && out.length > 1)) continue;
      if (part === '..' && out.length > 1) out.pop();
      else out.push(part);
    }
    return out.join(s) || s;
  },
  isAbs: (p) => (pathx.sep === '/' ? p.startsWith('/') : /^[a-zA-Z]:[\\/]/.test(p) || /^[\\/]{2}/.test(p)),
  join: (dir, rel) => pathx.norm(dir.replace(/[\\/]+$/, '') + pathx.sep + rel),
  /** Local path from a file:// URL. */
  fromFileUrl: (u) => (pathx.sep === '/' ? u.replace(/^file:\/\//i, '') : u.replace(/^file:\/\/\//i, '')),
  toFileUrl: (p) => (pathx.sep === '/' ? 'file://' + p : 'file:///' + p.replace(/\\/g, '/')),
  dirname: (p) => p.replace(/[\\/][^\\/]*$/, ''),
  basename: (p) => p.replace(/^.*[\\/]/, ''),
  ext: (p) => (/\.([^.\\/]+)$/.exec(p) || [, ''])[1].toLowerCase(),
  same: (a, b) => pathx.norm(a).toLowerCase() === pathx.norm(b).toLowerCase(),
  /** Relative path from `dir` to `target`, with forward slashes; null if on another drive. */
  relative(dir, target) {
    const a = pathx.norm(dir).split(pathx.sep), b = pathx.norm(target).split(pathx.sep);
    if (a[0].toLowerCase() !== b[0].toLowerCase()) return null;
    let i = 0;
    while (i < a.length && i < b.length && a[i].toLowerCase() === b[i].toLowerCase()) i++;
    return [...Array(a.length - i).fill('..'), ...b.slice(i)].join('/');
  },
  fileUrl: (p) => '/__file/' + encodeURIComponent(p),
};
const MD_EXT = ['md', 'markdown', 'mdown', 'mkd', 'mkdn', 'mdwn', 'mdx'];
const IMG_EXT = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico', 'avif', 'apng'];

function loadScript(src) {
  loadScript.cache ??= new Map();
  if (!loadScript.cache.has(src)) {
    loadScript.cache.set(src, new Promise((resolve, reject) => {
      const s = document.createElement('script');
      s.src = src;
      s.onload = resolve;
      s.onerror = () => reject(new Error('Failed to load ' + src));
      document.head.append(s);
    }));
  }
  return loadScript.cache.get(src);
}

/** KaTeX is only fetched when a document actually contains math. */
const loadKatex = () => loadScript('vendor/katex/katex.min.js').then(() => loadScript('vendor/katex/mhchem.min.js'));

// ------------------------------------------------------------- toasts
function toast(msg, { error = false, ms = 2600 } = {}) {
  const el = h('div', { class: 'toast' + (error ? ' error' : ''), text: msg });
  $('#toasts').append(el);
  setTimeout(() => {
    el.style.transition = 'opacity .25s';
    el.style.opacity = '0';
    setTimeout(() => el.remove(), 260);
  }, error ? Math.max(ms, 4500) : ms);
}
const fail = (e) => toast(String(e && e.message || e), { error: true });

// ----------------------------------------------------------- tooltips
(() => {
  let tip, timer;
  const hide = () => { clearTimeout(timer); tip?.remove(); tip = null; };
  document.addEventListener('mouseover', (e) => {
    const target = e.target.closest?.('[data-tip]');
    hide();
    if (!target) return;
    timer = setTimeout(() => {
      if (!target.isConnected) return;
      tip = h('div', { class: 'tooltip', text: target.dataset.tip });
      document.body.append(tip);
      const r = target.getBoundingClientRect(), t = tip.getBoundingClientRect();
      let top = r.bottom + 7;
      if (top + t.height > innerHeight - 4) top = r.top - t.height - 7;
      tip.style.top = top + 'px';
      tip.style.left = clamp(r.left + r.width / 2 - t.width / 2, 6, innerWidth - t.width - 6) + 'px';
    }, 450);
  });
  document.addEventListener('mousedown', hide, true);
  document.addEventListener('keydown', hide, true);
  window.addEventListener('blur', hide);
})();

// ----------------------------------------------------------- popovers
const popover = (() => {
  let current = null;
  function close() {
    if (!current) return;
    current.el.remove();
    current.anchor?.classList.remove('on');
    current.onClose?.();
    current = null;
  }
  function open(anchor, content, { align = 'left', cls = '', onClose, at } = {}) {
    const toggling = current && current.anchor === anchor && anchor;
    close();
    if (toggling) return null;
    const el = h('div', { class: 'popover ' + cls }, content);
    $('#overlays').append(el);
    const r = at ? { left: at.x, right: at.x, top: at.y, bottom: at.y } : anchor.getBoundingClientRect();
    const b = el.getBoundingClientRect();
    let left = align === 'right' ? r.right - b.width : r.left;
    let top = r.bottom + 6;
    if (top + b.height > innerHeight - 8) top = Math.max(8, (at ? r.top : r.top - 6) - b.height);
    el.style.left = clamp(left, 8, Math.max(8, innerWidth - b.width - 8)) + 'px';
    el.style.top = top + 'px';
    anchor?.classList.add('on');
    current = { el, anchor, onClose };
    return el;
  }
  document.addEventListener('mousedown', (e) => {
    if (current && !current.el.contains(e.target) && !(current.anchor && current.anchor.contains(e.target))) close();
  }, true);
  window.addEventListener('blur', close);
  window.addEventListener('resize', close);
  return { open, close, get isOpen() { return !!current; } };
})();

/** items: [{label, icon, kbd, run, checked, disabled} | '-' | {title}] */
function showMenu(anchor, items, opts = {}) {
  const nodes = items.filter(Boolean).map((it) => {
    if (it === '-') return h('div', { class: 'menu-sep' });
    if (it.title) return h('div', { class: 'menu-title', text: it.title });
    return h('button', {
      class: 'menu-item' + (it.checked ? ' checked' : ''),
      disabled: it.disabled,
      onclick: () => { popover.close(); it.run?.(); },
    },
      h('span', { html: icon(it.icon || '') , style: it.icon ? '' : 'visibility:hidden' }),
      h('span', { class: 'label', text: it.label }),
      it.kbd ? h('kbd', { text: it.kbd }) : null);
  });
  return popover.open(anchor, nodes, opts);
}

// ------------------------------------------------------------ dialogs
/** buttons: [{label, value, primary, danger}] -> resolves with value (null on Esc). */
function dialog({ title, message, buttons }) {
  return new Promise((resolve) => {
    const done = (v) => { scrim.remove(); document.removeEventListener('keydown', onKey, true); resolve(v); };
    const onKey = (e) => {
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); done(null); }
      else if (e.key === 'Enter') { e.preventDefault(); e.stopPropagation(); done(buttons.find((b) => b.primary)?.value ?? null); }
    };
    const scrim = h('div', { class: 'scrim', onmousedown: (e) => { if (e.target === scrim) done(null); } },
      h('div', { class: 'dialog' },
        h('h3', { text: title }),
        h('p', { text: message }),
        h('div', { class: 'actions' }, buttons.map((b) =>
          h('button', { class: 'btn' + (b.primary ? ' primary' : '') + (b.danger ? ' danger' : ''), text: b.label, onclick: () => done(b.value) })))));
    $('#overlays').append(scrim);
    document.addEventListener('keydown', onKey, true);
    scrim.querySelector('.btn.primary')?.focus();
  });
}
