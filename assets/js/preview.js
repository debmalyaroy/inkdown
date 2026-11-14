'use strict';
/* Preview: sanitise + reconcile rendered HTML, code highlighting, math,
   diagrams (Mermaid, Graphviz, ECharts, inline SVG), lightbox, find-in-page. */

window.ink = {};
const isDark = () => document.documentElement.dataset.theme === 'dark';
const UI_FONT = '"Segoe UI Variable Text","Segoe UI",system-ui,sans-serif';

function buildSearchRegex(query, { matchCase, wholeWord, regex }, flags = 'g') {
  if (!query) return null;
  let src = regex ? query : escapeRegex(query);
  if (wholeWord) src = `\\b(?:${src})\\b`;
  try {
    return new RegExp(src, flags + (matchCase ? '' : 'i') + 'm');
  } catch { return null; }
}

// ------------------------------------------------------------ lightbox
const Lightbox = (() => {
  let box = null;
  function close() {
    if (!box) return;
    box.el.remove();
    document.removeEventListener('keydown', box.onKey, true);
    box = null;
  }
  /** node: <img> or <svg> already in the document. */
  function open(node, { onSave } = {}) {
    close();
    const r = node.getBoundingClientRect();
    const baseW = node.tagName === 'IMG' ? node.naturalWidth || r.width : r.width;
    const baseH = node.tagName === 'IMG' ? node.naturalHeight || r.height : r.height;
    if (!baseW || !baseH) return;
    const content = node.cloneNode(true);
    content.removeAttribute('data-zoomable');
    content.style.cssText = 'max-width:none;cursor:inherit;margin:0;border-radius:0';
    const stage = h('div', { class: 'lightbox-stage' }, content);
    const pct = h('span');
    const btn = (ic, tip, fn) => h('button', { class: 'icon-btn', 'data-tip': tip, html: icon(ic), onclick: (e) => { e.stopPropagation(); fn(); } });
    const bar = h('div', { class: 'lightbox-bar', onmousedown: (e) => e.stopPropagation(), ondblclick: (e) => e.stopPropagation() },
      btn('zoomOut', 'Zoom out', () => zoomBy(1 / 1.3)), pct, btn('zoomIn', 'Zoom in', () => zoomBy(1.3)),
      btn('reset', 'Fit to window', fit),
      onSave ? h('div', { class: 'sep' }) : null,
      onSave ? btn('download', 'Save image', onSave) : null,
      h('div', { class: 'sep' }), btn('x', 'Close (Esc)', close));
    const el = h('div', { class: 'lightbox' }, stage, bar);
    let scale = 1, x = 0, y = 0;
    const paint = () => {
      content.style.width = baseW * scale + 'px';
      content.style.height = baseH * scale + 'px';
      stage.style.transform = `translate(${x}px,${y}px)`;
      pct.textContent = Math.round(scale * 100) + '%';
    };
    function fit() {
      const vw = innerWidth - 80, vh = innerHeight - 140;
      const cap = node.tagName === 'IMG' ? 1 : 4;
      scale = Math.min(vw / baseW, vh / baseH, cap);
      x = (innerWidth - baseW * scale) / 2;
      y = (innerHeight - 60 - baseH * scale) / 2;
      paint();
    }
    function zoomAt(f, cx, cy) {
      const next = clamp(scale * f, 0.05, 40);
      x = cx - (cx - x) * (next / scale);
      y = cy - (cy - y) * (next / scale);
      scale = next;
      paint();
    }
    const zoomBy = (f) => zoomAt(f, innerWidth / 2, innerHeight / 2);
    el.addEventListener('wheel', (e) => { e.preventDefault(); zoomAt(Math.exp(-e.deltaY * 0.0016), e.clientX, e.clientY); }, { passive: false });
    el.addEventListener('mousedown', (e) => {
      if (e.button !== 0) return;
      const sx = e.clientX - x, sy = e.clientY - y;
      let moved = false;
      el.classList.add('grabbing');
      const move = (ev) => { moved = true; x = ev.clientX - sx; y = ev.clientY - sy; paint(); };
      const up = (ev) => {
        el.classList.remove('grabbing');
        removeEventListener('mousemove', move);
        removeEventListener('mouseup', up);
        if (!moved && ev.target === el) close();
      };
      addEventListener('mousemove', move);
      addEventListener('mouseup', up);
    });
    el.addEventListener('dblclick', fit);
    const onKey = (e) => {
      if (e.key === 'Escape') close();
      else if (e.key === '+' || e.key === '=') zoomBy(1.3);
      else if (e.key === '-') zoomBy(1 / 1.3);
      else if (e.key === '0') fit();
      else return;
      e.preventDefault(); e.stopPropagation();
    };
    document.addEventListener('keydown', onKey, true);
    $('#overlays').append(el);
    box = { el, onKey };
    fit();
  }
  return { open, close, get isOpen() { return !!box; } };
})();

// --------------------------------------------------- find in rendered DOM
/** Text search over a rendered element, painted with the CSS Highlight API. */
function makeDomFinder(root, pane) {
  // ---- find in preview
  const finder = { ranges: [], index: -1 };
  function clearFind() {
    finder.ranges = []; finder.index = -1;
    CSS.highlights?.delete('ink-find'); CSS.highlights?.delete('ink-find-current');
  }
  function find(query, opts) {
    clearFind();
    const re = buildSearchRegex(query, opts);
    if (!re || !window.Highlight) return 0;
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
      acceptNode: (n) => n.parentElement.closest('.katex-mathml, .block-tools, .codeblock-head, .diagram-kind, .anchor')
        ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
    });
    for (let n; (n = walker.nextNode());) {
      re.lastIndex = 0;
      for (let m; (m = re.exec(n.nodeValue));) {
        if (!m[0].length) { re.lastIndex++; continue; }
        const r = new Range();
        r.setStart(n, m.index); r.setEnd(n, m.index + m[0].length);
        finder.ranges.push(r);
        if (finder.ranges.length >= 5000) break;
      }
    }
    if (finder.ranges.length) CSS.highlights.set('ink-find', new Highlight(...finder.ranges));
    return finder.ranges.length;
  }
  function findStep(dir) {
    const n = finder.ranges.length;
    if (!n) return -1;
    if (finder.index < 0) {
      const top = pane.getBoundingClientRect().top;
      const first = finder.ranges.findIndex((r) => r.getBoundingClientRect().top >= top);
      finder.index = dir > 0 ? (first < 0 ? 0 : first) : ((first < 0 ? n : first) - 1 + n) % n;
    } else finder.index = (finder.index + dir + n) % n;
    const r = finder.ranges[finder.index];
    CSS.highlights.set('ink-find-current', new Highlight(r));
    const rect = r.getBoundingClientRect(), pr = pane.getBoundingClientRect();
    if (rect.top < pr.top + 60 || rect.bottom > pr.bottom - 60) pane.scrollTop += rect.top - pr.top - pane.clientHeight / 3;
    return finder.index;
  }
  return { find, findStep, clearFind, get findCount() { return finder.ranges.length; } };
}

// ------------------------------------------------------------- preview
const Preview = (() => {
  const root = $('#preview');
  const pane = $('#preview-pane');
  let baseDir = '';
  let renderSeq = 0;
  let map = null;
  let renderedFor = null; // {doc, gen}
  const svgCache = new Map();
  const charts = new Set();

  // ---- sanitising
  function resolveMedia(src) {
    if (/^(https?:|data:|blob:)/i.test(src)) return src;
    let p = src.split(/[?#]/)[0];
    try { p = decodeURIComponent(p); } catch { /* keep raw */ }
    if (/^file:\/\//i.test(p)) p = pathx.fromFileUrl(p);
    if (pathx.isAbs(p)) return pathx.fileUrl(pathx.norm(p));
    if (!baseDir) return src;
    return pathx.fileUrl(pathx.join(baseDir, p));
  }
  DOMPurify.addHook('afterSanitizeAttributes', (node) => {
    if (node.nodeType !== 1) return;
    const tag = node.tagName;
    if ((tag === 'IMG' || tag === 'VIDEO' || tag === 'AUDIO' || tag === 'SOURCE') && node.hasAttribute('src')) {
      const src = node.getAttribute('src');
      node.setAttribute('data-src', src);
      node.setAttribute('src', resolveMedia(src));
    }
  });
  const PURIFY = { RETURN_DOM_FRAGMENT: true, FORBID_TAGS: ['style', 'form', 'link', 'meta', 'base', 'title', 'dialog'], ADD_ATTR: ['target'] };
  const sanitizeSvg = (svg) => DOMPurify.sanitize(svg, { USE_PROFILES: { svg: true, svgFilters: true } });

  // ---- sourcepos helpers
  const startLine = (el) => parseInt(el.getAttribute?.('data-sourcepos') || '0', 10) || 0;
  function shiftLines(el, d) {
    const fix = (n) => {
      const m = /^(\d+):(\d+)-(\d+):(\d+)$/.exec(n.getAttribute('data-sourcepos'));
      if (m) n.setAttribute('data-sourcepos', `${+m[1] + d}:${m[2]}-${+m[3] + d}:${m[4]}`);
    };
    if (el.hasAttribute('data-sourcepos')) fix(el);
    el.querySelectorAll('[data-sourcepos]').forEach(fix);
  }
  /** Replace `oldEl` with `newEl`, keeping the reconcile key and source position. */
  function swap(oldEl, newEl) {
    if (oldEl._key !== undefined) newEl._key = oldEl._key;
    const sp = oldEl.getAttribute('data-sourcepos');
    if (sp && !newEl.hasAttribute('data-sourcepos')) newEl.setAttribute('data-sourcepos', sp);
    oldEl.replaceWith(newEl);
    return newEl;
  }
  function wrap(el, wrapper) {
    swap(el, wrapper);
    wrapper.append(el);
    return wrapper;
  }

  // ---- enhancers
  function renderMath(el) {
    const display = el.getAttribute('data-math-style') === 'display';
    const tex = el.textContent;
    const out = h(display ? 'div' : 'span', { class: display ? 'math-display' : 'math-inline' });
    const paint = () => {
      try {
        katex.render(tex, out, { displayMode: display, throwOnError: false, strict: 'ignore', trust: false, output: 'htmlAndMathml' });
      } catch (e) {
        out.className = 'math-error';
        out.textContent = String(e.message || e);
      }
      map = null;
    };
    if (window.katex) paint();
    else { out.textContent = tex; loadKatex().then(paint, () => {}); }
    out.dataset.tex = tex;
    const target = el.parentElement?.tagName === 'PRE' ? el.parentElement : el;
    if (target !== el) { const sp = el.getAttribute('data-sourcepos'); if (sp && !target.hasAttribute('data-sourcepos')) target.setAttribute('data-sourcepos', sp); }
    swap(target, out);
  }

  const DIAGRAM_LANGS = { mermaid: 'mermaid', mmd: 'mermaid', dot: 'graphviz', graphviz: 'graphviz', echarts: 'chart', chart: 'chart', svg: 'svg' };
  const KIND_LABEL = { mermaid: 'Mermaid', graphviz: 'Graphviz', chart: 'Chart', svg: 'SVG' };
  const toolBtn = (act, ic, tip) => h('button', { 'data-act': act, 'data-tip': tip, html: icon(ic) });

  function enhanceCode(pre, code, stale) {
    const lang = (/(?:^|\s)language-(\S+)/.exec(code.className) || [, ''])[1].toLowerCase();
    const kind = DIAGRAM_LANGS[lang];
    if (kind) {
      const box = h('div', { class: 'diagram', dataset: { kind } },
        h('span', { class: 'diagram-kind', text: KIND_LABEL[kind] }),
        h('div', { class: 'block-tools' },
          kind === 'svg' && Designer.isEditable(code.textContent) ? toolBtn('design', 'edit', 'Edit in the visual designer') : null,
          kind !== 'chart' ? toolBtn('zoom', 'expand', 'Open full view') : null,
          toolBtn('copy-src', 'copy', 'Copy source'),
          toolBtn('save-img', 'download', 'Save as image')),
        h('div', { class: 'diagram-stage' }));
      box._src = code.textContent.replace(/\n$/, '');
      box._kind = kind;
      const old = stale.shift();
      if (old && old.kind === kind && kind !== 'chart') {
        box.querySelector('.diagram-stage').innerHTML = old.html;
        box._stale = true;
      }
      swap(pre, box);
      drawDiagram(box);
      return;
    }
    const label = h('span', { text: lang || 'text' });
    const paint = () => {
      const def = hljs.getLanguage(lang);
      if (!def) return;
      label.textContent = (def.name || lang).split(',')[0];
      if (code.textContent.length > 300000) return; // keep huge listings instant
      try {
        code.innerHTML = hljs.highlight(code.textContent, { language: lang, ignoreIllegals: true }).value;
        code.classList.add('hljs');
      } catch { /* leave plain */ }
    };
    if (lang && hljs.getLanguage(lang)) paint();
    else if (lang) loadLanguage(lang).then((ok) => ok && paint());
    const wrapper = h('div', { class: 'codeblock' },
      h('div', { class: 'codeblock-head' }, label,
        h('div', { class: 'block-tools' }, toolBtn('copy-code', 'copy', 'Copy code'))));
    wrap(pre, wrapper);
  }

  // highlight.js grammars beyond the built-in common set are fetched on demand
  let aliasMap = null;
  async function loadLanguage(lang) {
    try {
      aliasMap ??= await fetch('vendor/hljs/aliases.json').then((r) => r.json());
      const file = aliasMap[lang];
      if (!file) return false;
      await loadScript(`vendor/hljs/languages/${file}.min.js`);
      return !!hljs.getLanguage(lang);
    } catch { return false; }
  }

  function enhance(top, stale) {
    const all = (sel) => (top.matches(sel) ? [top] : []).concat(Array.from(top.querySelectorAll(sel)));
    for (const pre of all('pre')) {
      const code = pre.querySelector(':scope > code');
      if (code && !code.hasAttribute('data-math-style')) enhanceCode(pre, code, stale);
    }
    for (const m of all('[data-math-style]')) renderMath(m);
    for (const t of all('table')) if (!t.closest('.frontmatter')) wrap(t, h('div', { class: 'table-wrap' }));
    for (const img of all('img')) {
      img.setAttribute('data-zoomable', '');
      img.addEventListener('error', () => {
        img.classList.add('img-error');
        img.removeAttribute('data-zoomable');
        if (!img.alt) img.alt = 'Image not found: ' + (img.dataset.src || '');
      }, { once: true });
    }
    for (const cb of all('li input[type="checkbox"], td input[type="checkbox"]')) cb.removeAttribute('disabled');
    for (const a of all('a[href]')) {
      const href = a.getAttribute('href');
      if (/^https?:/i.test(href) && !a.title) a.title = href;
    }
  }

  // ---- diagrams
  let mermaidTheme = null, mermaidSeq = 0, vizInstance = null;
  const MERMAID_VARS = {
    light: {
      background: '#ffffff', primaryColor: '#eef0ff', primaryBorderColor: '#8b8ff5', primaryTextColor: '#1f2328',
      secondaryColor: '#f5ebff', secondaryBorderColor: '#c39bf0', tertiaryColor: '#f1f8f4', tertiaryBorderColor: '#8cc9a4',
      lineColor: '#6b7280', textColor: '#1f2328', mainBkg: '#eef0ff', nodeBorder: '#8b8ff5',
      clusterBkg: '#f7f8fa', clusterBorder: '#d9dde3', edgeLabelBackground: '#ffffff', titleColor: '#1f2328',
      noteBkgColor: '#fff8db', noteBorderColor: '#e3c96b', noteTextColor: '#1f2328',
      actorBkg: '#eef0ff', actorBorder: '#8b8ff5', activationBkgColor: '#e3e5ff', activationBorderColor: '#8b8ff5',
      pie1: '#6366f1', pie2: '#a855f7', pie3: '#ec4899', pie4: '#f59e0b', pie5: '#10b981', pie6: '#06b6d4', pie7: '#3b82f6', pie8: '#84cc16',
      pieOpacity: '1', pieStrokeColor: '#ffffff', pieOuterStrokeColor: '#d9dde3', pieTitleTextColor: '#1f2328', pieSectionTextColor: '#ffffff', pieLegendTextColor: '#1f2328',
      fontSize: '14px',
    },
    dark: {
      darkMode: true, background: '#16181d', primaryColor: '#2b2e57', primaryBorderColor: '#8183f4', primaryTextColor: '#e6e8ec',
      secondaryColor: '#3d2b59', secondaryBorderColor: '#a57be0', tertiaryColor: '#1f3a2e', tertiaryBorderColor: '#4f9d74',
      lineColor: '#9aa3af', textColor: '#e6e8ec', mainBkg: '#2b2e57', nodeBorder: '#8183f4',
      clusterBkg: '#1c1f26', clusterBorder: '#3a404b', edgeLabelBackground: '#16181d', titleColor: '#e6e8ec',
      noteBkgColor: '#3f3820', noteBorderColor: '#8c7c3b', noteTextColor: '#e6e8ec',
      actorBkg: '#2b2e57', actorBorder: '#8183f4', activationBkgColor: '#373b72', activationBorderColor: '#8183f4',
      pie1: '#818cf8', pie2: '#c084fc', pie3: '#f472b6', pie4: '#fbbf24', pie5: '#34d399', pie6: '#22d3ee', pie7: '#60a5fa', pie8: '#a3e635',
      pieOpacity: '1', pieStrokeColor: '#16181d', pieOuterStrokeColor: '#3a404b', pieTitleTextColor: '#e6e8ec', pieSectionTextColor: '#14151a', pieLegendTextColor: '#e6e8ec',
      fontSize: '14px',
    },
  };
  const RENDERERS = {
    async mermaid(src, theme) {
      await loadScript('vendor/mermaid.min.js');
      if (mermaidTheme !== theme) {
        mermaid.initialize({
          startOnLoad: false, securityLevel: 'strict', theme: 'base', themeVariables: MERMAID_VARS[theme],
          fontFamily: UI_FONT, suppressErrorRendering: true,
          flowchart: { curve: 'basis', padding: 14 }, sequence: { mirrorActors: false },
          gantt: { useWidth: 860, barHeight: 24, fontSize: 13, sectionFontSize: 13 },
        });
        mermaidTheme = theme;
      }
      const id = 'mmd' + (++mermaidSeq);
      try {
        return (await mermaid.render(id, src)).svg;
      } finally {
        document.getElementById(id)?.remove();
        document.getElementById('d' + id)?.remove();
      }
    },
    async graphviz(src, theme) {
      await loadScript('vendor/viz-global.js');
      vizInstance ??= await Viz.instance();
      const fg = theme === 'dark' ? '#d5d9e0' : '#2b3038';
      const svg = vizInstance.renderString(src, {
        format: 'svg',
        graphAttributes: { bgcolor: 'transparent', fontname: 'Segoe UI', fontcolor: fg, fontsize: 12 },
        nodeAttributes: { color: fg, fontcolor: fg, fontname: 'Segoe UI', fontsize: 12 },
        edgeAttributes: { color: fg, fontcolor: fg, fontname: 'Segoe UI', fontsize: 11 },
      });
      return sanitizeSvg(svg.slice(svg.indexOf('<svg')));
    },
    async svg(src) {
      const clean = sanitizeSvg(src);
      if (!/<svg[\s>]/i.test(clean)) throw new Error('No <svg> element found.');
      return clean;
    },
  };
  const CHART_COLORS = ['#6366f1', '#a855f7', '#ec4899', '#f59e0b', '#10b981', '#06b6d4', '#3b82f6', '#84cc16'];

  function setError(box, err) {
    box.querySelector('.diagram-error')?.remove();
    if (!err) return;
    const msg = String(err && err.message || err).replace(/^Error: /, '');
    box.append(h('pre', { class: 'diagram-error' }, h('b', { text: KIND_LABEL[box._kind] + ' could not be rendered' }), msg));
  }

  async function drawChart(box, stage) {
    await loadScript('vendor/echarts.min.js');
    const option = JSON.parse(box._src);
    if (!option || typeof option !== 'object' || Array.isArray(option)) throw new Error('Chart definition must be a JSON object (an ECharts option).');
    box._chart?.dispose();
    stage.replaceChildren();
    stage.style.height = clamp(Number(option.height) || 380, 120, 1600) + 'px';
    const chart = echarts.init(stage, isDark() ? 'dark' : null, { renderer: 'svg' });
    chart.setOption({
      backgroundColor: 'transparent', color: CHART_COLORS, textStyle: { fontFamily: UI_FONT },
      tooltip: { trigger: option.xAxis || option.yAxis ? 'axis' : 'item' },
      ...option,
    });
    box._chart = chart;
    charts.add(box);
  }

  async function drawDiagram(box) {
    const stage = box.querySelector('.diagram-stage');
    const theme = isDark() ? 'dark' : 'light';
    const src = box._src, kind = box._kind;
    try {
      if (kind === 'chart') {
        await drawChart(box, stage);
      } else {
        const key = `${kind}|${theme}|${src}`;
        let svg = svgCache.get(key);
        if (svg === undefined) {
          if (!stage.firstChild) stage.append(h('span', { class: 'diagram-loading', text: 'Rendering…' }));
          svg = await RENDERERS[kind](src, theme);
          if (svgCache.size > 300) svgCache.clear();
          svgCache.set(key, svg);
        }
        if (box._src !== src) return;
        stage.innerHTML = svg;
        stage.style.opacity = '';
        box._stale = false;
      }
      setError(box, null);
    } catch (e) {
      if (box._stale) stage.style.opacity = '.45';
      else stage.replaceChildren();
      setError(box, e);
    }
    map = null;
    Preview.onLayout?.();
  }

  /** Draw a diagram/chart into any element (used by the rich editor). */
  async function renderGraphic(stage, kind, src) {
    const theme = isDark() ? 'dark' : 'light';
    if (kind === 'chart') {
      await loadScript('vendor/echarts.min.js');
      const option = JSON.parse(src);
      if (!option || typeof option !== 'object' || Array.isArray(option)) throw new Error('Chart definition must be a JSON object.');
      stage._chart?.dispose();
      stage.replaceChildren();
      stage.style.height = clamp(Number(option.height) || 380, 120, 1600) + 'px';
      const chart = echarts.init(stage, theme === 'dark' ? 'dark' : null, { renderer: 'svg' });
      chart.setOption({
        backgroundColor: 'transparent', color: CHART_COLORS, textStyle: { fontFamily: UI_FONT },
        tooltip: { trigger: option.xAxis || option.yAxis ? 'axis' : 'item' }, ...option,
      });
      stage._chart = chart;
      return;
    }
    const key = `${kind}|${theme}|${src}`;
    let svg = svgCache.get(key);
    if (svg === undefined) {
      svg = await RENDERERS[kind](src, theme);
      svgCache.set(key, svg);
    }
    stage._chart?.dispose();
    stage._chart = null;
    stage.style.height = '';
    stage.innerHTML = svg;
  }

  function retheme() {
    mermaidTheme = null;
    return Promise.all(Array.from(root.querySelectorAll('.diagram'), drawDiagram));
  }
  function sweepCharts() {
    for (const box of charts) {
      if (!box.isConnected) { box._chart?.dispose(); charts.delete(box); }
    }
  }
  new ResizeObserver(debounce(() => { for (const b of charts) b._chart?.resize(); map = null; }, 80)).observe(root);

  // ---- front matter
  function frontMatterCard(raw) {
    const lines = raw.replace(/\r/g, '').trim().split('\n');
    const body = lines.slice(1, lines[lines.length - 1].trim() === '---' || lines[lines.length - 1].trim() === '...' ? -1 : undefined);
    const rows = [];
    let simple = true;
    for (const line of body) {
      if (!line.trim() || /^\s*#/.test(line)) continue;
      const m = /^([\w.-]+):\s*(.*)$/.exec(line);
      if (m) rows.push([m[1], m[2].replace(/^(["'])(.*)\1$/, '$2')]);
      else if (rows.length && /^\s+/.test(line)) {
        const item = line.trim().replace(/^-\s*/, '');
        rows[rows.length - 1][1] += (rows[rows.length - 1][1] ? ', ' : '') + item;
      } else { simple = false; break; }
    }
    const card = h('div', { class: 'frontmatter', 'data-sourcepos': `1:1-${lines.length}:3` });
    if (simple && rows.length) {
      card.append(h('table', {}, h('tbody', {}, rows.map(([k, v]) =>
        h('tr', {}, h('td', { text: k }), h('td', { text: v.replace(/^\[(.*)\]$/, '$1') }))))));
    } else {
      card.append(h('pre', { text: body.join('\n') }));
    }
    card._key = 'fm:' + raw;
    return card;
  }

  // ---- reconcile
  const keyOf = (el) => el._key ?? el.outerHTML.replace(/ data-sourcepos="[^"]*"/g, '');

  function apply(html, frontMatter, doc) {
    baseDir = doc.dir || '';
    const frag = DOMPurify.sanitize(html, PURIFY);
    const incoming = Array.from(frag.children);
    if (frontMatter) incoming.unshift(frontMatterCard(frontMatter));
    const fresh = incoming.map((el) => ({ el, key: keyOf(el) }));
    const old = Array.from(root.children);

    let s = 0;
    while (s < old.length && s < fresh.length && old[s]._key === fresh[s].key) s++;
    let e = 0;
    while (e < old.length - s && e < fresh.length - s && old[old.length - 1 - e]._key === fresh[fresh.length - 1 - e].key) e++;

    for (let i = 0; i < e; i++) {
      const o = old[old.length - 1 - i], n = fresh[fresh.length - 1 - i].el;
      const d = startLine(n) - startLine(o);
      if (d) shiftLines(o, d);
    }
    const anchor = e ? old[old.length - e] : null;
    const stale = [];
    for (let i = s; i < old.length - e; i++) {
      for (const box of old[i].matches('.diagram') ? [old[i]] : old[i].querySelectorAll('.diagram')) {
        if (!box.querySelector('.diagram-error') || box._stale) stale.push({ kind: box._kind, html: box.querySelector('.diagram-stage').innerHTML });
      }
      old[i].remove();
    }
    const added = [];
    for (let i = s; i < fresh.length - e; i++) {
      const { el, key } = fresh[i];
      el._key = key;
      root.insertBefore(el, anchor);
      added.push(el);
    }
    for (const el of added) enhance(el, stale);
    sweepCharts();
    map = null;
  }

  async function render(doc, text) {
    const mine = ++renderSeq;
    const gen = doc.gen;
    const markdown = !doc.kind || doc.kind.id === 'markdown';
    let res;
    try {
      res = markdown ? await host.invoke('render', { text }) : await Kinds.produce(doc, text);
    } catch (e) { fail(e); return false; }
    if (mine !== renderSeq) return false;
    if (renderedFor?.doc !== doc) { root.replaceChildren(); sweepCharts(); }
    root.dataset.kind = markdown ? 'markdown' : doc.kind.diagram ? 'diagram' : doc.kind.data ? 'data' : doc.kind.id;
    apply(res.html, res.frontMatter, doc);
    res.mount?.(root);
    renderedFor = { doc, gen };
    Preview.onProduced?.(doc, res);
    Preview.onUpdate?.();
    return true;
  }
  function clear() {
    renderSeq++;
    root.replaceChildren();
    sweepCharts();
    renderedFor = null;
    map = null;
  }
  const isCurrent = (doc) => renderedFor && renderedFor.doc === doc && renderedFor.gen === doc.gen;

  // ---- scroll mapping
  const topOf = (el) => el.getBoundingClientRect().top - pane.getBoundingClientRect().top + pane.scrollTop;
  function lineMap() {
    if (!map) {
      map = [];
      let last = 0;
      for (const el of root.querySelectorAll('[data-sourcepos]')) {
        const line = startLine(el);
        if (line > last) { map.push({ line, el }); last = line; }
      }
    }
    return map;
  }
  /** Scroll so that source `line` (1-based, may be fractional) sits at the top. */
  function scrollToLine(line, offset = 0) {
    const m = lineMap();
    if (!m.length) return;
    let lo = 0, hi = m.length - 1;
    while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (m[mid].line <= line) lo = mid; else hi = mid - 1; }
    const a = m[lo], b = m[lo + 1];
    if (line < a.line) { pane.scrollTop = 0; return; }
    const ya = topOf(a.el);
    let y = ya;
    if (b) y = ya + (topOf(b.el) - ya) * clamp((line - a.line) / (b.line - a.line), 0, 1);
    else {
      const end = /-(\d+):/.exec(a.el.getAttribute('data-sourcepos'));
      const span = Math.max(1, (end ? +end[1] : a.line) - a.line + 1);
      y = ya + a.el.getBoundingClientRect().height * clamp((line - a.line) / span, 0, 1);
    }
    pane.scrollTop = y - offset;
  }
  /** Fractional source line currently at the top of the preview. */
  function lineAtTop() {
    const m = lineMap();
    if (!m.length) return 1;
    const y = pane.scrollTop + 1;
    let lo = 0, hi = m.length - 1;
    while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (topOf(m[mid].el) <= y) lo = mid; else hi = mid - 1; }
    const a = m[lo], b = m[lo + 1];
    const ya = topOf(a.el);
    if (y < ya) return 1;
    if (!b) return a.line;
    const yb = topOf(b.el);
    return a.line + (b.line - a.line) * (yb > ya ? clamp((y - ya) / (yb - ya), 0, 1) : 0);
  }
  const lineOf = (node) => {
    const el = node.nodeType === 1 ? node : node.parentElement;
    const hit = el?.closest('[data-sourcepos]');
    return hit ? startLine(hit) : 0;
  };

  function headings() {
    return Array.from(root.querySelectorAll('h1,h2,h3,h4,h5,h6')).map((el) => ({
      el, level: +el.tagName[1], text: el.textContent.trim(), line: startLine(el),
    }));
  }

  const dom = makeDomFinder(root, pane);
  const { find, findStep, clearFind } = dom;

  // ---- image export
  async function svgToPngBase64(svgEl, scale = 2) {
    const r = svgEl.getBoundingClientRect();
    const clone = svgEl.cloneNode(true);
    clone.setAttribute('width', r.width); clone.setAttribute('height', r.height);
    clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
    clone.style.maxWidth = 'none';
    const img = new Image();
    img.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(new XMLSerializer().serializeToString(clone));
    await img.decode();
    const canvas = h('canvas', { width: Math.ceil(r.width * scale), height: Math.ceil(r.height * scale) });
    const ctx = canvas.getContext('2d');
    ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue('--doc-bg').trim() || '#fff';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
    return canvas.toDataURL('image/png').split(',')[1];
  }
  function saveImageMenu(box, anchor) {
    const svgEl = box.querySelector('.diagram-stage svg');
    if (!svgEl) return toast('Nothing to save yet');
    const dir = ink.activeDoc?.()?.dir || '';
    const name = (KIND_LABEL[box._kind] || 'diagram').toLowerCase();
    const saveSvg = async () => {
      const path = await host.invoke('save_dialog', { name: name + '.svg', kind: 'svg', dir });
      if (!path) return;
      const clone = svgEl.cloneNode(true);
      clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
      await host.invoke('save', { path, text: new XMLSerializer().serializeToString(clone), crlf: false });
      toast('Saved ' + pathx.basename(path));
    };
    const savePng = async () => {
      const path = await host.invoke('save_dialog', { name: name + '.png', kind: 'png', dir });
      if (!path) return;
      const data = await svgToPngBase64(svgEl);
      await host.invoke('write_binary', { path, data });
      toast('Saved ' + pathx.basename(path));
    };
    showMenu(anchor, [
      { label: 'Save as SVG (vector)', icon: 'download', run: () => saveSvg().catch(fail) },
      { label: 'Save as PNG', icon: 'image', run: () => savePng().catch(() => saveSvg().then(() => toast('PNG is not available for this diagram; saved SVG instead')).catch(fail)) },
    ], { align: 'right' });
  }

  // ---- interaction
  const flash = (btn) => {
    const prev = btn.innerHTML;
    btn.innerHTML = icon('check');
    setTimeout(() => { btn.innerHTML = prev; }, 1100);
  };
  root.addEventListener('click', (e) => {
    const actBtn = e.target.closest('[data-act]');
    if (actBtn) {
      const act = actBtn.dataset.act;
      const box = actBtn.closest('.diagram');
      if (act === 'copy-code') {
        navigator.clipboard.writeText(actBtn.closest('.codeblock').querySelector('pre').textContent.replace(/\n$/, '')).then(() => flash(actBtn), fail);
      } else if (act === 'copy-src') {
        navigator.clipboard.writeText(box._src).then(() => flash(actBtn), fail);
      } else if (act === 'zoom') {
        const svg = box.querySelector('.diagram-stage svg');
        if (svg) Lightbox.open(svg, { onSave: () => saveImageMenu(box, $('.lightbox-bar')) });
      } else if (act === 'save-img') saveImageMenu(box, actBtn);
      else if (act === 'design') ink.editDrawing?.(box._src);
      return;
    }
    const a = e.target.closest('a[href]');
    if (a && root.contains(a)) {
      e.preventDefault();
      ink.followLink?.(a.getAttribute('href'), a);
      return;
    }
    const img = e.target.closest('img[data-zoomable]');
    if (img) return Lightbox.open(img);
    const svg = e.target.closest('.diagram-stage > svg');
    if (svg && !window.getSelection().toString()) {
      const box = svg.closest('.diagram');
      if (box._kind !== 'chart') Lightbox.open(svg, { onSave: () => saveImageMenu(box, $('.lightbox-bar')) });
    }
  });
  root.addEventListener('change', (e) => {
    if (e.target.matches('input[type="checkbox"]')) {
      const line = lineOf(e.target);
      if (line) ink.toggleTask?.(line, e.target.checked);
    }
  });
  root.addEventListener('dblclick', (e) => {
    if (e.target.closest('.diagram, a, input, .block-tools')) return;
    const line = lineOf(e.target);
    if (line) ink.jumpToSource?.(line);
  });

  // ---- export
  async function exportHtml(title) {
    const clone = root.cloneNode(true);
    clone.querySelectorAll('.block-tools, .diagram-kind, .diagram-error, .diagram-loading').forEach((n) => n.remove());
    clone.querySelectorAll('[data-src]').forEach((n) => { n.setAttribute('src', n.dataset.src); n.removeAttribute('data-src'); });
    clone.querySelectorAll('[data-sourcepos]').forEach((n) => n.removeAttribute('data-sourcepos'));
    clone.querySelectorAll('[data-zoomable]').forEach((n) => n.removeAttribute('data-zoomable'));
    const live = root.querySelectorAll('input[type="checkbox"]');
    clone.querySelectorAll('input[type="checkbox"]').forEach((n, i) => {
      n.toggleAttribute('checked', !!live[i]?.checked);
      n.setAttribute('disabled', '');
    });
    const get = (u) => fetch(u).then((r) => r.text());
    const [docCss, katexCss, hlCss] = await Promise.all([
      get('doc.css'), get('vendor/katex/katex.min.css'), get(isDark() ? 'vendor/hljs/dark.css' : 'vendor/hljs/light.css'),
    ]);
    const fonts = `https://cdn.jsdelivr.net/npm/katex@${window.katex ? katex.version : "0.16.22"}/dist/fonts/`;
    const st = document.documentElement.style;
    const vars = ['--doc-size', '--doc-width'].map((v) => st.getPropertyValue(v) && `${v}:${st.getPropertyValue(v)}`).filter(Boolean).join(';');
    return `<!doctype html>
<html lang="en" data-theme="${isDark() ? 'dark' : 'light'}" style="${vars}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="Inkdown">
<title>${escapeHtml(title)}</title>
<style>
${docCss}
${katexCss.replace(/url\(fonts\//g, 'url(' + fonts)}
${hlCss}
html { background: var(--doc-bg); }
body { margin: 0; }
.diagram-stage > svg { cursor: default; }
</style>
</head>
<body>
<article class="${root.className}">
${clone.innerHTML}
</article>
</body>
</html>
`;
  }

  return {
    root, pane, render, clear, isCurrent, retheme, headings, lineMap, scrollToLine, lineAtTop, topOf,
    find, findStep, clearFind, exportHtml, onUpdate: null, onProduced: null, onLayout: null,
    get findCount() { return dom.findCount; },
    resolveMedia, sanitizeSvg, RENDERERS, DIAGRAM_LANGS, KIND_LABEL, CHART_COLORS, svgToPngBase64, renderGraphic,
  };
})();
