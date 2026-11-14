'use strict';
/* Document kinds: what a file is (by name), which views it offers, how its
   preview is produced, and the structured-data tree / table viewers. */

const Kinds = (() => {
  const ALL = ['read', 'split', 'edit'];
  const DEFS = {
    markdown: { label: 'Markdown', modes: ['read', 'write', 'split', 'edit'], ext: MD_EXT },
    adoc: { label: 'AsciiDoc', modes: ALL, ext: ['adoc', 'asciidoc', 'asc'], mode: 'asciidoc-lite' },
    json: { label: 'JSON', modes: ALL, ext: ['json', 'geojson', 'webmanifest', 'ipynb'], data: 'json', mime: 'application/json', cmMode: 'javascript' },
    yaml: { label: 'YAML', modes: ALL, ext: ['yaml', 'yml'], data: 'yaml', mime: 'text/x-yaml', cmMode: 'yaml' },
    toml: { label: 'TOML', modes: ALL, ext: ['toml'], data: 'toml', mime: 'text/x-toml', cmMode: 'toml' },
    csv: { label: 'CSV', modes: ALL, ext: ['csv', 'tsv'] },
    mermaid: { label: 'Mermaid', modes: ALL, ext: ['mmd', 'mermaid'], diagram: 'mermaid', mode: 'mermaid-lite' },
    graphviz: { label: 'Graphviz', modes: ALL, ext: ['dot', 'gv'], diagram: 'dot', mode: 'dot-lite' },
    svg: { label: 'SVG', modes: ALL, ext: ['svg'], diagram: 'svg', mime: 'image/svg+xml', cmMode: 'xml' },
    html: { label: 'HTML', modes: ALL, ext: ['html', 'htm', 'xhtml'], mime: 'text/html', cmMode: 'htmlmixed' },
    code: { label: 'Text', modes: ['edit'], ext: [] },
  };
  for (const [id, d] of Object.entries(DEFS)) d.id = id;
  const BY_EXT = new Map();
  for (const d of Object.values(DEFS)) for (const e of d.ext) BY_EXT.set(e, d);
  const BASENAMES = { dockerfile: 'dockerfile', makefile: 'cmake', 'cmakelists.txt': 'cmake', gemfile: 'ruby', rakefile: 'ruby', jenkinsfile: 'groovy', vagrantfile: 'ruby' };

  /** Kind + editor mode for a file name. */
  const SHEBANG = [[/\b(ba|z|k|da)?sh\b|\bfish\b/, 'x.sh'], [/python/, 'x.py'], [/\bnode\b|\bdeno\b|\bbun\b/, 'x.js'], [/ruby/, 'x.rb'], [/perl/, 'x.pl'], [/php/, 'x.php'], [/lua/, 'x.lua'], [/pwsh|powershell/, 'x.ps1']];
  function of(name, text) {
    const k = byName(name);
    // scripts without a telling extension: trust the "#!" line
    if (k.id === 'code' && !k.cmMode && text && text.startsWith('#!')) {
      const first = text.slice(0, 120).split('\n')[0];
      const hit = SHEBANG.find(([re]) => re.test(first));
      if (hit) return { ...byName(hit[1]), lang: pathx.ext(hit[1]) };
    }
    return k;
  }
  function byName(name) {
    const ext = pathx.ext(name);
    const base = BY_EXT.get(ext) || (ext || !/\./.test(name) ? null : DEFS.markdown);
    if (base && base.id !== 'code') return { ...base, lang: ext };
    // any other text file: a code document in whatever language the name implies
    const info = CodeMirror.findModeByFileName(name) || CodeMirror.findModeByName(BASENAMES[name.toLowerCase()] || '') || null;
    const usable = info && info.mode && info.mode !== 'null';
    return { ...DEFS.code, label: usable ? info.name : ext ? ext.toUpperCase() : 'Text', mime: usable ? info.mime || (info.mimes && info.mimes[0]) : 'text/plain', cmMode: usable ? info.mode : null, lang: ext || name.toLowerCase() };
  }

  const fence = (lang, text) => `<pre><code class="language-${lang}">${escapeHtml(text)}</code></pre>`;

  // ------------------------------------------------------------- CSV
  function parseCsv(text, name) {
    const first = text.slice(0, 4000).split('\n')[0] || '';
    const delim = pathx.ext(name) === 'tsv' || (first.split('\t').length > first.split(',').length && first.includes('\t')) ? '\t'
      : first.split(';').length > first.split(',').length ? ';' : ',';
    const rows = [];
    let row = [], cell = '', quoted = false;
    for (let i = 0; i < text.length; i++) {
      const c = text[i];
      if (quoted) {
        if (c === '"') { if (text[i + 1] === '"') { cell += '"'; i++; } else quoted = false; } else cell += c;
      } else if (c === '"' && cell === '') quoted = true;
      else if (c === delim) { row.push(cell); cell = ''; }
      else if (c === '\n') { row.push(cell); rows.push(row); row = []; cell = ''; if (rows.length > 20000) break; }
      else if (c !== '\r') cell += c;
    }
    if (cell !== '' || row.length) { row.push(cell); rows.push(row); }
    return rows.filter((r) => r.length > 1 || r[0] !== '');
  }
  const MAX_ROWS = 2000;
  function csvHtml(text, name) {
    const rows = parseCsv(text, name);
    if (!rows.length) return '<p><em>Empty table.</em></p>';
    const cols = Math.max(...rows.slice(0, 500).map((r) => r.length));
    const body = rows.slice(1, MAX_ROWS + 1);
    const numeric = Array.from({ length: cols }, (_, i) => body.length > 0 && body.slice(0, 200).every((r) => !r[i] || /^[-+]?[\d.,]+(e[-+]?\d+)?%?$/i.test(r[i].trim())));
    const cells = (r, tag) => Array.from({ length: cols }, (_, i) => `<${tag}${numeric[i] ? ' align="right"' : ''}>${escapeHtml(r[i] ?? '')}</${tag}>`).join('');
    const note = rows.length - 1 > MAX_ROWS ? `<p><em>Showing the first ${MAX_ROWS.toLocaleString()} of ${(rows.length - 1).toLocaleString()} rows.</em></p>` : '';
    return `<p class="data-summary">${(rows.length - 1).toLocaleString()} rows × ${cols} columns</p><table class="data-table"><thead><tr>${cells(rows[0], 'th')}</tr></thead><tbody>${body.map((r) => `<tr>${cells(r, 'td')}</tr>`).join('')}</tbody></table>${note}`;
  }
  function csvMarkdown(text, name) {
    const rows = parseCsv(text, name).slice(0, 5000);
    if (!rows.length) return '';
    const cols = Math.max(...rows.map((r) => r.length));
    const line = (r) => '| ' + Array.from({ length: cols }, (_, i) => (r[i] ?? '').replace(/\|/g, '\\|').replace(/\n/g, ' ')).join(' | ') + ' |';
    return [line(rows[0]), line(Array(cols).fill('---')), ...rows.slice(1).map(line)].join('\n') + '\n';
  }

  // ------------------------------------------------------- data tree
  const CHUNK = 200;
  const typeOf = (v) => (v === null ? 'null' : Array.isArray(v) ? 'array' : typeof v);
  const pathKey = (path, key) => (typeof key === 'number' ? `${path}[${key}]` : /^[A-Za-z_$][\w$]*$/.test(key) ? `${path}.${key}` : `${path}[${JSON.stringify(key)}]`);
  function leafEl(v) {
    const t = typeOf(v);
    if (t === 'string') {
      const shown = v.length > 400 ? v.slice(0, 400) + '…' : v;
      const el = h('span', { class: 'tv tv-string' }, JSON.stringify(shown));
      if (/^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(v)) el.prepend(h('i', { class: 'tv-swatch', style: `background:${v}` }));
      if (v.length > 400) el.title = `${v.length.toLocaleString()} characters`;
      return el;
    }
    return h('span', { class: 'tv tv-' + t, text: String(v) });
  }
  function peek(v) {
    if (Array.isArray(v)) return v.length ? '' : '[]';
    const keys = Object.keys(v);
    return keys.length ? '{ ' + keys.slice(0, 5).join(', ') + (keys.length > 5 ? ', …' : '') + ' }' : '{}';
  }
  function nodeEl(key, value, path, open) {
    const t = typeOf(value);
    const keyEl = key === null ? null : h('span', { class: 'tn-key' + (typeof key === 'number' ? ' idx' : ''), text: String(key), 'data-path': path });
    if (t !== 'object' && t !== 'array') return h('div', { class: 'tn leaf' }, keyEl, keyEl ? h('span', { class: 'tn-sep', text: ':' }) : null, leafEl(value));
    const entries = t === 'array' ? value : Object.keys(value);
    const count = entries.length;
    const kids = h('div', { class: 'tn-kids', hidden: true });
    const el = h('div', { class: 'tn branch' },
      h('div', { class: 'tn-head' }, h('span', { class: 'tn-caret' }), keyEl,
        h('span', { class: 'tn-meta', text: t === 'array' ? `[${count}]` : `{${count}}` }), h('span', { class: 'tn-peek', text: peek(value) })), kids);
    let built = 0;
    const grow = () => {
      kids.querySelector(':scope > .tn-more')?.remove();
      const end = Math.min(count, built + CHUNK);
      const frag = document.createDocumentFragment();
      for (let i = built; i < end; i++) {
        const k = t === 'array' ? i : entries[i];
        frag.append(nodeEl(k, value[k], pathKey(path, k), false));
      }
      built = end;
      if (built < count) frag.append(h('button', { class: 'tn-more', text: `Show ${Math.min(CHUNK, count - built)} more of ${(count - built).toLocaleString()}`, onclick: grow }));
      kids.append(frag);
    };
    el._toggle = (show) => {
      if (show && !built && count) grow();
      kids.hidden = !show;
      el.classList.toggle('open', show);
    };
    if (open) el._toggle(true);
    return el;
  }
  function mountTree(box, value, label) {
    const t = typeOf(value);
    const size = t === 'array' ? `${value.length.toLocaleString()} items` : t === 'object' ? `${Object.keys(value).length.toLocaleString()} keys` : t;
    const root = nodeEl(null, value, '$', true);
    const setAll = (show) => {
      let budget = 3000;
      const walk = (el) => {
        if (!el._toggle || budget-- <= 0) return;
        el._toggle(show);
        if (show) for (const c of el.querySelectorAll(':scope > .tn-kids > .branch')) walk(c);
      };
      if (show) walk(root);
      else { for (const b of root.querySelectorAll('.branch.open')) b._toggle(false); root._toggle(true); }
    };
    box.replaceChildren(
      h('div', { class: 'tree-bar' }, h('b', { text: label }), h('span', { text: size }), h('span', { class: 'grow' }),
        h('button', { class: 'btn sm', text: 'Expand all', onclick: () => setAll(true) }),
        h('button', { class: 'btn sm', text: 'Collapse', onclick: () => setAll(false) })),
      h('div', { class: 'tree' }, root));
    box.onclick = (e) => {
      const key = e.target.closest('.tn-key');
      if (key && e.detail === 2) {
        navigator.clipboard.writeText(key.dataset.path).then(() => toast('Copied path ' + key.dataset.path));
        return;
      }
      const head = e.target.closest('.tn-head');
      if (head && !String(getSelection())) head.parentElement._toggle(!head.parentElement.classList.contains('open'));
    };
  }

  // -------------------------------------------------------- producers
  let rev = 0;
  /** Build preview HTML for a non-Markdown document. Returns {html, mount?, status?}. */
  async function produce(doc, text) {
    const k = doc.kind;
    if (k.diagram) return { html: text.trim() ? fence(k.diagram, text) : '<p class="data-summary">Start typing to draw the diagram.</p>' };
    if (k.data) {
      const res = await host.invoke('data_parse', { format: k.data, text });
      if (res.error !== undefined) {
        const where = res.line ? `Line ${res.line}: ` : '';
        const stale = doc.lastValue !== undefined;
        return {
          status: { ok: false, message: where + res.error, line: res.line || 0 },
          html: `<div class="data-error"><b>${k.label} is not valid</b>${escapeHtml(where + res.error)}</div>` + (stale ? `<div class="data-view stale" data-rev="${++rev}"></div>` : ''),
          mount: stale ? (root) => mountTree(root.querySelector('.data-view'), doc.lastValue, k.label) : null,
        };
      }
      doc.lastValue = res.value;
      return {
        status: { ok: true, message: 'Valid ' + k.label },
        html: `<div class="data-view" data-rev="${++rev}"></div>`,
        mount: (root) => mountTree(root.querySelector('.data-view'), res.value, k.label),
      };
    }
    if (k.id === 'csv') return { html: csvHtml(text, doc.name) };
    if (k.id === 'html') return { html: text };
    if (k.id === 'adoc') {
      await loadScript('vendor/asciidoctor.bundle.js');
      try {
        return { html: await Adoc.convert(text, { standalone: false, safe: 'secure', attributes: { showtitle: true, 'source-highlighter': 'highlight.js' } }) };
      } catch (e) {
        return { html: `<div class="data-error"><b>AsciiDoc could not be converted</b>${escapeHtml(String(e.message || e))}</div>` };
      }
    }
    const big = text.length > 400000;
    return { html: big ? `<pre><code>${escapeHtml(text)}</code></pre>` : fence(k.lang || 'text', text) };
  }

  /** Markdown equivalent used by the Word / RTF exporters; null when there is none. */
  function asMarkdown(doc, text) {
    const k = doc.kind;
    if (k.id === 'markdown') return text;
    if (k.id === 'csv') return csvMarkdown(text, doc.name);
    if (k.id === 'adoc' || k.id === 'html') return null;
    let ticks = '```';
    while (text.includes(ticks)) ticks += '`';
    return `${ticks}${k.diagram || k.lang || ''}\n${text.replace(/\n$/, '')}\n${ticks}\n`;
  }

  const STARTERS = [
    ['JSON', 'Untitled.json', '{\n  "name": "example",\n  "version": 1,\n  "tags": ["a", "b"],\n  "nested": { "enabled": true, "ratio": 0.5 }\n}\n'],
    ['YAML', 'Untitled.yaml', 'name: example\nversion: 1\ntags:\n  - a\n  - b\nnested:\n  enabled: true\n  ratio: 0.5\n'],
    ['TOML', 'Untitled.toml', 'name = "example"\nversion = 1\ntags = ["a", "b"]\n\n[nested]\nenabled = true\nratio = 0.5\n'],
    ['CSV table', 'Untitled.csv', 'Name,Quantity,Price\nApples,3,1.20\nPears,5,0.95\n'],
    ['Drawing (visual designer)', 'Untitled.svg', ''],
    ['Mermaid diagram', 'Untitled.mmd', 'flowchart LR\n    A[Start] --> B{Decision}\n    B -- Yes --> C[Do the thing]\n    B -- No --> D[Skip it]\n    C --> E[Done]\n    D --> E\n'],
    ['Graphviz diagram', 'Untitled.dot', 'digraph G {\n    rankdir=LR;\n    node [shape=box, style=rounded];\n    A -> B -> C;\n    A -> C;\n}\n'],
    ['AsciiDoc', 'Untitled.adoc', '= Document title\nAuthor Name\n\nOpening paragraph with *bold*, _italic_ and `code`.\n\n== Section\n\n* First point\n* Second point\n\nNOTE: An admonition draws attention to something.\n\n[source,rust]\n----\nfn main() {}\n----\n'],
    ['HTML page', 'Untitled.html', '<h1>Title</h1>\n<p>A paragraph of <strong>HTML</strong>.</p>\n'],
    ['Plain text', 'Untitled.txt', ''],
  ];

  return { of, produce, asMarkdown, DEFS, STARTERS };
})();
