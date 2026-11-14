'use strict';
/* App shell: documents & tabs, view modes, sidebar, commands, menus, tools, export, find bar. */

(() => {
  Object.assign(ICONS, {
    underline: '<path d="M6 4v6a6 6 0 0 0 12 0V4M4 20h16"/>',
    indent: '<path d="M21 6H11M21 12H11M21 18H11M3 8l4 4-4 4"/>',
    outdent: '<path d="M21 6H11M21 12H11M21 18H11M7 8l-4 4 4 4"/>',
    eraser: '<path d="m7 21-4.300-4.300a1 1 0 0 1 0-1.400l9.600-9.600a2 2 0 0 1 2.800 0l5.600 5.600a2 2 0 0 1 0 2.800L13 21zM22 21H7M5 11l9 9"/>',
    wrench: '<path d="M14.700 6.300a1 1 0 0 0 0 1.400l1.600 1.600a1 1 0 0 0 1.400 0l3.770-3.770a6 6 0 0 1-7.940 7.940l-6.910 6.910a2.120 2.120 0 0 1-3-3l6.910-6.910a6 6 0 0 1 7.940-7.940z"/>',
    doc: '<path d="M14.500 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.500zM14 2v6h6M8 13h8M8 17h6"/>',
    cloud: '<path d="M17.500 19H9a7 7 0 1 1 6.710-9h1.790a4.500 4.500 0 1 1 0 9z"/>',
    clipboard: '<rect x="8" y="2" width="8" height="4" rx="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/>',
    smile: '<circle cx="12" cy="12" r="10"/><path d="M8 14s1.500 2 4 2 4-2 4-2M9 9h.01M15 9h.01"/>',
    listTree: '<path d="M21 12h-8M21 6H8M21 18h-8M3 6v4c0 1.100.9 2 2 2h3M3 10v6c0 1.100.9 2 2 2h3"/>',
    sub: '<path d="m4 5 8 8M12 5l-8 8M20 19h-4c0-1.500.44-2 1.500-2.500S20 15.330 20 14c0-.47-.17-.93-.48-1.290a2.110 2.110 0 0 0-2.620-.44c-.42.24-.74.62-.9 1.070"/>',
    sup: '<path d="m4 19 8-8M12 19l-8-8M20 12h-4c0-1.500.44-2 1.500-2.500S20 8.330 20 7c0-.47-.17-.93-.48-1.290a2.110 2.110 0 0 0-2.620-.44c-.42.24-.74.62-.9 1.070"/>',
  });

  const cm = Editor.cm;
  const S = {
    docs: [],
    active: null,
    untitled: 0,
    info: { version: '', installed: false, isDefault: true },
    filesDir: '',
    settings: {
      theme: 'auto', mode: 'read', sidebar: false, sidebarPanel: 'outline', sidebarWidth: 260, split: 0.5,
      fontSize: 16, editorSize: 14.5, zoom: 1, width: 'medium', serif: false,
      lineNumbers: false, wrap: true, syncScroll: true, autosave: false, recent: [],
    },
  };
  const set = S.settings;
  let idSeq = 0;
  let pendingPreviewLine = null;
  const lock = { editor: 0, preview: 0 };

  const persist = debounce(() => host.send('save_settings', { settings: set }), 400);
  const mode = () => document.body.dataset.mode;
  const writeMode = () => !!S.active && mode() === 'write';
  const editorVisible = () => !!S.active && (mode() === 'split' || mode() === 'edit');
  const previewVisible = () => !!S.active && (mode() === 'read' || mode() === 'split');
  const canEdit = () => writeMode() || editorVisible();
  const flushRich = () => { if (Rich.ready) Rich.flush(); };
  const isMd = () => !!S.active && S.active.kind.id === 'markdown';
  const mdOnly = () => (isMd() ? true : (toast('This tool works on Markdown documents'), false));
  /** The view a document can actually show when the user prefers `want`. */
  const modeFor = (doc, want) => {
    const ms = doc.kind.modes;
    return ms.includes(want) ? want : want === 'write' && ms.includes('split') ? 'split' : want !== 'read' && ms.includes('edit') ? 'edit' : ms[0];
  };
  const wantGutter = () => !!set.lineNumbers || (!!S.active && !isMd());
  function paintMode(m) {
    document.body.dataset.mode = m;
    for (const b of $$('#mode-switch button')) b.classList.toggle('active', b.dataset.mode === m || (m === 'edit' && b.dataset.mode === 'split'));
  }

  // =============================================================== theme
  const media = matchMedia('(prefers-color-scheme: dark)');
  const THEMES = {
    light: { name: 'Light', base: 'light', sw: ['#ffffff', '#f4f5f8', '#5b5fe9', '#1f2328'] },
    sepia: { name: 'Sepia', base: 'light', sw: ['#fbf5e9', '#f3ead8', '#b5622a', '#3b3024'] },
    solarized: { name: 'Solarized Light', base: 'light', sw: ['#fdf6e3', '#eee8d5', '#268bd2', '#47585e'] },
    mint: { name: 'Mint', base: 'light', sw: ['#f6fbf8', '#e8f3ed', '#1f8a5b', '#1c2b24'] },
    dark: { name: 'Dark', base: 'dark', sw: ['#16181d', '#1b1e24', '#9698fb', '#e3e6eb'] },
    nord: { name: 'Nord', base: 'dark', sw: ['#2e3440', '#292e39', '#88c0d0', '#eceff4'] },
    dracula: { name: 'Dracula', base: 'dark', sw: ['#282a36', '#21222c', '#bd93f9', '#f8f8f2'] },
    'solarized-dark': { name: 'Solarized Dark', base: 'dark', sw: ['#002b36', '#00252f', '#2aa198', '#d6dccf'] },
    black: { name: 'Black', base: 'dark', sw: ['#000000', '#0a0a0a', '#4cc2ff', '#ffffff'] },
  };
  const themeId = () => (THEMES[set.theme] ? set.theme : media.matches ? 'dark' : 'light');
  function applyTheme() {
    const id = themeId();
    const dark = THEMES[id].base === 'dark';
    const changed = isDark() !== dark;
    document.documentElement.dataset.palette = id;
    document.documentElement.dataset.theme = dark ? 'dark' : 'light';
    $('#hljs-light').disabled = dark;
    $('#hljs-dark').disabled = !dark;
    $('#btn-theme').innerHTML = icon(dark ? 'moon' : 'sun');
    host.send('set_theme', { dark });
    if (changed && writeMode()) { flushRich(); Rich.load(S.active); }
    return changed ? Preview.retheme() : Promise.resolve();
  }
  media.addEventListener('change', () => { if (!THEMES[set.theme]) applyTheme(); });
  function themePicker(anchor = $('#btn-theme')) {
    const pick = (id) => { set.theme = id; applyTheme(); persist(); paint(); };
    const cards = h('div', { class: 'theme-grid' });
    const paint = () => cards.replaceChildren(
      h('button', { class: 'theme-card auto' + (THEMES[set.theme] ? '' : ' active'), onclick: () => pick('auto') },
        h('i', { style: 'background:linear-gradient(135deg,#ffffff 50%,#16181d 50%)' }), h('span', { text: 'Match Windows' })),
      ...Object.entries(THEMES).map(([id, t]) => h('button', { class: 'theme-card' + (set.theme === id ? ' active' : ''), onclick: () => pick(id) },
        h('i', { style: `background:${t.sw[0]};border-color:${t.sw[1]}` }, h('u', { style: `background:${t.sw[1]}` }), h('em', { style: `background:${t.sw[2]}` }), h('s', { style: `background:${t.sw[3]}` })),
        h('span', { text: t.name }))));
    paint();
    popover.open(anchor, [h('div', { class: 'menu-title', style: 'padding-left:2px', text: 'Theme' }), cards], { align: 'right', cls: 'themes' });
  }
  $('#btn-theme').addEventListener('click', () => themePicker());

  function applyAppearance() {
    const st = document.documentElement.style;
    st.setProperty('--doc-size', (set.fontSize * set.zoom).toFixed(2) + 'px');
    st.setProperty('--editor-size', (set.editorSize * set.zoom).toFixed(2) + 'px');
    st.setProperty('--doc-width', { narrow: '42em', medium: '51em', wide: '66em', full: 'none' }[set.width] || '51em');
    Preview.root.classList.toggle('serif', !!set.serif);
    document.body.classList.toggle('serif-doc', !!set.serif);
    if (cm.getOption('lineNumbers') !== wantGutter()) cm.setOption('lineNumbers', wantGutter());
    document.body.classList.toggle('gutter', wantGutter());
    paintAutosave();
    if (cm.getOption('lineWrapping') !== !!set.wrap) cm.setOption('lineWrapping', !!set.wrap);
    $('#st-zoom').textContent = Math.round(set.zoom * 100) + '%';
    cm.refresh();
  }
  function zoom(f) {
    set.zoom = f === 0 ? 1 : clamp(Math.round(set.zoom * f * 20) / 20, 0.6, 2.4);
    applyAppearance();
    persist();
  }

  // =========================================================== documents
  function makeDoc(d = {}) {
    const text = d.text || '';
    const name = d.name || `Untitled-${++S.untitled}.md`;
    const kind = Kinds.of(name, text);
    const doc = {
      id: ++idSeq,
      path: d.path || '',
      name,
      kind,
      dir: d.dir || '',
      crlf: !!d.crlf,
      readonly: !!d.readonly,
      cm: Editor.newDoc(text, kind),
      saved: text,
      gen: 0,
      richGen: -1,
      dirty: false,
      conflict: null,
      view: { line: 1 },
    };
    doc.clean = doc.cm.changeGeneration(true);
    return doc;
  }
  const findDoc = (path) => S.docs.find((d) => d.path && pathx.same(d.path, path));

  function addRecent(path) {
    set.recent = [path, ...set.recent.filter((p) => !pathx.same(p, path))].slice(0, 14);
    persist();
  }
  const updateWatch = () => host.send('watch', { paths: S.docs.filter((d) => d.path).map((d) => d.path) });

  function openDocs(payloads, { hash } = {}) {
    let last = null;
    for (const p of payloads) {
      if (p.error) { toast(p.error, { error: true }); continue; }
      let doc = findDoc(p.path);
      if (!doc) {
        doc = makeDoc(p);
        S.docs.push(doc);
        addRecent(p.path);
      }
      last = doc;
    }
    if (last) {
      updateWatch();
      activate(last, hash);
    }
    return last;
  }
  async function openPath(path, hash) {
    const existing = findDoc(path);
    if (existing) return activate(existing, hash);
    try {
      openDocs([await host.invoke('read', { path })], { hash });
    } catch (e) {
      fail(e);
      set.recent = set.recent.filter((p) => !pathx.same(p, path));
      persist();
      if (!S.active) renderStart();
    }
  }
  async function openDialog() {
    const docs = await host.invoke('open_dialog', { dir: S.active?.dir || '' });
    openDocs(docs);
  }
  function newDoc(text = '', name) {
    const doc = makeDoc({
      text: typeof text === 'string' ? text : '',
      name: name ? name.replace(/^Untitled(?=\.)/, () => `Untitled-${++S.untitled}`) : undefined,
    });
    S.docs.push(doc);
    if (set.mode === 'read') { set.mode = 'write'; persist(); }
    activate(doc);
    if (doc.kind.id === 'svg' && !doc.cm.getValue().trim()) setTimeout(() => designFile().catch(fail), 60);
  }
  async function openWelcome() {
    const existing = S.docs.find((d) => d.builtin);
    if (existing) return activate(existing);
    const text = await fetch('welcome.md').then((r) => r.text());
    const doc = makeDoc({ name: 'Welcome to Inkdown.md', text });
    doc.builtin = true;
    S.docs.push(doc);
    activate(doc);
  }

  // ---- position bookkeeping across views
  function richLine() {
    const p = Rich.pane, doc = S.active;
    const span = p.scrollHeight - p.clientHeight;
    return 1 + (span > 0 ? p.scrollTop / span : 0) * (doc ? doc.cm.lineCount() : 1);
  }
  function setRichLine(line) {
    requestAnimationFrame(() => {
      const p = Rich.pane, doc = S.active;
      if (!doc) return;
      p.scrollTop = ((line - 1) / Math.max(1, doc.cm.lineCount())) * (p.scrollHeight - p.clientHeight);
    });
  }
  const currentLine = (m = mode()) => (m === 'read' ? Preview.lineAtTop() : m === 'write' ? richLine() : Editor.topLine());
  function rememberView() {
    if (S.active) S.active.view.line = currentLine();
  }

  async function showRich(doc, line, focus) {
    if (!Rich.isSynced(doc)) await Rich.load(doc);
    if (S.active !== doc || mode() !== 'write') return;
    if (line != null) setRichLine(line);
    renderOutline();
    updateToolbar();
    if (focus) Rich.focus();
  }

  function activate(doc, hash) {
    if (S.active !== doc) {
      flushRich();
      rememberView();
      Editor.setErrorLine(null);
      S.active = doc;
      cm.swapDoc(doc.cm);
      cm.setOption('readOnly', false);
      Preview.clear();
      pendingPreviewLine = doc.view.line;
      if (doc.dir) S.filesDir = doc.dir;
    }
    applyKindUI(doc);
    Editor.applyKind(doc.kind, doc.cm);
    paintMode(modeFor(doc, set.mode));
    document.body.dataset.empty = '0';
    $('#start').hidden = true;
    renderTabs();
    renderBanner();
    updateTitle();
    updateStatus();
    refreshFiles();
    if (mode() === 'write') {
      showRich(doc, doc.view.line, true).catch(fail);
    } else {
      if (previewVisible() || isMd()) renderNow().then(() => { if (hash) scrollToAnchor(hash); });
      requestAnimationFrame(() => {
        cm.refresh();
        if (editorVisible()) { Editor.scrollToLine(doc.view.line); cm.focus(); } else Preview.pane.focus();
      });
    }
    updateToolbar();
  }

  async function closeDoc(doc = S.active) {
    if (!doc) return true;
    flushRich();
    if (doc.dirty) {
      activate(doc);
      const r = await dialog({
        title: 'Save changes?',
        message: `"${doc.name}" has unsaved changes.`,
        buttons: [{ label: 'Cancel', value: null }, { label: "Don't save", value: 'discard', danger: true }, { label: 'Save', value: 'save', primary: true }],
      });
      if (!r) return false;
      if (r === 'save' && !(await save(doc))) return false;
    }
    const i = S.docs.indexOf(doc);
    S.docs.splice(i, 1);
    updateWatch();
    if (S.active === doc) {
      S.active = null;
      Rich.detach();
      const next = S.docs[Math.min(i, S.docs.length - 1)];
      if (next) activate(next);
      else showEmpty();
    } else renderTabs();
    return true;
  }
  function showEmpty() {
    S.active = null;
    Rich.detach();
    cm.swapDoc(Editor.newDoc(''));
    Preview.clear();
    document.body.dataset.empty = '1';
    applyKindUI(null);
    $('#start').hidden = false;
    closeFind();
    renderTabs(); renderBanner(); renderOutline(); refreshFiles(); updateTitle(); updateStatus(); renderStart();
  }
  function cycleTab(dir) {
    if (S.docs.length < 2) return;
    const i = S.docs.indexOf(S.active);
    activate(S.docs[(i + dir + S.docs.length) % S.docs.length]);
  }

  // ---- saving
  async function save(doc = S.active, { as = false } = {}) {
    if (!doc) return false;
    flushRich();
    let path = doc.path;
    try {
      if (!path || as || doc.readonly) {
        path = await host.invoke('save_dialog', { name: doc.name, dir: doc.dir || S.filesDir, kind: doc.kind.id === 'markdown' ? 'md' : 'file', ext: pathx.ext(doc.name) });
        if (!path) return false;
      }
      const text = doc.cm.getValue();
      await host.invoke('save', { path, text, crlf: doc.crlf });
      doc.saved = text;
      doc.clean = doc.cm.changeGeneration(true);
      doc.dirty = false;
      doc.conflict = null;
      doc.savedAt = new Date();
      if (path !== doc.path) {
        doc.path = path; doc.name = pathx.basename(path); doc.dir = pathx.dirname(path);
        doc.readonly = false; doc.builtin = false;
        S.filesDir = doc.dir;
        addRecent(path);
        updateWatch();
        refreshFiles();
        doc.kind = Kinds.of(doc.name, text);
        if (doc === S.active) activate(doc);
      }
      renderTabs(); renderBanner(); updateTitle(); updateStatus();
      return true;
    } catch (e) { fail(e); return false; }
  }
  // ---- AutoSave: every changed document that has a file, shortly after the last edit
  let autosaving = false;
  async function autosaveNow() {
    autosave.cancel();
    if (!set.autosave || autosaving) return;
    flushRich();
    autosaving = true;
    try {
      for (const d of [...S.docs]) if (d.path && d.dirty && !d.conflict && !d.readonly) await save(d);
    } finally { autosaving = false; }
  }
  const autosave = debounce(autosaveNow, 900);
  function paintAutosave() {
    const b = $('#btn-autosave');
    b.classList.toggle('on', !!set.autosave);
    b.querySelector('b').textContent = set.autosave ? 'On' : 'Off';
  }
  async function toggleAutosave() {
    set.autosave = !set.autosave;
    paintAutosave();
    persist();
    if (!set.autosave) return;
    const doc = S.active;
    if (doc && !doc.path) {
      toast('Choose where to keep this document. AutoSave takes over from there.', { ms: 4500 });
      if (!(await save(doc))) return;
    }
    autosaveNow();
  }
  $('#btn-autosave').addEventListener('click', () => toggleAutosave().catch(fail));
  window.addEventListener('blur', () => autosaveNow());

  // ---- external changes
  const changeTimers = new Map();
  ink.onFileChanged = (path) => {
    clearTimeout(changeTimers.get(path));
    changeTimers.set(path, setTimeout(async () => {
      changeTimers.delete(path);
      const doc = findDoc(path);
      if (!doc) return;
      let fresh;
      try { fresh = await host.invoke('read', { path: doc.path }); } catch { return; }
      if (fresh.text === doc.saved) return;
      if (doc === S.active) flushRich();
      if (!doc.dirty) {
        loadFresh(doc, fresh);
        if (doc === S.active) toast('Reloaded: file changed on disk');
      } else {
        doc.conflict = fresh;
        renderBanner();
      }
    }, 220));
  };
  function loadFresh(doc, fresh) {
    const active = doc === S.active;
    const cursor = doc.cm.getCursor();
    const line = active ? currentLine() : doc.view.line;
    doc.cm.setValue(fresh.text);
    doc.cm.setCursor(cursor);
    doc.saved = fresh.text; doc.crlf = fresh.crlf;
    doc.clean = doc.cm.changeGeneration(true);
    doc.dirty = false; doc.conflict = null; doc.gen++;
    doc.view.line = line;
    if (active) {
      if (mode() === 'write') showRich(doc, line, false).catch(fail);
      else {
        if (editorVisible()) Editor.scrollToLine(line);
        pendingPreviewLine = line;
        renderNow();
      }
    }
    renderTabs(); renderBanner(); updateTitle(); updateStatus();
  }
  function renderBanner() {
    const el = $('#banner'), doc = S.active;
    if (!doc || !doc.conflict) { el.hidden = true; return; }
    el.hidden = false;
    el.replaceChildren(
      h('span', { text: `"${doc.name}" was changed by another program.` }),
      h('button', { class: 'btn sm', text: 'Reload from disk', onclick: () => loadFresh(doc, doc.conflict) }),
      h('button', { class: 'btn sm', text: 'Keep my version', onclick: () => { doc.saved = doc.conflict.text; doc.conflict = null; renderBanner(); } }));
  }

  /** Apply a whole-document Markdown transformation in any mode. */
  async function transformDoc(fn) {
    const doc = S.active;
    if (!doc) return false;
    flushRich();
    const before = doc.cm.getValue();
    const after = await fn(before, doc);
    if (after == null || after === before) return false;
    const cur = doc.cm.getCursor();
    doc.cm.replaceRange(after, { line: 0, ch: 0 }, { line: doc.cm.lastLine(), ch: doc.cm.getLine(doc.cm.lastLine()).length }, '+input');
    doc.cm.setCursor(cur);
    if (mode() === 'write') await showRich(doc, null, false);
    return true;
  }

  // ============================================================== render
  async function renderNow(force = false) {
    scheduleRender.cancel();
    const doc = S.active;
    if (!doc || (mode() === 'write' && !force)) return;
    const t0 = performance.now();
    await Preview.render(doc, doc.cm.getValue());
    renderCost = performance.now() - t0;
  }
  // Re-render delay adapts to how expensive the document is, so typing never stutters.
  let renderCost = 0, renderTimer = 0;
  const scheduleRender = () => { clearTimeout(renderTimer); renderTimer = setTimeout(() => renderNow(), clamp(renderCost * 0.5, 110, 450)); };
  scheduleRender.cancel = () => clearTimeout(renderTimer);
  const ensureRendered = async () => { flushRich(); if (S.active && !Preview.isCurrent(S.active)) await renderNow(true); };

  Preview.onUpdate = () => {
    if (mode() !== 'write') renderOutline();
    if (pendingPreviewLine != null) {
      lock.preview = performance.now() + 150;
      Preview.scrollToLine(pendingPreviewLine);
      pendingPreviewLine = null;
    } else if (mode() === 'split' && set.syncScroll && cm.hasFocus()) {
      syncPreviewToEditor();
    }
    if (!$('#findbar').hidden && findTarget() === Preview) runFind(false);
    updateSpy();
  };

  function noteChange(doc) {
    doc.gen++;
    const dirty = !doc.cm.isClean(doc.clean);
    if (dirty !== doc.dirty) { doc.dirty = dirty; renderTabs(); updateTitle(); updateStatus(); }
  }
  cm.on('change', () => {
    const doc = S.active;
    if (!doc) return;
    noteChange(doc);
    if (mode() !== 'write' && (previewVisible() || doc.kind.id === 'markdown')) scheduleRender();
    updateCounts();
    autosave();
    if (!$('#findbar').hidden && findTarget() === Editor) refreshFind();
  });
  cm.on('cursorActivity', () => { updatePos(); if (mode() === 'edit') updateSpy(); });

  Preview.onProduced = (doc, res) => {
    if (doc !== S.active) return;
    const st = res.status || null;
    Editor.setErrorLine(st && !st.ok ? st.line : null);
    const el = $('#data-status');
    if (!el) return;
    el.textContent = st ? (st.ok ? '✓ ' : '✗ ') + st.message : '';
    el.className = 'data-status ' + (st ? (st.ok ? 'ok' : 'bad') : '');
    el.onclick = st && st.line ? () => { if (mode() === 'read') setMode('split'); Editor.revealLine(st.line); } : null;
  };

  // Late layout changes (a diagram finishing) must not leave the two panes out of step.
  Preview.onLayout = debounce(() => { if (S.active && mode() === 'split' && set.syncScroll) syncPreviewToEditor(); }, 140);

  function gotoLine() {
    if (!editorVisible()) return;
    const input = h('input', { type: 'text', placeholder: `Line number (1 to ${cm.lineCount()})`, class: 'link-input', spellcheck: 'false' });
    const go = () => { const n = parseInt(input.value, 10); popover.close(); if (n) Editor.revealLine(clamp(n, 1, cm.lineCount())); };
    input.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); go(); } e.stopPropagation(); });
    popover.open($('#st-pos'), [h('div', { class: 'menu-title', style: 'padding-left:0', text: 'Go to line' }), input], { cls: 'src-popover', align: 'right' });
    input.focus();
  }

  // ---- rich editor hooks
  const outlineSoon = debounce(() => { if (writeMode()) renderOutline(); }, 300);
  Rich.onSync = (doc, md) => {
    if (md !== doc.cm.getValue()) {
      doc.cm.setValue(md);
      if (doc !== S.active) noteChange(doc);
    }
    doc.richGen = doc.gen;
    outlineSoon();
  };
  let toolbarRaf = 0;
  Rich.onSelection = () => { cancelAnimationFrame(toolbarRaf); toolbarRaf = requestAnimationFrame(updateToolbar); };
  Rich.onEdit = () => {
    const doc = S.active;
    if (doc && !doc.dirty) { doc.dirty = true; renderTabs(); updateTitle(); updateStatus(); }
    autosave();
  };
  Rich.pane.addEventListener('scroll', () => requestAnimationFrame(updateSpy), { passive: true });

  // ========================================================= scroll sync
  function syncPreviewToEditor() {
    lock.preview = performance.now() + 120;
    const info = cm.getScrollInfo();
    const p = Preview.pane;
    if (info.top <= 1) p.scrollTop = 0;
    else if (Preview.lineMap().length) Preview.scrollToLine(Editor.topLine());
    else p.scrollTop = (info.top / Math.max(1, info.height - info.clientHeight)) * (p.scrollHeight - p.clientHeight);
  }
  let rafE = 0, rafP = 0;
  cm.on('scroll', () => {
    if (mode() !== 'split' || !set.syncScroll || performance.now() < lock.editor) return;
    cancelAnimationFrame(rafE);
    rafE = requestAnimationFrame(syncPreviewToEditor);
  });
  Preview.pane.addEventListener('scroll', () => {
    cancelAnimationFrame(rafP);
    rafP = requestAnimationFrame(() => {
      updateSpy();
      if (mode() !== 'split' || !set.syncScroll || performance.now() < lock.preview) return;
      lock.editor = performance.now() + 120;
      const p = Preview.pane, info = cm.getScrollInfo();
      if (p.scrollTop <= 1) cm.scrollTo(null, 0);
      else if (Preview.lineMap().length) Editor.scrollToLine(Preview.lineAtTop());
      else cm.scrollTo(null, (p.scrollTop / Math.max(1, p.scrollHeight - p.clientHeight)) * (info.height - info.clientHeight));
    });
  }, { passive: true });

  // ================================================================ modes
  function setMode(m, { focus = true } = {}) {
    const prev = mode();
    const doc = S.active;
    if (set.mode !== m) { set.mode = m; persist(); }
    if (doc) m = modeFor(doc, m);
    const line = doc && prev && prev !== m ? currentLine(prev) : null;
    if (prev === 'write' && m !== 'write') flushRich();
    paintMode(m);
    if (!doc) return;
    if (m === 'write') {
      showRich(doc, line, focus).catch(fail);
    } else {
      const wasSource = prev === 'split' || prev === 'edit';
      if (m !== 'read') {
        cm.refresh();
        if (line != null && !wasSource) { lock.editor = performance.now() + 150; Editor.scrollToLine(line); }
        if (focus) cm.focus();
      } else if (focus) Preview.pane.focus();
      if (m !== 'edit') {
        if (!Preview.isCurrent(doc)) { pendingPreviewLine = line; renderNow(true); }
        else if (line != null && (prev === 'edit' || prev === 'write')) requestAnimationFrame(() => { lock.preview = performance.now() + 150; Preview.scrollToLine(line); });
      } else if (!Preview.isCurrent(doc)) scheduleRender();
      renderOutline();
    }
    if (!$('#findbar').hidden) runFind(false);
    updatePos();
    updateToolbar();
  }
  const cycleMode = () => setMode({ read: 'write', write: 'split', split: 'read', edit: 'read' }[mode()] || 'read');

  // ================================================================= tabs
  function renderTabs() {
    const tabs = $('#tabs');
    tabs.replaceChildren(...S.docs.map((doc) => h('div', {
      class: 'tab' + (doc === S.active ? ' active' : '') + (doc.dirty ? ' dirty' : ''),
      'data-tip': doc.path || doc.name,
      onmousedown: (e) => { if (e.button === 1) { e.preventDefault(); closeDoc(doc); } },
      onclick: () => activate(doc),
    },
      h('span', { class: 'tab-name', text: doc.name }),
      h('button', { class: 'tab-close', html: icon('x'), onclick: (e) => { e.stopPropagation(); closeDoc(doc); } }))));
    tabs.querySelector('.tab.active')?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }
  $('#tabs').addEventListener('wheel', (e) => { $('#tabs').scrollLeft += e.deltaY; }, { passive: true });

  function updateTitle() {
    const doc = S.active;
    host.send('set_title', { title: doc ? `${doc.dirty ? '● ' : ''}${doc.name} - Inkdown` : 'Inkdown' });
  }

  // ============================================================ status bar
  function updatePos() {
    const el = $('#st-pos');
    if (!editorVisible()) { el.textContent = ''; return; }
    const c = cm.getCursor();
    const sel = cm.getSelection().length;
    el.textContent = `Ln ${c.line + 1}, Col ${c.ch + 1}` + (sel ? `  (${sel} selected)` : '');
  }
  const countWords = (text) => (text.match(/[\p{L}\p{N}][\p{L}\p{N}'’-]*/gu) || []).length;
  const updateCounts = debounce(() => {
    const doc = S.active;
    if (!doc) { $('#st-words').textContent = ''; $('#st-read').textContent = ''; return; }
    const text = doc.cm.getValue();
    const words = countWords(text);
    $('#st-words').textContent = `${words.toLocaleString()} words · ${text.length.toLocaleString()} chars`;
    $('#st-read').textContent = words ? `${Math.max(1, Math.round(words / 230))} min read` : '';
  }, 250);
  function updateStatus() {
    const doc = S.active;
    $('#st-path').textContent = doc ? (doc.path ? '\u200e' + doc.path : doc.name) : '';
    $('#st-path').disabled = !doc?.path;
    const st = $('#st-state');
    st.replaceChildren();
    if (doc) {
      st.append(h('span', { class: 'dot' + (doc.dirty ? '' : ' ok') }),
        doc.readonly ? 'Read-only' : doc.dirty ? (set.autosave && doc.path ? 'Saving…' : 'Edited') : doc.path ? 'Saved' + (doc.savedAt ? ' ' + doc.savedAt.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : '') : 'Not saved yet');
    }
    $('#st-eol').textContent = doc ? (doc.crlf ? 'CRLF' : 'LF') : '';
    updatePos();
    updateCounts.flush();
  }
  $('#st-path').addEventListener('click', () => S.active?.path && host.send('reveal', { path: S.active.path }));
  $('#st-words').addEventListener('click', () => showStats().catch(fail));

  // =============================================================== sidebar
  function setSidebar(on) {
    set.sidebar = on;
    document.body.dataset.sidebar = on ? 'on' : 'off';
    $('#btn-sidebar').classList.toggle('on', on);
    $('#sidebar').style.width = set.sidebarWidth + 'px';
    persist();
    cm.refresh();
  }
  function setPanel(name) {
    set.sidebarPanel = name;
    for (const b of $$('.sidebar-tabs button')) b.classList.toggle('active', b.dataset.panel === name);
    $('#panel-outline').hidden = name !== 'outline';
    $('#panel-files').hidden = name !== 'files';
    if (name === 'files') refreshFiles();
    persist();
  }
  for (const b of $$('.sidebar-tabs button')) b.addEventListener('click', () => setPanel(b.dataset.panel));

  const viewPane = () => (mode() === 'write' ? Rich.pane : Preview.pane);
  const topIn = (el, pane) => el.getBoundingClientRect().top - pane.getBoundingClientRect().top + pane.scrollTop;
  let outlineItems = [];
  function renderOutline() {
    const box = $('#outline');
    const heads = !S.active ? [] : mode() === 'write' ? Rich.headings() : Preview.headings();
    if (!heads.length) {
      outlineItems = [];
      box.replaceChildren(h('div', { class: 'side-empty', text: S.active ? 'Headings in this document will appear here.' : 'No document open.' }));
      return;
    }
    const min = Math.min(...heads.map((x) => x.level));
    outlineItems = heads.map((hd) => {
      const btn = h('button', { class: 'outline-item', 'data-level': hd.level - min + 1, 'data-tip': hd.text.length > 34 ? hd.text : null },
        h('span', { text: hd.text || '(empty heading)' }));
      btn.addEventListener('click', () => goToHeading(hd));
      return { ...hd, btn };
    });
    box.replaceChildren(...outlineItems.map((o) => o.btn));
    updateSpy();
  }
  function goToHeading(hd) {
    if (mode() === 'edit') return Editor.revealLine(hd.line);
    if (!hd.el?.isConnected) return;
    lock.preview = 0;
    const pane = viewPane();
    pane.scrollTop = topIn(hd.el, pane) - 12;
    if (mode() === 'split') {
      lock.editor = performance.now() + 200;
      Editor.scrollToLine(hd.line);
    }
  }
  function updateSpy() {
    if (!outlineItems.length || !set.sidebar || set.sidebarPanel !== 'outline') return;
    let current = null;
    if (mode() === 'edit') {
      const line = cm.getCursor().line + 1;
      for (const o of outlineItems) if (o.line <= line) current = o;
    } else {
      const y = viewPane().getBoundingClientRect().top + 40;
      for (const o of outlineItems) { if (o.el?.isConnected && o.el.getBoundingClientRect().top <= y) current = o; else break; }
      current ??= outlineItems[0];
    }
    for (const o of outlineItems) o.btn.classList.toggle('current', o === current);
    current?.btn.scrollIntoView({ block: 'nearest' });
  }

  let filesShown = '';
  async function refreshFiles(force = false) {
    if (!set.sidebar || set.sidebarPanel !== 'files') return;
    const head = $('#files-head'), list = $('#files');
    if (!S.filesDir) {
      filesShown = '';
      head.replaceChildren();
      list.replaceChildren(h('div', { class: 'side-empty' }, 'Open a file or folder to browse its Markdown files.',
        h('div', { style: 'margin-top:12px' }, h('button', { class: 'btn sm', text: 'Open folder…', onclick: openFolder }))));
      return;
    }
    const mark = () => { for (const el of $$('.file-item', list)) el.classList.toggle('current', !!S.active?.path && pathx.same(el.dataset.path, S.active.path)); };
    if (!force && pathx.same(filesShown, S.filesDir)) return mark();
    let res;
    try { res = await host.invoke('list_dir', { path: S.filesDir }); } catch (e) { list.replaceChildren(h('div', { class: 'side-empty', text: String(e.message) })); return; }
    filesShown = S.filesDir;
    head.replaceChildren(
      h('button', { class: 'icon-btn sm', html: icon('up'), 'data-tip': 'Parent folder', disabled: !res.parent, onclick: () => { S.filesDir = res.parent; refreshFiles(); } }),
      h('b', { text: res.name, 'data-tip': res.path }),
      h('button', { class: 'icon-btn sm', html: icon('reset'), 'data-tip': 'Refresh', onclick: () => refreshFiles(true) }),
      h('button', { class: 'icon-btn sm', html: icon('folder'), 'data-tip': 'Open another folder…', onclick: openFolder }));
    list.replaceChildren(...res.entries.map((en) => h('button', {
      class: 'file-item' + (en.dir ? ' dir' : ''), dataset: { path: en.path }, 'data-tip': en.name.length > 30 ? en.name : null,
      onclick: () => { if (en.dir) { S.filesDir = en.path; refreshFiles(); } else openPath(en.path); },
    }, h('span', { html: icon(en.dir ? 'folder' : 'file'), style: 'display:contents' }), h('span', { text: en.name }))));
    if (!res.entries.length) list.append(h('div', { class: 'side-empty', text: 'No Markdown files in this folder.' }));
    mark();
  }
  async function openFolder() {
    const dir = await host.invoke('open_folder_dialog');
    if (!dir) return;
    S.filesDir = dir;
    setSidebar(true);
    setPanel('files');
    refreshFiles(true);
  }

  // ---- splitters
  function dragSplitter(el, onMove, onEnd) {
    el.addEventListener('mousedown', (e) => {
      e.preventDefault();
      el.classList.add('dragging');
      document.body.style.cursor = 'col-resize';
      const move = (ev) => onMove(ev.clientX);
      const up = () => {
        el.classList.remove('dragging');
        document.body.style.cursor = '';
        removeEventListener('mousemove', move);
        removeEventListener('mouseup', up);
        cm.refresh(); persist(); onEnd?.();
      };
      addEventListener('mousemove', move);
      addEventListener('mouseup', up);
    });
  }
  dragSplitter($('#split-sidebar'), (x) => {
    set.sidebarWidth = clamp(x, 170, Math.min(560, innerWidth - 300));
    $('#sidebar').style.width = set.sidebarWidth + 'px';
  });
  function applySplit() {
    document.body.style.setProperty('--split-a', set.split);
    document.body.style.setProperty('--split-b', 1 - set.split);
  }
  dragSplitter($('#split-main'), (x) => {
    const a = $('#editor-pane').getBoundingClientRect(), b = $('#preview-pane').getBoundingClientRect();
    set.split = clamp((x - a.left) / (b.right - a.left), 0.2, 0.8);
    applySplit();
  });
  $('#split-main').addEventListener('dblclick', () => { set.split = 0.5; applySplit(); cm.refresh(); persist(); });

  // ================================================================ links
  const slugify = (s) => s.toLowerCase().replace(/[^\p{L}\p{N}\s_-]/gu, '').trim().replace(/\s/g, '-');
  function scrollToAnchor(hash) {
    let id = hash.replace(/^#/, '');
    try { id = decodeURIComponent(id); } catch { /* raw */ }
    if (mode() === 'write') {
      const hd = Rich.headings().find((x) => slugify(x.text) === id.toLowerCase());
      if (hd) goToHeading(hd);
      return !!hd;
    }
    if (!id) { Preview.pane.scrollTop = 0; return true; }
    const t = Preview.root.querySelector(`[id="${CSS.escape(id)}"]`) || Preview.root.querySelector(`[id="${CSS.escape(id.toLowerCase())}"]`)
      || Array.from(Preview.root.querySelectorAll('a.anchor')).find((a) => a.getAttribute('href') === '#' + id);
    if (!t) return false;
    if (mode() === 'edit') setMode('split', { focus: false });
    const target = t.closest('h1,h2,h3,h4,h5,h6,li,p') || t;
    Preview.pane.scrollTop = Preview.topOf(target) - 14;
    return true;
  }
  ink.followLink = async (href, a) => {
    const doc = S.active;
    if (href.startsWith('#')) { if (!scrollToAnchor(href)) toast('No such heading in this document'); return; }
    if (/^(https?:|mailto:)/i.test(href)) return host.invoke('open_external', { target: href }).catch(fail);
    let target = href, hash = '';
    const hi = target.indexOf('#');
    if (hi >= 0) { hash = target.slice(hi); target = target.slice(0, hi); }
    try { target = decodeURIComponent(target); } catch { /* raw */ }
    if (/^file:\/\//i.test(target)) target = pathx.fromFileUrl(target);
    else if (/^[a-z][a-z0-9+.-]+:/i.test(target) && !pathx.isAbs(target)) return toast('Inkdown does not open this kind of link: ' + href.split(':')[0]);
    if (!pathx.isAbs(target)) {
      if (!doc?.dir) return toast('Save this document first to follow relative links');
      target = pathx.join(doc.dir, target);
    } else target = pathx.norm(target);
    if (a?.dataset.wikilink !== undefined && !pathx.ext(target)) target += '.md';
    if (MD_EXT.includes(pathx.ext(target)) || pathx.ext(target) === 'txt') return openPath(target, hash);
    if (!(await host.invoke('exists', { path: target }))) {
      if (!pathx.ext(target) && await host.invoke('exists', { path: target + '.md' })) return openPath(target + '.md', hash);
      return toast('File not found: ' + target, { error: true });
    }
    const ok = await dialog({
      title: 'Open with the default app?',
      message: target,
      buttons: [{ label: 'Cancel', value: false }, { label: 'Open', value: true, primary: true }],
    });
    if (ok) host.invoke('open_external', { target }).catch(fail);
  };
  // ---- visual diagram designer
  ink.editDrawing = async (src) => {
    if (!S.active) return;
    const svg = await Designer.open(src);
    if (svg == null) return;
    const changed = await transformDoc((text) => { const i = text.indexOf(src); return i < 0 ? null : text.slice(0, i) + svg + text.slice(i + src.length); });
    if (!changed) toast('The drawing has changed in the document; reopen it from the preview.', { error: true });
  };
  async function designFile() {
    const doc = S.active;
    if (!doc || doc.kind.id !== 'svg') return;
    const text = doc.cm.getValue();
    if (text.trim() && !Designer.isEditable(text)) return toast('This SVG was not made in the designer. Edit its code in the Code tab instead.', { ms: 5000 });
    const svg = await Designer.open(text);
    if (svg != null) await transformDoc(() => svg + '\n');
  }
  async function insertDrawing() {
    if (!canEdit() || !mdOnly()) return;
    const svg = await Designer.open('');
    if (svg == null) return;
    const block = '```svg\n' + svg + '\n```';
    if (writeMode()) await Rich.insertMarkdown(block); else Editor.insertBlock(block);
  }
  ink.activeDoc = () => S.active;
  ink.toggleTask = (line, checked) => {
    const doc = S.active;
    const text = doc.cm.getLine(line - 1);
    if (text === undefined) return;
    const m0 = /\[( |x|X)\]/.exec(text);
    if (!m0) return;
    doc.cm.replaceRange(checked ? '[x]' : '[ ]', { line: line - 1, ch: m0.index }, { line: line - 1, ch: m0.index + 3 }, '+input');
  };
  ink.jumpToSource = (line) => {
    if (mode() === 'read') setMode('split', { focus: false });
    Editor.revealLine(line);
  };

  // ================================================= images, paste, drop
  function linkPathFor(abs) {
    const doc = S.active;
    const rel = doc?.dir ? pathx.relative(doc.dir, abs) : null;
    return rel != null && !rel.startsWith('../../..') ? rel : pathx.toFileUrl(abs);
  }
  const urlFor = (abs) => encodeURI(linkPathFor(abs)).replace(/[()]/g, escape);
  function insertImage(src, alt) {
    if (writeMode()) Rich.exec('image', { src, alt });
    else if (editorVisible()) Editor.insertImage(src, alt);
  }
  async function pickImage() {
    if (!canEdit() || !mdOnly()) return;
    const p = await host.invoke('pick_image', { dir: S.active.dir });
    if (p) insertImage(urlFor(p), pathx.basename(p).replace(/\.[^.]+$/, ''));
  }
  ink.pasteImage = async (file) => {
    const doc = S.active;
    if (!doc?.dir) return toast('Save the document first: pasted images are stored next to it.');
    try {
      const data = await new Promise((res, rej) => {
        const r = new FileReader();
        r.onload = () => res(String(r.result).split(',')[1]);
        r.onerror = () => rej(r.error);
        r.readAsDataURL(file);
      });
      const ext = (file.type.split('/')[1] || 'png').replace('jpeg', 'jpg').replace('svg+xml', 'svg');
      const rel = await host.invoke('save_image', { dir: doc.dir, ext, data });
      insertImage(rel, 'image');
      toast('Image saved to ' + rel);
    } catch (e) { fail(e); }
  };
  ink.onDrop = (paths) => {
    const docs = [], images = [];
    for (const p of paths) (IMG_EXT.includes(pathx.ext(p)) ? images : docs).push(p);
    if (images.length && canEdit() && isMd()) {
      for (const p of images) {
        insertImage(urlFor(p), pathx.basename(p).replace(/\.[^.]+$/, ''));
        if (editorVisible()) cm.replaceSelection('\n');
      }
    } else if (images.length && !docs.length) toast('Switch to Write or Markdown to insert images');
    (async () => { for (const p of docs) await openPath(p); })();
  };

  // =============================================================== export
  const baseName = (doc) => doc.name.replace(/\.[^.]+$/, '');
  async function exported(path) {
    const r = await dialog({
      title: 'Export complete',
      message: path,
      buttons: [{ label: 'Close', value: null }, { label: 'Show in folder', value: 'reveal' }, { label: 'Open', value: 'open', primary: true }],
    });
    if (r === 'reveal') host.send('reveal', { path });
    if (r === 'open') host.invoke('open_external', { target: path }).catch(fail);
  }
  async function exportHtml() {
    const doc = S.active;
    if (!doc) return;
    try {
      await ensureRendered();
      const path = await host.invoke('save_dialog', { name: baseName(doc) + '.html', dir: doc.dir, kind: 'html' });
      if (!path) return;
      await host.invoke('save', { path, text: await Preview.exportHtml(baseName(doc)), crlf: false });
      exported(path);
    } catch (e) { fail(e); }
  }
  async function exportFile(format) {
    const doc = S.active;
    if (!doc) return null;
    const markdown = Kinds.asMarkdown(doc, doc.cm.getValue());
    if (markdown == null) { toast(`Word and RTF export are not available for ${doc.kind.label} yet. Use PDF, HTML or "Copy with formatting".`, { ms: 6000 }); return null; }
    try {
      await ensureRendered();
      const path = await host.invoke('save_dialog', { name: `${baseName(doc)}.${format}`, dir: doc.dir, kind: format });
      if (!path) return null;
      toast('Exporting…', { ms: 1500 });
      await Exporter.toFile(format, path, markdown, baseName(doc));
      return path;
    } catch (e) { fail(e); return null; }
  }
  async function copyRich(quiet = false) {
    const doc = S.active;
    if (!doc) return false;
    try {
      await ensureRendered();
      await Exporter.copyRich(doc.cm.getValue());
      if (!quiet) toast('Copied with formatting. Paste into Word, Outlook, Gmail or Google Docs.');
      return true;
    } catch (e) { fail(e); return false; }
  }
  // ---- integrations (accounts the user connects themselves)
  async function integrations() {
    let st = await host.invoke('google_status');
    const body = h('div');
    const scrim = h('div', { class: 'scrim', onmousedown: (e) => { if (e.target === scrim) scrim.remove(); } },
      h('div', { class: 'dialog wide' }, h('h3', { text: 'Integrations' }), body,
        h('div', { class: 'actions' }, h('button', { class: 'btn', text: 'Close', onclick: () => scrim.remove() }))));
    const paint = () => {
      const id = h('input', { type: 'text', class: 'link-input', placeholder: 'OAuth client ID  (ends with .apps.googleusercontent.com)', spellcheck: 'false' });
      id.value = st.clientId || '';
      const secret = h('input', { type: 'password', class: 'link-input', placeholder: st.hasSecret ? 'Client secret (saved)' : 'Client secret' });
      const note = h('p', { class: 'integ-note' });
      const connect = async () => {
        note.className = 'integ-note';
        note.textContent = 'Finish signing in in the browser window that just opened…';
        try { st = await host.invoke('google_connect', { clientId: id.value, clientSecret: secret.value }); toast('Google account connected'); paint(); }
        catch (e) { note.textContent = String(e.message || e); note.classList.add('bad'); }
      };
      body.replaceChildren(h('div', { class: 'integ-card' },
        h('div', { class: 'integ-head' }, h('span', { html: icon('cloud'), style: 'display:contents' }), h('b', { text: 'Google Drive and Docs' }),
          h('span', { class: 'integ-state' + (st.connected ? ' on' : ''), text: st.connected ? 'Connected' : 'Not connected' })),
        h('p', { text: 'Export sends the document to your own Google Drive as a native Google Doc. Inkdown can only reach files it creates itself, and the sign-in is stored encrypted on this PC.' }),
        st.connected
          ? h('div', { class: 'src-actions' }, h('button', { class: 'btn sm danger', text: 'Disconnect', onclick: async () => { st = await host.invoke('google_disconnect'); paint(); } }))
          : h('div', {},
            h('p', { class: 'integ-steps', text: 'One-time setup: in Google Cloud Console create a project, enable the Google Drive API, set up the OAuth consent screen, then create an OAuth client of type "Desktop app" and paste its ID and secret below.' }),
            id, h('div', { style: 'height:6px' }), secret, note,
            h('div', { class: 'src-actions' },
              h('button', { class: 'btn sm', text: 'Open Google Cloud Console', onclick: () => host.invoke('open_external', { target: 'https://console.cloud.google.com/apis/credentials' }).catch(fail) }),
              h('button', { class: 'btn sm primary', text: 'Connect Google account', onclick: connect })))));
    };
    paint();
    $('#overlays').append(scrim);
  }
  async function uploadToGoogle() {
    const doc = S.active;
    const markdown = Kinds.asMarkdown(doc, doc.cm.getValue());
    if (markdown == null) { toast(`Upload is not available for ${doc.kind.label} yet.`); return; }
    await ensureRendered();
    toast('Uploading to Google Drive…', { ms: 2500 });
    const assets = await Exporter.collect();
    const made = await host.invoke('google_upload', { text: markdown, assets, title: baseName(doc) });
    const open = await dialog({ title: 'Google Doc created', message: made.name + '\n' + made.webViewLink, buttons: [{ label: 'Close', value: false }, { label: 'Open in browser', value: true, primary: true }] });
    if (open) host.invoke('open_external', { target: made.webViewLink }).catch(fail);
  }

  async function exportGoogle() {
    if (!S.active) return;
    let account = { connected: false };
    try { account = await host.invoke('google_status'); } catch { /* offline build */ }
    if (account.connected) return uploadToGoogle().catch(fail);
    const r = await dialog({
      title: 'Export to Google Docs',
      message: 'Connect your Google account once (Integrations) and Inkdown uploads straight to your Drive as a Google Doc.\n\nWithout an account: Inkdown copies the formatted document and opens a blank Google Doc; press Ctrl+V there. Or save a Word file to upload yourself.',
      buttons: [{ label: 'Cancel', value: null }, { label: 'Save .docx', value: 'docx' }, { label: 'Copy and open Docs', value: 'paste' }, { label: 'Connect account…', value: 'connect', primary: true }],
    });
    if (r === 'connect') return integrations().catch(fail);
    if (r === 'paste') {
      if (!(await copyRich(true))) return;
      await host.invoke('open_external', { target: 'https://docs.google.com/document/create' }).catch(fail);
      toast('Document copied. Press Ctrl+V in the new Google Doc.', { ms: 7000 });
    } else if (r === 'docx') {
      const path = await exportFile('docx');
      if (!path) return;
      host.send('reveal', { path });
      await host.invoke('open_external', { target: 'https://drive.google.com/drive/my-drive' }).catch(fail);
      toast('Drag the highlighted .docx into Google Drive, then open it with Google Docs.', { ms: 8000 });
    }
  }
  async function printDoc() {
    if (!S.active) return;
    const prevMode = mode(), prevTheme = set.theme, dark = isDark();
    const hidden = prevMode === 'edit' || prevMode === 'write';
    try {
      if (hidden) setMode('read', { focus: false });
      await ensureRendered();
      if (dark) { set.theme = 'light'; await applyTheme(); }
      await new Promise((r) => requestAnimationFrame(() => setTimeout(r, 80)));
      window.print();
    } finally {
      if (dark) { set.theme = prevTheme; applyTheme(); }
      if (hidden) setMode(prevMode);
    }
  }
  function exportMenu(anchor = $('#btn-export'), opts = { align: 'right' }) {
    if (!S.active) return toast('Open a document first');
    showMenu(anchor, [
      { title: 'Save as file' },
      m('exportDocx'), m('exportRtf'), m('exportHtml'), m('print'),
      { title: 'Send elsewhere' },
      m('exportGoogle'), m('copyRich'), m('copyMarkdown'),
    ], opts);
  }
  $('#btn-export').addEventListener('click', () => exportMenu());

  // ================================================================ tools
  const selText = () => (writeMode() ? Rich.selectionText() : editorVisible() ? cm.getSelection() : '');
  function insertText(text) {
    if (writeMode()) Rich.replaceSelection(text);
    else if (editorVisible()) { cm.replaceSelection(text); cm.focus(); }
  }
  function changeCase(kind) {
    const text = selText();
    if (!text) return toast('Select some text first');
    const fn = {
      upper: (s) => s.toUpperCase(),
      lower: (s) => s.toLowerCase(),
      title: (s) => s.toLowerCase().replace(/(^|[\s(\[-])(\p{L})/gu, (m0, a, b) => a + b.toUpperCase()),
      sentence: (s) => s.toLowerCase().replace(/(^\s*|[.!?]\s+)(\p{L})/gu, (m0, a, b) => a + b.toUpperCase()),
    }[kind];
    if (writeMode()) Rich.replaceSelection(fn(text));
    else cm.replaceSelection(fn(text), 'around');
  }
  function sortLines(desc) {
    if (!editorVisible()) return toast('Sorting works on lines in Markdown view');
    if (!cm.somethingSelected()) return toast('Select the lines to sort');
    const a = cm.getCursor('from'), b = cm.getCursor('to');
    const last = b.ch === 0 && b.line > a.line ? b.line - 1 : b.line;
    const lines = [];
    for (let i = a.line; i <= last; i++) lines.push(cm.getLine(i));
    lines.sort((x, y) => x.localeCompare(y, undefined, { numeric: true, sensitivity: 'base' }));
    if (desc) lines.reverse();
    cm.replaceRange(lines.join('\n'), { line: a.line, ch: 0 }, { line: last, ch: cm.getLine(last).length }, '+input');
  }
  function textToTable() {
    if (!mdOnly()) return;
    const text = selText().trim();
    if (!text || !text.includes('\n')) return toast('Select rows of comma, tab or semicolon separated text first');
    const lines = text.split(/\r?\n/).filter((l) => l.trim());
    const sep = ['\t', ';', ',', '|'].find((s) => lines.every((l) => l.includes(s))) || /\s{2,}/;
    const rows = lines.map((l) => l.split(sep).map((c) => c.trim().replace(/^"(.*)"$/, '$1').replace(/\|/g, '\\|')));
    const cols = Math.max(...rows.map((r) => r.length));
    const row = (r) => '| ' + Array.from({ length: cols }, (_, i) => r[i] || '').join(' | ') + ' |';
    const md = [row(rows[0]), row(Array(cols).fill('---')), ...rows.slice(1).map(row)].join('\n');
    if (writeMode()) { Rich.replaceSelection(' '); Rich.insertMarkdown(md).catch(fail); }
    else { cm.replaceSelection(md); Editor.formatTable(); cm.focus(); }
  }
  async function updateToc() {
    if (!S.active || !mdOnly()) return;
    await ensureRendered();
    const heads = Preview.headings().map((x) => ({ level: x.level, text: x.text, id: x.el.querySelector('a.anchor')?.id || (x.el.querySelector('a.anchor')?.getAttribute('href') || '').slice(1) })).filter((x) => x.id);
    const body = heads.length > 1 && heads.filter((x) => x.level === 1).length === 1 ? heads.filter((x) => x.level > 1) : heads;
    if (!body.length) return toast('Add some headings first');
    const min = Math.min(...body.map((x) => x.level));
    const toc = '<!-- toc -->\n' + body.map((x) => `${'  '.repeat(x.level - min)}- [${x.text.replace(/[[\]]/g, '\\$&')}](#${x.id})`).join('\n') + '\n<!-- /toc -->';
    const changed = await transformDoc((md) => {
      if (/<!-- toc -->[\s\S]*?<!-- \/toc -->/.test(md)) return md.replace(/<!-- toc -->[\s\S]*?<!-- \/toc -->/, () => toc);
      const lines = md.split('\n');
      let at = 0;
      if (lines[0] === '---') { const end = lines.indexOf('---', 1); if (end > 0) at = end + 1; }
      const h1 = lines.findIndex((l, i) => i >= at && /^#\s/.test(l));
      if (h1 >= 0) at = h1 + 1;
      lines.splice(at, 0, '', toc, '');
      return lines.join('\n').replace(/\n{3,}/g, '\n\n');
    });
    toast(changed ? 'Table of contents updated' : 'Table of contents is already up to date');
  }
  async function tidyMarkdown() {
    const doc = S.active;
    if (!doc || !mdOnly()) return;
    const ok = await dialog({
      title: 'Tidy Markdown',
      message: 'Rewrites the whole document in one consistent style: ATX headings, "-" bullets, aligned tables, fenced code and normalised spacing. The content is unchanged, and you can undo it.',
      buttons: [{ label: 'Cancel', value: false }, { label: 'Tidy document', value: true, primary: true }],
    });
    if (!ok) return;
    const changed = await transformDoc(async () => { await Rich.load(doc); return Rich.toMarkdown(); });
    toast(changed ? 'Document tidied' : 'Already tidy');
  }
  async function showStats() {
    const doc = S.active;
    if (!doc) return;
    await ensureRendered();
    const text = Preview.root.textContent || '';
    const words = countWords(text);
    const q = (sel) => Preview.root.querySelectorAll(sel).length;
    const rows = [
      ['Words', words], ['Characters', text.length], ['Characters (no spaces)', text.replace(/\s/g, '').length],
      ['Sentences', (text.match(/[^.!?\n]+[.!?]+(\s|$)/g) || []).length], ['Paragraphs', q('p')], ['Headings', q('h1,h2,h3,h4,h5,h6')],
      ['Lines of Markdown', doc.cm.lineCount()], ['Links', q('a[href]:not(.anchor)')], ['Images', q('img')], ['Tables', q('table')],
      ['Code blocks', q('.codeblock')], ['Diagrams and charts', q('.diagram')], ['Formulas', q('[data-tex]')],
      ['Reading time', `${Math.max(1, Math.round(words / 230))} min`], ['Speaking time', `${Math.max(1, Math.round(words / 130))} min`],
    ];
    const scrim = h('div', { class: 'scrim', onmousedown: (e) => { if (e.target === scrim) scrim.remove(); } },
      h('div', { class: 'dialog' }, h('h3', { text: 'Document statistics' }),
        h('table', { class: 'stats' }, rows.map(([k, v]) => h('tr', {}, h('td', { text: k }), h('td', { text: typeof v === 'number' ? v.toLocaleString() : v })))),
        h('div', { class: 'actions' }, h('button', { class: 'btn primary', text: 'Close', onclick: () => scrim.remove() }))));
    $('#overlays').append(scrim);
    scrim.querySelector('button').focus();
    scrim.addEventListener('keydown', (e) => { if (e.key === 'Escape' || e.key === 'Enter') scrim.remove(); });
  }
  const SYMBOLS = {
    'Punctuation': '— – … • · § ¶ † ‡ « » “ ” ‘ ’ © ® ™ ° № ✓ ✗',
    'Arrows': '← → ↑ ↓ ↔ ⇐ ⇒ ⇔ ↩ ↪ ➜ ⟶',
    'Math': '± × ÷ ≠ ≈ ≤ ≥ ∞ √ ∑ ∏ ∫ ∂ ∆ ∈ ∉ ∩ ∪ ⊂ ⊆ ∀ ∃ π µ λ Ω α β γ θ',
    'Currency': '€ £ ¥ ₹ ¢ ₽ ₩ ₿',
    'Emoji': '😀 😊 😂 🤔 👍 👎 🙏 👏 🎉 🔥 ⭐ ❤️ ✅ ❌ ⚠️ 💡 📌 📎 📝 📅 📈 🚀 🐛 🔒',
  };
  function symbolPicker(anchor) {
    const body = Object.entries(SYMBOLS).flatMap(([title, chars]) => [
      h('div', { class: 'menu-title', style: 'padding-left:2px', text: title }),
      h('div', { class: 'symbol-grid' }, chars.split(' ').map((c) => h('button', { text: c, onmousedown: (e) => e.preventDefault(), onclick: () => insertText(c) }))),
    ]);
    popover.open(anchor, body, { cls: 'symbols' });
  }
  const stamp = (opts) => new Date().toLocaleString(undefined, opts);

  const DOC_TEMPLATES = [
    ['Meeting notes', 'Agenda, decisions, actions', `# Meeting notes\n\n**Date:** ${stamp({ dateStyle: 'long' })}\\\n**Attendees:** \n\n## Agenda\n\n1. First topic\n2. Second topic\n\n## Decisions\n\n- Decision\n\n## Action items\n\n- [ ] Owner: task, due date\n- [ ] Owner: task, due date\n`],
    ['Report', 'Summary, findings, next steps', '# Report title\n\n> [!NOTE]\n> One-paragraph summary of the conclusion.\n\n## Background\n\nWhy this work was done.\n\n## Findings\n\n| Item | Result | Notes |\n| ---- | ------ | ----- |\n| A    |        |       |\n\n## Recommendations\n\n1. First recommendation\n2. Second recommendation\n\n## Next steps\n\n- [ ] First step\n'],
    ['README', 'Project overview and usage', '# Project name\n\nOne sentence describing what this project does.\n\n## Install\n\n```sh\ncommand to install\n```\n\n## Usage\n\n```sh\ncommand to run\n```\n\n## Configuration\n\n| Option | Default | Description |\n| ------ | ------- | ----------- |\n| name   | value   | What it does |\n\n## License\n\nLicense name.\n'],
    ['To-do list', 'Tasks grouped by priority', '# To do\n\n## Today\n\n- [ ] First task\n- [ ] Second task\n\n## This week\n\n- [ ] Task\n\n## Later\n\n- [ ] Task\n'],
    ['Article', 'Title, intro, sections', '---\ntitle: Article title\nauthor: Your name\ndate: ' + new Date().toISOString().slice(0, 10) + '\n---\n\n# Article title\n\nOpening paragraph that tells the reader what they will learn.\n\n## First point\n\nText.\n\n## Second point\n\nText.\n\n## Conclusion\n\nText.\n'],
    ['Letter', 'Formal letter layout', `Your name\\\nYour address\n\n${stamp({ dateStyle: 'long' })}\n\nRecipient name\\\nRecipient address\n\nDear Name,\n\nBody of the letter.\n\nYours sincerely,\n\nYour name\n`],
  ];
  function templateMenu(anchor) {
    showMenu(anchor, [{ title: 'New from template' }, { label: 'Blank document', icon: 'filePlus', kbd: 'Ctrl+N', run: () => newDoc() },
      ...DOC_TEMPLATES.map(([label, , text]) => ({ label, icon: 'doc', run: () => newDoc(text) })),
      { title: 'Other file types' },
      ...Kinds.STARTERS.map(([label, name, text]) => ({ label, icon: 'file', run: () => newDoc(text, name) }))]);
  }

  // ============================================================= commands
  const TEMPLATES = {
    flowchart: '```mermaid\nflowchart LR\n    A[Start] --> B{Decision}\n    B -- Yes --> C[Do the thing]\n    B -- No --> D[Skip it]\n    C --> E[Done]\n    D --> E\n```',
    sequence: '```mermaid\nsequenceDiagram\n    participant U as User\n    participant A as App\n    participant S as Server\n    U->>A: Click\n    A->>S: Request\n    S-->>A: Response\n    A-->>U: Update view\n```',
    classDiagram: '```mermaid\nclassDiagram\n    class Animal {\n        +String name\n        +speak() String\n    }\n    class Dog {\n        +fetch()\n    }\n    Animal <|-- Dog\n```',
    state: '```mermaid\nstateDiagram-v2\n    [*] --> Idle\n    Idle --> Running: start\n    Running --> Idle: stop\n    Running --> [*]: finish\n```',
    gantt: '```mermaid\ngantt\n    title Project plan\n    dateFormat YYYY-MM-DD\n    section Design\n    Research      :a1, 2026-01-05, 7d\n    Prototype     :a2, after a1, 10d\n    section Build\n    Implementation :b1, after a2, 21d\n    Testing        :b2, after b1, 7d\n```',
    pie: '```mermaid\npie showData\n    title Time spent\n    "Writing" : 45\n    "Editing" : 30\n    "Reviewing" : 25\n```',
    mindmap: '```mermaid\nmindmap\n  root((Topic))\n    Idea one\n      Detail\n    Idea two\n    Idea three\n```',
    er: '```mermaid\nerDiagram\n    CUSTOMER ||--o{ ORDER : places\n    ORDER ||--|{ LINE_ITEM : contains\n    CUSTOMER {\n        int id\n        string name\n    }\n```',
    timeline: '```mermaid\ntimeline\n    title Milestones\n    2024 : Idea\n    2025 : Prototype : First users\n    2026 : Launch\n```',
    gitGraph: '```mermaid\ngitGraph\n    commit\n    branch feature\n    commit\n    commit\n    checkout main\n    merge feature\n    commit\n```',
    graphviz: '```dot\ndigraph G {\n    rankdir=LR;\n    node [shape=box, style=rounded];\n    A -> B -> C;\n    A -> C;\n}\n```',
    chart: '```chart\n{\n  "title": { "text": "Monthly sales" },\n  "xAxis": { "type": "category", "data": ["Jan", "Feb", "Mar", "Apr", "May", "Jun"] },\n  "yAxis": { "type": "value" },\n  "series": [\n    { "name": "2025", "type": "bar", "data": [120, 200, 150, 80, 70, 110] },\n    { "name": "2026", "type": "line", "smooth": true, "data": [90, 160, 210, 140, 130, 180] }\n  ]\n}\n```',
    mathBlock: '$$\n\\int_{-\\infty}^{\\infty} e^{-x^2}\\,dx = \\sqrt{\\pi}\n$$',
    note: '> [!NOTE]\n> Useful information the reader should know.',
    tip: '> [!TIP]\n> A helpful suggestion.',
    warning: '> [!WARNING]\n> Something that needs attention.',
    details: '<details>\n<summary>Click to expand</summary>\n\nHidden content goes here.\n\n</details>',
  };
  /** Run `rich` in Write mode, `src` in the Markdown editor, nothing in the reader. */
  const both = (rich, src, anyKind = false) => () => { if (writeMode()) rich(); else if (editorVisible() && (anyKind || isMd())) src(); };
  const fmt = (name, src, arg) => both(() => Rich.exec(name, arg), src);
  const richOnly = (name, arg) => () => { if (writeMode()) Rich.exec(name, arg); else toast('Table tools work in Write mode'); };
  const insert = (key) => both(() => Rich.insertMarkdown(TEMPLATES[key]).catch(fail), () => Editor.insertBlock(TEMPLATES[key]));
  function srcParagraph() {
    const a = cm.getCursor('from').line, b = cm.getCursor('to').line;
    cm.operation(() => {
      for (let i = a; i <= b; i++) {
        const l = cm.getLine(i), n = l.replace(/^(\s*)(#{1,6}\s+|>\s?)/, '$1');
        if (n !== l) cm.replaceRange(n, { line: i, ch: 0 }, { line: i, ch: l.length }, '+input');
      }
    });
    cm.focus();
  }
  function srcClear() {
    const sel = cm.getSelection();
    if (!sel) return toast('Select the text to clean');
    cm.replaceSelection(sel.replace(/(\*\*|__|~~|==|\+\+)(.+?)\1/gs, '$2').replace(/(?<![\\\w])([*_`])(.+?)\1/gs, '$2').replace(/\[([^\]]+)\]\([^)]*\)/g, '$1'), 'around');
  }
  async function addFootnote() {
    const doc = S.active;
    if (!canEdit() || !mdOnly()) return;
    flushRich();
    let n = 1;
    while (doc.cm.getValue().includes(`[^${n}]`)) n++;
    insertText(`[^${n}]`);
    await transformDoc((md) => md.replace(/\s*$/, '') + `\n\n[^${n}]: Footnote text.\n`);
  }

  const COMMANDS = [
    { id: 'newDoc', group: 'File', label: 'New document', kbd: 'Ctrl+N', icon: 'filePlus', run: () => newDoc() },
    { id: 'newTemplate', group: 'File', label: 'New from template…', icon: 'doc', run: () => templateMenu($('#btn-new')) },
    { id: 'newWindow', group: 'File', label: 'New window', kbd: 'Ctrl+Shift+N', icon: 'window', run: () => host.send('new_window') },
    { id: 'open', group: 'File', label: 'Open file…', kbd: 'Ctrl+O', icon: 'file', run: () => openDialog().catch(fail) },
    { id: 'openFolder', group: 'File', label: 'Open folder…', icon: 'folder', run: () => openFolder().catch(fail) },
    { id: 'save', group: 'File', label: 'Save', kbd: 'Ctrl+S', icon: 'save', run: () => save() },
    { id: 'saveAs', group: 'File', label: 'Save as…', kbd: 'Ctrl+Shift+S', run: () => save(S.active, { as: true }) },
    { id: 'exportDocx', group: 'Export', label: 'Word document (.docx)…', icon: 'doc', run: async () => { const p = await exportFile('docx'); if (p) exported(p); } },
    { id: 'exportRtf', group: 'Export', label: 'Rich Text (.rtf)…', icon: 'doc', run: async () => { const p = await exportFile('rtf'); if (p) exported(p); } },
    { id: 'exportHtml', group: 'Export', label: 'Web page (.html)…', icon: 'external', run: exportHtml },
    { id: 'print', group: 'Export', label: 'PDF or printer…', kbd: 'Ctrl+P', icon: 'print', run: printDoc },
    { id: 'exportGoogle', group: 'Export', label: 'Google Docs…', icon: 'cloud', run: exportGoogle },
    { id: 'copyRich', group: 'Export', label: 'Copy with formatting', kbd: 'Ctrl+Shift+C', icon: 'clipboard', run: () => copyRich() },
    { id: 'copyMarkdown', group: 'Export', label: 'Copy Markdown source', icon: 'copy', run: () => { flushRich(); if (S.active) navigator.clipboard.writeText(S.active.cm.getValue()).then(() => toast('Markdown copied'), fail); } },
    { id: 'reveal', group: 'File', label: 'Show in File Explorer', run: () => S.active?.path && host.send('reveal', { path: S.active.path }) },
    { id: 'closeTab', group: 'File', label: 'Close tab', kbd: 'Ctrl+W', run: () => closeDoc() },
    { id: 'nextTab', group: 'View', label: 'Next tab', kbd: 'Ctrl+Tab', run: () => cycleTab(1) },
    { id: 'prevTab', group: 'View', label: 'Previous tab', kbd: 'Ctrl+Shift+Tab', run: () => cycleTab(-1) },
    { id: 'modeRead', group: 'View', label: 'Read: clean reader view', kbd: 'Alt+1', icon: 'read', run: () => setMode('read') },
    { id: 'modeWrite', group: 'View', label: 'Write: Word-style editor', kbd: 'Alt+2', icon: 'edit', run: () => setMode('write') },
    { id: 'modeSplit', group: 'View', label: 'Markdown: source with live preview', kbd: 'Alt+3', icon: 'split', run: () => setMode('split') },
    { id: 'modeEdit', group: 'View', label: 'Markdown: source only', kbd: 'Alt+4', icon: 'code', run: () => setMode('edit') },
    { id: 'cycleMode', group: 'View', label: 'Cycle view mode', kbd: 'Ctrl+E', run: cycleMode },
    { id: 'toggleSidebar', group: 'View', label: 'Toggle sidebar', kbd: 'Ctrl+\\', icon: 'sidebar', run: () => setSidebar(!set.sidebar) },
    { id: 'showOutline', group: 'View', label: 'Show outline', kbd: 'Ctrl+Shift+O', run: () => { setSidebar(true); setPanel('outline'); renderOutline(); } },
    { id: 'showFiles', group: 'View', label: 'Show files', kbd: 'Ctrl+Shift+E', run: () => { setSidebar(true); setPanel('files'); } },
    { id: 'toggleTheme', group: 'View', label: 'Toggle light / dark theme', run: () => { set.theme = isDark() ? 'light' : 'dark'; applyTheme(); persist(); } },
    { id: 'themes', group: 'View', label: 'Choose theme…', icon: 'sun', run: () => themePicker() },
    { id: 'autosave', group: 'File', label: 'Turn AutoSave on or off', icon: 'save', run: () => toggleAutosave().catch(fail) },
    { id: 'lineNumbers', group: 'View', label: 'Toggle line numbers', run: () => { set.lineNumbers = !set.lineNumbers; applyAppearance(); persist(); toast('Line numbers ' + (set.lineNumbers ? 'on' : 'off') + (mode() === 'read' || mode() === 'write' ? ' (shown in the Markdown view)' : '')); } },
    { id: 'zoomIn', group: 'View', label: 'Zoom in', kbd: 'Ctrl+=', run: () => zoom(1.1) },
    { id: 'zoomOut', group: 'View', label: 'Zoom out', kbd: 'Ctrl+-', run: () => zoom(1 / 1.1) },
    { id: 'zoomReset', group: 'View', label: 'Reset zoom', kbd: 'Ctrl+0', run: () => zoom(0) },
    { id: 'find', group: 'Edit', label: 'Find', kbd: 'Ctrl+F', icon: 'search', run: () => openFind(false) },
    { id: 'replace', group: 'Edit', label: 'Find and replace', kbd: 'Ctrl+H', run: () => openFind(true) },
    { id: 'gotoLine', group: 'Edit', label: 'Go to line…', kbd: 'Ctrl+G', run: gotoLine },
    { id: 'palette', group: 'View', label: 'Command palette', kbd: 'Ctrl+Shift+P', icon: 'command', run: openPalette },
    { id: 'undo', group: 'Edit', label: 'Undo', kbd: 'Ctrl+Z', icon: 'undo', native: true, run: both(() => Rich.exec('undo'), () => cm.undo(), true) },
    { id: 'redo', group: 'Edit', label: 'Redo', kbd: 'Ctrl+Y', icon: 'redo', native: true, run: both(() => Rich.exec('redo'), () => cm.redo(), true) },
    { id: 'paragraph', group: 'Format', label: 'Normal text', kbd: 'Ctrl+Alt+0', run: both(() => Rich.exec('paragraph'), srcParagraph) },
    ...[1, 2, 3, 4, 5, 6].map((n) => ({ id: 'h' + n, group: 'Format', label: 'Heading ' + n, kbd: 'Ctrl+' + n, run: fmt('heading', () => Editor.toggleLinePrefix('heading', n), n) })),
    { id: 'bold', group: 'Format', label: 'Bold', kbd: 'Ctrl+B', icon: 'bold', run: fmt('bold', () => Editor.toggleWrap('**')) },
    { id: 'italic', group: 'Format', label: 'Italic', kbd: 'Ctrl+I', icon: 'italic', run: fmt('italic', () => Editor.toggleWrap('*')) },
    { id: 'underline', group: 'Format', label: 'Underline', kbd: 'Ctrl+U', icon: 'underline', run: fmt('underline', () => Editor.toggleWrap('++')) },
    { id: 'strike', group: 'Format', label: 'Strikethrough', kbd: 'Ctrl+Shift+X', icon: 'strike', run: fmt('strike', () => Editor.toggleWrap('~~')) },
    { id: 'highlight', group: 'Format', label: 'Highlight', kbd: 'Ctrl+Shift+H', icon: 'highlight', run: fmt('highlight', () => Editor.toggleWrap('==')) },
    { id: 'code', group: 'Format', label: 'Inline code', kbd: 'Ctrl+`', icon: 'code', run: fmt('code', () => Editor.toggleWrap('`')) },
    { id: 'sup', group: 'Format', label: 'Superscript', icon: 'sup', run: fmt('sup', () => Editor.toggleWrap('^')) },
    { id: 'sub', group: 'Format', label: 'Subscript', icon: 'sub', run: fmt('sub', () => Editor.toggleWrap('~')) },
    { id: 'clear', group: 'Format', label: 'Clear formatting', kbd: 'Ctrl+Space', icon: 'eraser', run: both(() => Rich.exec('clear'), srcClear) },
    { id: 'link', group: 'Format', label: 'Link', kbd: 'Ctrl+K', icon: 'link', run: both(() => Rich.linkDialog($('#toolbar [data-cmd="link"]')), Editor.link) },
    { id: 'image', group: 'Insert', label: 'Image…', kbd: 'Ctrl+Shift+I', icon: 'image', run: () => pickImage().catch(fail) },
    { id: 'ul', group: 'Format', label: 'Bulleted list', kbd: 'Ctrl+Shift+8', icon: 'ul', run: fmt('ul', () => Editor.toggleLinePrefix('ul')) },
    { id: 'ol', group: 'Format', label: 'Numbered list', kbd: 'Ctrl+Shift+7', icon: 'ol', run: fmt('ol', () => Editor.toggleLinePrefix('ol')) },
    { id: 'task', group: 'Format', label: 'Checklist', kbd: 'Ctrl+Shift+9', icon: 'task', run: fmt('task', () => Editor.toggleLinePrefix('task')) },
    { id: 'indent', group: 'Format', label: 'Increase indent', kbd: 'Ctrl+]', icon: 'indent', run: both(() => Rich.exec('indent'), () => cm.execCommand('indentMore'), true) },
    { id: 'outdent', group: 'Format', label: 'Decrease indent', kbd: 'Ctrl+[', icon: 'outdent', run: both(() => Rich.exec('outdent'), () => cm.execCommand('indentLess'), true) },
    { id: 'quote', group: 'Format', label: 'Quote', kbd: 'Ctrl+Shift+.', icon: 'quote', run: fmt('quote', () => Editor.toggleLinePrefix('quote')) },
    { id: 'codeBlock', group: 'Insert', label: 'Code block', kbd: 'Ctrl+Shift+K', icon: 'codeblock', run: fmt('codeBlock', Editor.codeBlock) },
    { id: 'hr', group: 'Insert', label: 'Horizontal line', icon: 'hr', run: fmt('hr', () => Editor.insertBlock('---')) },
    { id: 'table', group: 'Insert', label: 'Table (3 × 3)', icon: 'table', run: both(() => Rich.exec('table', { rows: 3, cols: 3 }), () => Editor.table(3, 3)) },
    { id: 'rowBelow', group: 'Table', label: 'Insert row below', run: richOnly('rowBelow') },
    { id: 'rowAbove', group: 'Table', label: 'Insert row above', run: richOnly('rowAbove') },
    { id: 'colRight', group: 'Table', label: 'Insert column right', run: richOnly('colRight') },
    { id: 'colLeft', group: 'Table', label: 'Insert column left', run: richOnly('colLeft') },
    { id: 'delRow', group: 'Table', label: 'Delete row', run: richOnly('delRow') },
    { id: 'delCol', group: 'Table', label: 'Delete column', run: richOnly('delCol') },
    { id: 'delTable', group: 'Table', label: 'Delete table', run: richOnly('delTable') },
    { id: 'alignLeft', group: 'Table', label: 'Align column left', run: richOnly('align', 'left') },
    { id: 'alignCenter', group: 'Table', label: 'Align column centre', run: richOnly('align', 'center') },
    { id: 'alignRight', group: 'Table', label: 'Align column right', run: richOnly('align', 'right') },
    { id: 'formatTable', group: 'Table', label: 'Tidy table at cursor (Markdown view)', kbd: 'Ctrl+Shift+F', run: () => { if (!editorVisible()) return toast('Tables are always tidy in Write mode'); if (!Editor.formatTable()) toast('Place the cursor inside a table first'); } },
    { id: 'mathInline', group: 'Insert', label: 'Inline formula', kbd: 'Ctrl+M', icon: 'sigma', run: both(() => Rich.exec('mathInline'), () => Editor.toggleWrap('$')) },
    { id: 'mathBlock', group: 'Insert', label: 'Formula block', kbd: 'Ctrl+Shift+M', icon: 'sigma', run: insert('mathBlock') },
    { id: 'insDrawing', group: 'Insert', label: 'Drawing (visual designer)…', kbd: 'Ctrl+Shift+D', icon: 'edit', run: () => (S.active?.kind.id === 'svg' ? designFile() : insertDrawing()).catch(fail) },
    { id: 'insFlow', group: 'Insert', label: 'Flowchart', icon: 'diagram', run: insert('flowchart') },
    { id: 'insSeq', group: 'Insert', label: 'Sequence diagram', icon: 'diagram', run: insert('sequence') },
    { id: 'insClass', group: 'Insert', label: 'Class diagram', icon: 'diagram', run: insert('classDiagram') },
    { id: 'insState', group: 'Insert', label: 'State diagram', icon: 'diagram', run: insert('state') },
    { id: 'insEr', group: 'Insert', label: 'Entity relationship diagram', icon: 'diagram', run: insert('er') },
    { id: 'insGantt', group: 'Insert', label: 'Gantt chart', icon: 'diagram', run: insert('gantt') },
    { id: 'insTimeline', group: 'Insert', label: 'Timeline', icon: 'diagram', run: insert('timeline') },
    { id: 'insMind', group: 'Insert', label: 'Mind map', icon: 'diagram', run: insert('mindmap') },
    { id: 'insGit', group: 'Insert', label: 'Git graph', icon: 'diagram', run: insert('gitGraph') },
    { id: 'insPie', group: 'Insert', label: 'Pie chart', icon: 'chart', run: insert('pie') },
    { id: 'insChart', group: 'Insert', label: 'Data chart (bar / line)', icon: 'chart', run: insert('chart') },
    { id: 'insDot', group: 'Insert', label: 'Graphviz graph', icon: 'diagram', run: insert('graphviz') },
    { id: 'insNote', group: 'Insert', label: 'Callout: note', icon: 'info', run: insert('note') },
    { id: 'insTip', group: 'Insert', label: 'Callout: tip', icon: 'info', run: insert('tip') },
    { id: 'insWarn', group: 'Insert', label: 'Callout: warning', icon: 'info', run: insert('warning') },
    { id: 'insDetails', group: 'Insert', label: 'Collapsible section', run: insert('details') },
    { id: 'insFootnote', group: 'Insert', label: 'Footnote', run: () => addFootnote().catch(fail) },
    { id: 'insSymbol', group: 'Insert', label: 'Symbol or emoji…', icon: 'smile', run: () => { if (canEdit()) setTimeout(() => symbolPicker($('#tb-insert')), 0); } },
    { id: 'insDate', group: 'Insert', label: 'Today\u2019s date', icon: 'clock', run: () => canEdit() && insertText(stamp({ dateStyle: 'long' })) },
    { id: 'insTime', group: 'Insert', label: 'Current date and time', icon: 'clock', run: () => canEdit() && insertText(stamp({ dateStyle: 'medium', timeStyle: 'short' })) },
    { id: 'toc', group: 'Tools', label: 'Insert or update table of contents', icon: 'listTree', run: () => updateToc().catch(fail) },
    { id: 'stats', group: 'Tools', label: 'Document statistics', icon: 'chart', run: () => showStats().catch(fail) },
    { id: 'tidy', group: 'Tools', label: 'Tidy Markdown formatting…', icon: 'sparkles', run: () => tidyMarkdown().catch(fail) },
    { id: 'toTable', group: 'Tools', label: 'Convert selected text to table', icon: 'table', run: textToTable },
    { id: 'caseUpper', group: 'Tools', label: 'UPPERCASE', run: () => changeCase('upper') },
    { id: 'caseLower', group: 'Tools', label: 'lowercase', run: () => changeCase('lower') },
    { id: 'caseTitle', group: 'Tools', label: 'Title Case', run: () => changeCase('title') },
    { id: 'caseSentence', group: 'Tools', label: 'Sentence case', run: () => changeCase('sentence') },
    { id: 'sortAsc', group: 'Tools', label: 'Sort selected lines A to Z', run: () => sortLines(false) },
    { id: 'sortDesc', group: 'Tools', label: 'Sort selected lines Z to A', run: () => sortLines(true) },
    { id: 'formatData', group: 'Data', label: 'Format document (JSON, YAML, TOML)', kbd: 'Alt+Shift+F', icon: 'sparkles', run: () => formatData(false) },
    { id: 'minifyData', group: 'Data', label: 'Minify JSON', run: () => formatData(true) },
    { id: 'toJson', group: 'Data', label: 'Convert to JSON', run: () => convertData('json') },
    { id: 'toYaml', group: 'Data', label: 'Convert to YAML', run: () => convertData('yaml') },
    { id: 'toToml', group: 'Data', label: 'Convert to TOML', run: () => convertData('toml') },
    ...Kinds.STARTERS.map(([label, name, text], i) => ({ id: 'newKind' + i, group: 'File', label: 'New ' + label, icon: 'file', run: () => newDoc(text, name) })),
    { id: 'integrations', group: 'File', label: 'Integrations (Google account)…', icon: 'cloud', run: () => integrations().catch(fail) },
    { id: 'welcome', group: 'Help', label: 'Feature tour', icon: 'sparkles', run: () => openWelcome().catch(fail) },
    { id: 'install', group: 'Help', label: 'Install and set as default Markdown app', run: installApp },
    { id: 'defaultApps', group: 'Help', label: 'Open Windows default apps settings', run: () => host.send('default_apps') },
    { id: 'devtools', group: 'Help', label: 'Developer tools', kbd: 'F12', run: () => host.send('devtools') },
    { id: 'about', group: 'Help', label: 'About Inkdown', icon: 'info', run: () => dialog({
      title: 'Inkdown ' + S.info.version,
      message: 'A native Markdown viewer and editor for Windows, written in Rust.\n\nWord-style editing, CommonMark + GitHub Flavored Markdown, math (KaTeX), diagrams (Mermaid, Graphviz), charts (ECharts) and export to Word, RTF, HTML, PDF and Google Docs. Everything renders offline.',
      buttons: [{ label: 'Close', value: true, primary: true }],
    }) },
  ];
  const MD_GROUPS = new Set(['Format', 'Insert', 'Table']);
  const CMD = Object.fromEntries(COMMANDS.map((c) => [c.id, c]));
  const KEYS = new Map(COMMANDS.filter((c) => c.kbd && !c.native).map((c) => [c.kbd.toLowerCase(), c]));
  KEYS.set('f1', CMD.palette);
  KEYS.set('ctrl+shift+z', { run: CMD.redo.run });
  const run = (id) => CMD[id]?.run();
  function m(id) { return { label: CMD[id].label, icon: CMD[id].icon, kbd: CMD[id].kbd, run: CMD[id].run }; }

  const CODE_KEYS = { Equal: '=', Minus: '-', NumpadAdd: '=', NumpadSubtract: '-', Backquote: '`', Backslash: '\\', Period: '.', Comma: ',', Slash: '/', Tab: 'tab', Numpad0: '0', Space: 'space', BracketLeft: '[', BracketRight: ']' };
  function comboOf(e) {
    let key = CODE_KEYS[e.code];
    if (!key) {
      if (/^Key[A-Z]$/.test(e.code)) key = e.code.slice(3).toLowerCase();
      else if (/^Digit\d$/.test(e.code)) key = e.code.slice(5);
      else if (/^F\d+$/.test(e.code)) key = e.code.toLowerCase();
      else return null;
    }
    return (e.ctrlKey ? 'ctrl+' : '') + (e.altKey ? 'alt+' : '') + (e.shiftKey ? 'shift+' : '') + key;
  }
  document.addEventListener('keydown', (e) => {
    if ($('.scrim') || $('.designer') || Lightbox.isOpen) return;
    if (e.key === 'Escape') {
      if (popover.isOpen) { popover.close(); e.preventDefault(); return; }
      if (!$('#findbar').hidden) { closeFind(); e.preventDefault(); return; }
      return;
    }
    if (!e.ctrlKey && !e.altKey && !/^F\d+$/.test(e.code)) return;
    if (e.target.closest?.('.popover')) return;
    const combo = comboOf(e);
    const cmd = combo && KEYS.get(combo);
    if (!cmd || (e.ctrlKey && e.altKey && combo !== 'ctrl+alt+0')) return;
    // In code files the Markdown formatting keys belong to the editor (Sublime bindings).
    if (S.active && !isMd() && MD_GROUPS.has(cmd.group) && !(cmd.id === 'insDrawing' && S.active.kind.id === 'svg')) return;
    e.preventDefault();
    e.stopPropagation();
    popover.close();
    cmd.run();
  }, true);
  document.addEventListener('wheel', (e) => {
    if (!e.ctrlKey || $('.designer')) return;
    e.preventDefault();
    zoom(e.deltaY < 0 ? 1.1 : 1 / 1.1);
  }, { passive: false, capture: true });
  document.addEventListener('click', (e) => {
    const el = e.target.closest('[data-cmd]');
    if (el) run(el.dataset.cmd);
  });

  // ============================================================== toolbar
  let styleLabel, tableBtn;
  function buildToolbar() {
    const bar = $('#toolbar');
    const keepFocus = (e) => e.preventDefault();
    const btn = (id) => h('button', { class: 'icon-btn', 'data-cmd': id, 'data-tip': CMD[id].label + (CMD[id].kbd ? ` (${CMD[id].kbd})` : ''), html: icon(CMD[id].icon), onmousedown: keepFocus });
    const sep = () => h('div', { class: 'sep' });
    const menuOf = (ids) => ids.map((id) => (typeof id === 'string' && id !== '-' ? m(id) : id));
    const drop = (attrs, html, items) => {
      const b = h('button', { class: 'icon-btn drop', html, onmousedown: keepFocus, ...attrs });
      b.addEventListener('click', () => showMenu(b, menuOf(items)));
      return b;
    };
    styleLabel = h('span', { text: 'Normal text' });
    const style = drop({ class: 'icon-btn drop style-drop', 'data-tip': 'Paragraph style' }, '', ['paragraph', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', '-', 'quote', 'codeBlock']);
    style.append(styleLabel, h('span', { html: icon('down'), style: 'display:contents' }));
    const pick = h('button', { class: 'icon-btn', 'data-tip': 'Insert table', html: icon('table'), onmousedown: keepFocus });
    pick.addEventListener('click', () => tablePicker(pick));
    tableBtn = drop({ 'data-tip': 'Table tools', hidden: true, class: 'icon-btn drop accent' }, icon('table') + '<span>Table</span>' + icon('down'),
      ['rowBelow', 'rowAbove', 'colRight', 'colLeft', '-', 'alignLeft', 'alignCenter', 'alignRight', '-', 'delRow', 'delCol', 'delTable']);
    const more = drop({ 'data-tip': 'More formatting' }, '<b class="aa">Aa</b>' + icon('down'), ['sup', 'sub', 'code', 'highlight', '-', 'clear']);
    const ins = drop({ id: 'tb-insert', 'data-tip': 'Insert diagram, chart, formula, symbol…' }, icon('plus') + '<span>Insert</span>' + icon('down'), [
      { title: 'Diagrams' }, 'insDrawing', '-', 'insFlow', 'insSeq', 'insClass', 'insState', 'insEr', 'insGantt', 'insTimeline', 'insMind', 'insGit', 'insDot',
      { title: 'Charts' }, 'insPie', 'insChart',
      { title: 'Formulas' }, 'mathInline', 'mathBlock',
      { title: 'Blocks' }, 'insNote', 'insTip', 'insWarn', 'insDetails', 'insFootnote',
      { title: 'Text' }, 'insSymbol', 'insDate', 'insTime',
    ]);
    const tools = drop({ 'data-tip': 'Document tools' }, icon('wrench') + '<span>Tools</span>' + icon('down'), [
      'toc', 'stats', 'tidy', 'toTable', { title: 'Change case' }, 'caseUpper', 'caseLower', 'caseTitle', 'caseSentence',
      { title: 'Lines' }, 'sortAsc', 'sortDesc', 'formatTable',
    ]);
    bar.append(btn('undo'), btn('redo'), sep(), style, sep(), btn('bold'), btn('italic'), btn('underline'), btn('strike'), more, sep(),
      btn('link'), btn('image'), sep(), btn('ul'), btn('ol'), btn('task'), btn('outdent'), btn('indent'), sep(),
      pick, btn('codeBlock'), btn('hr'), sep(), ins, tools, tableBtn);
  }
  // ---- toolbar for non-Markdown documents
  function applyKindUI(doc) {
    const k = doc ? doc.kind : Kinds.DEFS.markdown;
    const md = k.id === 'markdown';
    document.body.dataset.kind = md ? 'markdown' : 'other';
    const sw = $('#mode-switch');
    sw.hidden = k.modes.length < 2;
    $('button[data-mode="write"]', sw).hidden = !k.modes.includes('write');
    $('button[data-mode="split"] span:last-child', sw).textContent = md ? 'Markdown' : 'Code';
    $('button[data-mode="read"] span:last-child', sw).textContent = md || k.id === 'adoc' || k.id === 'html' ? 'Read' : 'View';
    $('#st-kind').textContent = doc ? k.label : '';
    requestAnimationFrame(() => { if (S.active === doc && doc && Editor.indentLabel) $('#st-kind').textContent = k.label + ' · ' + Editor.indentLabel; });
    if (cm.getOption('lineNumbers') !== wantGutter()) { cm.setOption('lineNumbers', wantGutter()); cm.refresh(); }
    document.body.classList.toggle('gutter', wantGutter());
    const bar = $('#toolbar2');
    bar.replaceChildren();
    if (!doc || md) return;
    const keep = (e) => e.preventDefault();
    const btn = (id) => h('button', { class: 'icon-btn', 'data-cmd': id, 'data-tip': CMD[id].label + (CMD[id].kbd ? ` (${CMD[id].kbd})` : ''), html: icon(CMD[id].icon), onmousedown: keep });
    const text = (label, tip, fn, ic) => h('button', { class: 'icon-btn drop', 'data-tip': tip, html: (ic ? icon(ic) : '') + `<span>${label}</span>`, onmousedown: keep, onclick: fn });
    const drop = (label, tip, items, ic) => {
      const b = h('button', { class: 'icon-btn drop', 'data-tip': tip, html: (ic ? icon(ic) : '') + `<span>${label}</span>` + icon('down'), onmousedown: keep });
      b.addEventListener('click', () => showMenu(b, items));
      return b;
    };
    const sep = () => h('div', { class: 'sep' });
    bar.append(btn('undo'), btn('redo'), sep(), h('span', { class: 'kind-chip', text: k.label }));
    if (k.data) {
      bar.append(sep(), text('Format', 'Re-indent and normalise (Alt+Shift+F)', () => formatData(false), 'sparkles'));
      if (k.data === 'json') bar.append(text('Minify', 'Remove all whitespace', () => formatData(true)));
      bar.append(drop('Convert', 'Open a converted copy in a new tab', ['json', 'yaml', 'toml'].filter((f) => f !== k.data).map((f) => ({ label: 'To ' + f.toUpperCase(), icon: 'file', run: () => convertData(f) }))));
    }
    if (k.id === 'mermaid') {
      bar.append(sep(), drop('Templates', 'Replace the diagram with a starter', ['flowchart', 'sequence', 'classDiagram', 'state', 'er', 'gantt', 'timeline', 'mindmap', 'gitGraph', 'pie']
        .map((key) => ({ label: key.replace(/([A-Z])/g, ' $1').replace(/^./, (c) => c.toUpperCase()), icon: 'diagram', run: () => transformDoc(() => TEMPLATES[key].split('\n').slice(1, -1).join('\n') + '\n') })), 'diagram'));
    }
    if (k.id === 'svg') bar.append(sep(), text('Open designer', 'Edit this drawing visually (Ctrl+Shift+D)', () => designFile().catch(fail), 'edit'));
    if (k.diagram) {
      bar.append(text('Save image', 'Save the drawing as SVG or PNG', async () => {
        await ensureRendered();
        const b = Preview.root.querySelector('.diagram [data-act="save-img"]');
        if (b && previewVisible()) b.click(); else toast('Open the View or Code tab to save the drawing');
      }, 'download'));
    }
    bar.append(sep(), drop('Tools', 'Editing tools', [m('find'), m('replace'), '-', m('sortAsc'), m('sortDesc'), '-', m('caseUpper'), m('caseLower'), m('caseTitle')], 'wrench'),
      h('span', { class: 'grow' }), h('button', { id: 'data-status', class: 'data-status' }));
  }
  async function formatData(compact) {
    const doc = S.active;
    if (!doc || !doc.kind.data) return toast('Formatting is available for JSON, YAML and TOML files');
    const text = doc.cm.getValue();
    if (doc.kind.data !== 'json' && /(^|\s)#/.test(text)) {
      const ok = await dialog({ title: `Format ${doc.kind.label}`, message: 'Formatting rewrites the file from its data, so comments will be removed. You can undo it afterwards.', buttons: [{ label: 'Cancel', value: false }, { label: 'Format anyway', value: true, primary: true }] });
      if (!ok) return;
    }
    try {
      const out = await host.invoke('data_convert', { from: doc.kind.data, to: doc.kind.data, text, compact: !!compact });
      await transformDoc(() => out);
    } catch (e) { fail(e); }
  }
  async function convertData(to) {
    const doc = S.active;
    if (!doc || !doc.kind.data) return toast('Conversion is available for JSON, YAML and TOML files');
    if (doc.kind.data === to) return toast(`Already ${to.toUpperCase()}`);
    try {
      const out = await host.invoke('data_convert', { from: doc.kind.data, to, text: doc.cm.getValue() });
      newDoc(out, `${baseName(doc)}.${to}`);
      const made = S.active;
      made.saved = ''; made.clean = -1; made.dirty = true;
      renderTabs(); updateTitle(); updateStatus();
    } catch (e) { fail(e); }
  }

  function updateToolbar() {
    if (!styleLabel) return;
    const st = writeMode() ? Rich.state() : null;
    for (const b of $$('#toolbar [data-cmd]')) b.classList.toggle('on', !!(st && st.active[b.dataset.cmd]));
    styleLabel.textContent = st ? st.style : 'Style';
    tableBtn.hidden = !(st && st.inTable);
  }
  function tablePicker(anchor) {
    const label = h('p', { text: 'Pick a size' });
    const cells = [];
    const grid = h('div', { class: 'grid' });
    for (let r = 1; r <= 7; r++) for (let c = 1; c <= 8; c++) {
      const cell = h('i');
      cell.addEventListener('mouseenter', () => {
        cells.forEach((x) => x.el.classList.toggle('on', x.r <= r && x.c <= c));
        label.textContent = `${c} columns × ${r} rows`;
      });
      cell.addEventListener('mousedown', (e) => e.preventDefault());
      cell.addEventListener('click', () => {
        popover.close();
        if (writeMode()) Rich.exec('table', { rows: r, cols: c });
        else if (editorVisible()) Editor.table(r, c);
      });
      cells.push({ el: cell, r, c });
      grid.append(cell);
    }
    popover.open(anchor, [grid, label], { cls: 'table-picker' });
  }

  // ================================================================ menus
  function mainMenu() {
    const recent = set.recent.filter((p) => !S.docs.some((d) => d.path && pathx.same(d.path, p))).slice(0, 6);
    showMenu($('#btn-menu'), [
      m('newDoc'), m('newTemplate'), m('newWindow'), m('open'), m('openFolder'), '-',
      m('save'), m('saveAs'), { label: 'Export…', icon: 'download', run: () => setTimeout(() => exportMenu($('#btn-export')), 0) }, '-',
      recent.length ? { title: 'Recent' } : null,
      ...recent.map((p) => ({ label: pathx.basename(p), icon: 'clock', run: () => openPath(p) })),
      recent.length ? '-' : null,
      m('integrations'), m('stats'), m('palette'), m('welcome'),
      !S.info.isDefault || !S.info.installed ? m('install') : null,
      m('about'),
    ], { align: 'right' });
  }
  $('#btn-menu').addEventListener('click', mainMenu);
  $('#btn-new').addEventListener('contextmenu', (e) => { e.preventDefault(); e.stopPropagation(); templateMenu($('#btn-new')); });

  function settingsPanel() {
    const seg = (key, opts, after) => h('div', { class: 'seg' }, opts.map(([val, label]) =>
      h('button', { class: set[key] === val ? 'active' : '', text: label, onclick: (e) => {
        set[key] = val;
        for (const b of e.target.parentElement.children) b.classList.toggle('active', b === e.target);
        after(); persist();
      } })));
    const range = (key, min, max, step) => h('input', { type: 'range', min, max, step, value: set[key], oninput: (e) => { set[key] = +e.target.value; applyAppearance(); persist(); } });
    const sw = (key, after = applyAppearance) => h('button', { class: 'switch' + (set[key] ? ' on' : ''), onclick: (e) => { set[key] = !set[key]; e.target.classList.toggle('on', set[key]); after(); persist(); } });
    const row = (label, control) => h('div', { class: 'row' }, h('span', { text: label }), control);
    popover.open($('#btn-settings'), [
      h('div', { class: 'menu-title', style: 'padding-left:0', text: 'Appearance' }),
      row('Theme', h('button', { class: 'btn sm', text: (THEMES[set.theme] ? THEMES[set.theme].name : 'Match Windows') + '  ›', onclick: () => setTimeout(() => themePicker($('#btn-settings')), 0) })),
      row('Reading font', seg('serif', [[false, 'Sans'], [true, 'Serif']], applyAppearance)),
      row('Page width', seg('width', [['narrow', 'Narrow'], ['medium', 'Medium'], ['wide', 'Wide'], ['full', 'Full']], applyAppearance)),
      row('Text size', range('fontSize', 13, 22, 0.5)),
      h('div', { class: 'menu-sep' }),
      h('div', { class: 'menu-title', style: 'padding-left:0', text: 'Markdown editor' }),
      row('Editor text size', range('editorSize', 11, 20, 0.5)),
      row('Line numbers', sw('lineNumbers', () => { applyAppearance(); if (mode() === 'read' || mode() === 'write') toast('Line numbers appear in the Markdown view'); })),
      row('Wrap long lines', sw('wrap')),
      row('Synchronised scrolling', sw('syncScroll', () => {})),
    ], { align: 'right', cls: 'settings' });
  }
  $('#btn-settings').addEventListener('click', settingsPanel);

  // ---- command palette
  function openPalette() {
    const input = h('input', { type: 'text', placeholder: 'Type a command…', spellcheck: 'false' });
    const list = h('div', { class: 'palette-list' });
    const scrim = h('div', { class: 'scrim' }, h('div', { class: 'palette' }, input, list));
    let items = [], sel = 0;
    const close = () => scrim.remove();
    const paint = () => {
      const q = input.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
      items = COMMANDS.filter((c) => q.every((w) => (c.group + ' ' + c.label).toLowerCase().includes(w)));
      sel = clamp(sel, 0, Math.max(0, items.length - 1));
      list.replaceChildren(...items.map((c, i) => h('button', {
        class: 'palette-item' + (i === sel ? ' sel' : ''),
        onclick: () => { close(); c.run(); },
        onmousemove: () => { if (sel !== i) { sel = i; mark(); } },
      }, h('small', { text: c.group }), h('span', { text: c.label }), c.kbd ? h('kbd', { text: c.kbd }) : null)));
    };
    const mark = () => Array.from(list.children).forEach((el, i) => el.classList.toggle('sel', i === sel));
    input.addEventListener('input', () => { sel = 0; paint(); });
    input.addEventListener('keydown', (e) => {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        sel = (sel + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % Math.max(1, items.length);
        mark();
        list.children[sel]?.scrollIntoView({ block: 'nearest' });
      } else if (e.key === 'Enter') { e.preventDefault(); const c = items[sel]; close(); c?.run(); }
      else if (e.key === 'Escape') { e.preventDefault(); close(); if (editorVisible()) cm.focus(); else if (writeMode()) Rich.focus(); }
    });
    scrim.addEventListener('mousedown', (e) => { if (e.target === scrim) close(); });
    $('#overlays').append(scrim);
    paint();
    input.focus();
  }

  // ---- context menu
  document.addEventListener('contextmenu', (e) => {
    // The rich editor keeps the native menu: it carries spelling suggestions.
    if (e.target.closest('#rich .ProseMirror') && !e.target.closest('.rich-code-head')) return;
    e.preventDefault();
    const at = { x: e.clientX, y: e.clientY };
    if (e.target.closest('.CodeMirror')) {
      const has = cm.somethingSelected();
      return showMenu(null, [
        { label: 'Cut', kbd: 'Ctrl+X', disabled: !has, run: () => navigator.clipboard.writeText(cm.getSelection()).then(() => cm.replaceSelection('')) },
        { label: 'Copy', kbd: 'Ctrl+C', icon: 'copy', disabled: !has, run: () => navigator.clipboard.writeText(cm.getSelection()) },
        { label: 'Paste', kbd: 'Ctrl+V', run: () => navigator.clipboard.readText().then((t) => { cm.replaceSelection(t); cm.focus(); }).catch(fail) },
        { label: 'Select all', kbd: 'Ctrl+A', run: () => { cm.execCommand('selectAll'); cm.focus(); } },
        '-', m('bold'), m('italic'), m('code'), m('link'), '-', m('formatTable'),
      ], { at });
    }
    if (e.target.closest('#preview')) {
      const picked = String(getSelection());
      const link = e.target.closest('a[href]');
      const img = e.target.closest('img');
      const node = e.target;
      return showMenu(null, [
        { label: 'Copy', kbd: 'Ctrl+C', icon: 'copy', disabled: !picked, run: () => document.execCommand('copy') },
        link ? { label: 'Copy link address', icon: 'link', run: () => navigator.clipboard.writeText(link.getAttribute('href')) } : null,
        img ? { label: 'Open image in full view', icon: 'expand', run: () => Lightbox.open(img) } : null,
        { label: 'Select all', kbd: 'Ctrl+A', run: () => getSelection().selectAllChildren(Preview.root) },
        '-',
        { label: 'Edit this passage', icon: 'edit', run: () => {
          const hit = node.closest('[data-sourcepos]');
          if (hit) ink.jumpToSource(parseInt(hit.getAttribute('data-sourcepos'), 10));
        } },
        m('copyRich'),
        { label: 'Export…', icon: 'download', run: () => setTimeout(() => exportMenu(null, { at }), 0) },
      ], { at });
    }
  });
  // Ctrl+A in the reader should select the document, not the whole window chrome.
  document.addEventListener('keydown', (e) => {
    if (e.ctrlKey && e.code === 'KeyA' && !$('.designer') && !e.target.closest('input, textarea, .CodeMirror, .ProseMirror') && S.active && previewVisible()) {
      e.preventDefault();
      getSelection().selectAllChildren(Preview.root);
    }
  });

  // ============================================================= find bar
  const findOpts = { matchCase: false, wholeWord: false, regex: false };
  const findTarget = () => (mode() === 'read' ? Preview : mode() === 'write' ? Rich : Editor);
  function paintCount(index) {
    const el = $('#find-count'), n = findTarget().findCount;
    const q = $('#find-input').value;
    el.classList.toggle('none', !!q && !n);
    el.textContent = !q ? '' : !n ? 'No results' : index >= 0 ? `${index + 1} of ${n}` : `${n} found`;
  }
  function runFind(step = true) {
    const target = findTarget();
    for (const t of [Editor, Preview, Rich]) if (t !== target) t.clearFind();
    $('#replace-row').hidden = target !== Editor || !$('#findbar').dataset.replace;
    const n = target.find($('#find-input').value, findOpts);
    paintCount(n && step ? target.findStep(1) : -1);
  }
  const refreshFind = debounce(() => { if (!$('#findbar').hidden) { Editor.find($('#find-input').value, findOpts); paintCount(-1); } }, 250);
  function openFind(replace) {
    if (!S.active) return;
    const bar = $('#findbar'), input = $('#find-input');
    const sel = mode() === 'read' ? String(getSelection()) : mode() === 'write' ? Rich.selectionText() : cm.getSelection();
    if (replace && !editorVisible()) setMode('split', { focus: false });
    bar.hidden = false;
    if (replace) bar.dataset.replace = '1'; else delete bar.dataset.replace;
    if (sel && !sel.includes('\n') && sel.length < 200) input.value = sel;
    input.focus();
    input.select();
    runFind(false);
  }
  function closeFind() {
    $('#findbar').hidden = true;
    Editor.clearFind();
    Preview.clearFind();
    Rich.clearFind();
    if (editorVisible()) cm.focus();
  }
  const stepFind = (dir) => paintCount(findTarget().findStep(dir));
  $('#find-input').addEventListener('input', debounce(() => runFind(true), 110));
  $('#find-input').addEventListener('keydown', (e) => {
    if (e.key === 'Enter') { e.preventDefault(); stepFind(e.shiftKey ? -1 : 1); }
  });
  $('#replace-input').addEventListener('keydown', (e) => {
    if (e.key === 'Enter') { e.preventDefault(); $('#replace-one').click(); }
  });
  $('#find-next').addEventListener('click', () => stepFind(1));
  $('#find-prev').addEventListener('click', () => stepFind(-1));
  $('#find-close').addEventListener('click', closeFind);
  for (const [id, key] of [['find-case', 'matchCase'], ['find-word', 'wholeWord'], ['find-regex', 'regex']]) {
    $('#' + id).addEventListener('click', (e) => {
      findOpts[key] = !findOpts[key];
      e.currentTarget.classList.toggle('on', findOpts[key]);
      runFind(true);
    });
  }
  $('#replace-one').addEventListener('click', () => {
    if (Editor.findIndex < 0) return stepFind(1);
    if (Editor.replaceOne($('#replace-input').value, !findOpts.regex)) {
      refreshFind.cancel();
      Editor.find($('#find-input').value, findOpts);
      paintCount(Editor.findStep(1));
    }
  });
  $('#replace-all').addEventListener('click', () => {
    if (!Editor.findCount) return;
    const n = Editor.replaceAll($('#replace-input').value, !findOpts.regex);
    refreshFind.cancel();
    Editor.find($('#find-input').value, findOpts);
    paintCount(-1);
    toast(`Replaced ${n} occurrence${n === 1 ? '' : 's'}`);
  });

  // ========================================================= start screen
  function renderStart() {
    const el = $('#start');
    const card = (ic, title, sub, fn) => h('button', { class: 'card', onclick: fn }, h('span', { html: icon(ic), style: 'display:contents' }), h('b', { text: title }), h('small', { text: sub }));
    const needsSetup = !S.info.isDefault || !S.info.installed;
    el.replaceChildren(h('div', { class: 'start-inner' },
      h('div', { class: 'start-hero' }, h('img', { src: 'icon.png', alt: '' }),
        h('div', {}, h('h1', { text: 'Inkdown' }), h('p', { text: 'A fast editor for Markdown, code, data and diagrams.' }))),
      needsSetup ? h('div', { class: 'start-note' },
        h('div', {}, h('b', { text: S.info.installed ? 'Make Inkdown your Markdown app' : 'Install Inkdown' }),
          h('small', { text: 'Open .md files with a double-click, add a Start Menu shortcut. No administrator rights needed.' })),
        h('button', { class: 'btn primary', text: S.info.installed ? 'Set as default' : 'Install', onclick: installApp })) : null,
      h('div', { class: 'start-actions' },
        card('file', 'Open file', 'Ctrl+O', () => run('open')),
        card('filePlus', 'New document', 'Ctrl+N', () => newDoc()),
        card('sparkles', 'Feature tour', 'See everything Inkdown renders', () => run('welcome'))),
      h('h2', { text: 'Start from a template' }),
      h('div', { class: 'template-row' }, DOC_TEMPLATES.map(([label, sub, text]) =>
        h('button', { class: 'template', onclick: () => newDoc(text) }, h('b', { text: label }), h('small', { text: sub })))),
      set.recent.length ? h('h2', { text: 'Recent' }) : null,
      ...set.recent.slice(0, 8).map((p) => h('button', { class: 'recent-item', onclick: () => openPath(p) },
        h('span', { html: icon('file'), style: 'display:contents' }),
        h('div', {}, h('b', { text: pathx.basename(p) }), h('small', { text: pathx.dirname(p) })))),
      h('div', { class: 'drop-hint', text: 'Tip: drop any text file on this window to open it: Markdown, code, JSON, YAML, diagrams and more.' })));
  }
  async function installApp() {
    try {
      const r = await host.invoke('install');
      S.info.isDefault = r.isDefault;
      const choice = await dialog({
        title: 'Inkdown',
        message: r.message,
        buttons: r.isDefault
          ? [{ label: 'Done', value: null, primary: true }]
          : [{ label: 'Close', value: null }, { label: 'Open default apps', value: 'settings', primary: true }],
      });
      if (choice === 'settings') host.send('default_apps');
      if (!S.active) renderStart();
    } catch (e) {
      dialog({ title: 'Installation failed', message: String(e.message || e), buttons: [{ label: 'Close', value: null, primary: true }] });
    }
  }

  // ================================================================ close
  ink.requestClose = async () => {
    if ($('.scrim')) return;
    flushRich();
    const dirty = S.docs.filter((d) => d.dirty);
    if (dirty.length) {
      const r = await dialog({
        title: 'Save changes before closing?',
        message: dirty.length === 1 ? `"${dirty[0].name}" has unsaved changes.` : `${dirty.length} documents have unsaved changes:\n${dirty.map((d) => '• ' + d.name).join('\n')}`,
        buttons: [{ label: 'Cancel', value: null }, { label: "Don't save", value: 'discard', danger: true }, { label: dirty.length > 1 ? 'Save all' : 'Save', value: 'save', primary: true }],
      });
      if (!r) return;
      if (r === 'save') for (const d of dirty) { if (!(await save(d))) return; }
    }
    persist.flush();
    host.send('close');
  };

  // ================================================================= boot
  for (const b of $$('#mode-switch button')) b.addEventListener('click', () => setMode(b.dataset.mode));

  async function boot() {
    hydrateIcons();
    $('#btn-sidebar').innerHTML = icon('sidebar');
    $('#btn-new').innerHTML = icon('plus');
    $('#btn-new').dataset.tip = 'New document (Ctrl+N). Right-click for templates';
    buildToolbar();
    let init = { settings: null, docs: [] };
    try { init = await host.invoke('ready'); } catch (e) { fail(e); }
    if (init.settings && typeof init.settings === 'object') {
      for (const k of Object.keys(set)) if (typeof init.settings[k] === typeof set[k]) set[k] = init.settings[k];
      if (!Array.isArray(set.recent)) set.recent = [];
    }
    if (init.sep) pathx.sep = init.sep;
    Object.assign(S.info, { version: init.version, installed: init.installed || init.packaged, isDefault: init.isDefault || init.packaged });
    applyTheme();
    applyAppearance();
    applySplit();
    setSidebar(!!set.sidebar);
    setPanel(set.sidebarPanel === 'files' ? 'files' : 'outline');
    setMode(['read', 'write', 'split', 'edit'].includes(set.mode) ? set.mode : 'read', { focus: false });
    let shown = false;
    const show = () => { if (!shown) { shown = true; performance.mark('ink-show'); host.send('show'); } };
    setTimeout(show, 1200);
    if (init.docs.length && openDocs(init.docs)) {
      if (mode() === 'write' || !previewVisible()) setTimeout(show, mode() === 'write' ? 350 : 60);
      else {
        const prev = Preview.onUpdate;
        Preview.onUpdate = () => { Preview.onUpdate = prev; prev(); requestAnimationFrame(show); };
      }
    } else {
      showEmpty();
      requestAnimationFrame(show);
    }
  }
  boot();
})();
