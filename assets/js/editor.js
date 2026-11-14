'use strict';
/* Editor: CodeMirror setup, Markdown formatting commands, find & replace. */

const Editor = (() => {
  const MODE = {
    name: 'yaml-frontmatter',
    base: { name: 'gfm', highlightFormatting: true, fencedCodeBlockHighlighting: true, taskLists: true, strikethrough: true },
  };

  const cm = CodeMirror($('#editor'), {
    mode: MODE,
    lineWrapping: true,
    lineNumbers: false,
    styleActiveLine: true,
    keyMap: 'sublime',
    matchBrackets: true,
    autoCloseBrackets: { pairs: '()[]{}``', explode: '' },
    indentUnit: 4,
    tabSize: 4,
    indentWithTabs: false,
    placeholder: 'Start writing Markdown…',
    foldGutter: true,
    gutters: ['CodeMirror-linenumbers', 'CodeMirror-foldgutter'],
    undoDepth: 1000,
    viewportMargin: 40,
    cursorScrollMargin: 60,
    dragDrop: false,
    extraKeys: {
      Enter: 'newlineAndIndentContinueMarkdownList',
      Tab(cm) {
        const { line } = cm.getCursor();
        if (cm.somethingSelected() || /^\s*([-*+]|\d+[.)])\s/.test(cm.getLine(line))) cm.execCommand('indentMore');
        else if (cm.getOption('indentWithTabs')) cm.replaceSelection('\t', 'end', '+input');
        else cm.execCommand('insertSoftTab');
      },
      'Shift-Tab': 'indentLess',
      Home: 'goLineLeftSmart',
      'Alt-Up': (cm) => moveLines(-1),
      'Alt-Down': (cm) => moveLines(1),
      'Shift-Alt-Down': duplicateLines,
      'Ctrl-/': 'toggleComment',
      'Ctrl-F': false, 'Ctrl-G': false, 'Shift-Ctrl-F': false, 'Shift-Ctrl-R': false, 'Ctrl-S': false,
    },
  });

  // ---- languages: every CodeMirror mode is available and fetched on first use
  CodeMirror.modeURL = 'vendor/cm/mode/%N/%N.js';
  const kw = (list) => new RegExp('(?:' + list + ')\\b');
  CodeMirror.defineSimpleMode('mermaid-lite', {
    start: [
      { regex: /%%.*/, token: 'comment' },
      { regex: /"(?:[^"\\]|\\.)*"?/, token: 'string' },
      { regex: kw('flowchart|graph|sequenceDiagram|classDiagram|stateDiagram(?:-v2)?|erDiagram|gantt|pie|journey|gitGraph|mindmap|timeline|quadrantChart|xychart-beta|sankey-beta|block-beta|requirementDiagram|C4Context'), token: 'keyword' },
      { regex: kw('subgraph|end|participant|actor|as|loop|alt|else|opt|par|and|rect|note|over|left of|right of|activate|deactivate|class|classDef|style|linkStyle|click|direction|title|section|dateFormat|axisFormat|state|autonumber|commit|branch|checkout|merge|showData|TB|TD|BT|RL|LR'), token: 'atom' },
      { regex: /[-=.~]{2,}[>xo)]{0,2}|<[-=.]{2,}|[|:]{1,2}[-o|{}<>*]+/, token: 'operator' },
      { regex: /\|[^|]*\|/, token: 'string-2' },
      { regex: /\d+(?:\.\d+)?%?/, token: 'number' },
      { regex: /[[\](){}]/, token: 'bracket' },
    ],
    meta: { lineComment: '%%' },
  });
  CodeMirror.defineSimpleMode('dot-lite', {
    start: [
      { regex: /\/\/.*|#.*/, token: 'comment' },
      { regex: /\/\*/, token: 'comment', next: 'comment' },
      { regex: /"(?:[^"\\]|\\.)*"?/, token: 'string' },
      { regex: /<[^<>]*>/, token: 'string-2' },
      { regex: kw('strict|digraph|graph|subgraph|node|edge'), token: 'keyword' },
      { regex: /->|--/, token: 'operator' },
      { regex: /[A-Za-z_][\w]*(?=\s*=)/, token: 'attribute' },
      { regex: /-?\d+(?:\.\d+)?/, token: 'number' },
      { regex: /[{[]/, indent: true, token: 'bracket' },
      { regex: /[}\]]/, dedent: true, token: 'bracket' },
    ],
    comment: [{ regex: /.*?\*\//, token: 'comment', next: 'start' }, { regex: /.*/, token: 'comment' }],
    meta: { lineComment: '//' },
  });
  CodeMirror.defineSimpleMode('asciidoc-lite', {
    start: [
      { regex: /^\/\/.*/, token: 'comment', sol: true },
      { regex: /^={1,6}\s.*/, token: 'header', sol: true },
      { regex: /^:[\w-]+:.*/, token: 'meta', sol: true },
      { regex: /^(?:NOTE|TIP|IMPORTANT|WARNING|CAUTION):/, token: 'keyword', sol: true },
      { regex: /^\[.*\]\s*$/, token: 'attribute', sol: true },
      { regex: /^(?:-{4,}|={4,}|\.{4,}|\*{4,}|_{4,}|\+{4,}|\|={3,})\s*$/, token: 'hr', sol: true },
      { regex: /^\s*(?:[*.-]{1,5}|\d+\.)\s/, token: 'variable-2', sol: true },
      { regex: /^\.[^\s.].*/, token: 'string-2', sol: true },
      { regex: /`[^`\n]+`/, token: 'comment' },
      { regex: /\*\*?[^*\n]+\*\*?/, token: 'strong' },
      { regex: /__?[^_\n]+__?/, token: 'em' },
      { regex: /(?:https?|link|image|xref|include)::?[^\s[]*(?:\[[^\]]*\])?/, token: 'link' },
      { regex: /\{[\w-]+\}/, token: 'atom' },
    ],
  });

  /** Editor mode spec for a document kind (see kinds.js). */
  function specFor(kind) {
    if (!kind || kind.id === 'markdown') return { spec: MODE, load: null };
    if (kind.mode) return { spec: kind.mode, load: null };
    if (kind.id === 'json') return { spec: { name: 'javascript', json: true }, load: null };
    if (!kind.cmMode) return { spec: null, load: null };
    return { spec: kind.mime || kind.cmMode, load: kind.cmMode };
  }
  const newDoc = (text, kind) => {
    const { spec, load } = specFor(kind);
    return CodeMirror.Doc(text, load && !CodeMirror.modes[load] ? null : spec);
  };
  /** Make sure the language of the active document is loaded and applied. */
  function applyKind(kind, cmDoc) {
    const { spec, load } = specFor(kind);
    const md = !kind || kind.id === 'markdown';
    cm.setOption('autoCloseBrackets', md ? { pairs: '()[]{}``', explode: '' } : { pairs: '()[]{}\'\'""``', explode: '[]{}()' });
    // follow the file's own indentation
    let tabs = 0, spaced = 0, unit = 0;
    if (!md) {
      for (const l of cmDoc.getValue().slice(0, 40000).split('\n')) {
        if (l[0] === '\t') tabs++;
        else if (l[0] === ' ' && l.trim()) { spaced++; const w = l.length - l.trimStart().length; if (w > 1 && (!unit || w < unit)) unit = w; }
      }
    }
    const useTabs = tabs > spaced;
    cm.setOption('indentWithTabs', useTabs);
    cm.setOption('indentUnit', md ? 4 : useTabs ? 4 : [2, 3, 4, 8].includes(unit) ? unit : 2);
    indentLabel = md ? '' : useTabs ? 'Tabs' : 'Spaces: ' + cm.getOption('indentUnit');
    const apply = () => { if (cm.getDoc() === cmDoc && cm.getOption('mode') !== spec) cm.setOption('mode', spec); };
    if (load && !CodeMirror.modes[load]) CodeMirror.requireMode(load, apply);
    else apply();
  }
  let indentLabel = '';
  let errorLine = null;
  function setErrorLine(line) {
    if (errorLine) { cm.removeLineClass(errorLine, 'background', 'cm-error-line'); errorLine = null; }
    if (line && line >= 1 && line <= cm.lineCount()) errorLine = cm.addLineClass(line - 1, 'background', 'cm-error-line');
  }

  // --------------------------------------------------------- line ops
  function selectedLines() {
    const a = cm.getCursor('from'), b = cm.getCursor('to');
    const last = b.line > a.line && b.ch === 0 ? b.line - 1 : b.line;
    return [a.line, last];
  }
  function moveLines(dir) {
    const [a, b] = selectedLines();
    if ((dir < 0 && a === 0) || (dir > 0 && b === cm.lastLine())) return;
    cm.operation(() => {
      const sel = cm.listSelections()[0];
      if (dir < 0) {
        const moved = cm.getLine(a - 1);
        cm.replaceRange('', { line: a - 1, ch: 0 }, { line: a, ch: 0 });
        cm.replaceRange('\n' + moved, { line: b - 1, ch: cm.getLine(b - 1).length });
      } else {
        const moved = cm.getLine(b + 1);
        cm.replaceRange('', { line: b, ch: cm.getLine(b).length }, { line: b + 1, ch: moved.length });
        cm.replaceRange(moved + '\n', { line: a, ch: 0 });
      }
      cm.setSelection({ line: sel.anchor.line + dir, ch: sel.anchor.ch }, { line: sel.head.line + dir, ch: sel.head.ch });
    });
  }
  function duplicateLines() {
    const [a, b] = selectedLines();
    const text = cm.getRange({ line: a, ch: 0 }, { line: b, ch: cm.getLine(b).length });
    cm.replaceRange('\n' + text, { line: b, ch: cm.getLine(b).length });
  }

  // ------------------------------------------------------- formatting
  function toggleWrap(before, after = before) {
    cm.operation(() => {
      let from = cm.getCursor('from'), to = cm.getCursor('to');
      if (!cm.somethingSelected()) {
        const w = cm.findWordAt(from);
        if (/[\p{L}\p{N}]/u.test(cm.getRange(w.anchor, w.head))) { from = w.anchor; to = w.head; }
        else {
          cm.replaceSelection(before + after);
          cm.setCursor({ line: from.line, ch: from.ch + before.length });
          return;
        }
      }
      const sel = cm.getRange(from, to);
      const pre = cm.getRange({ line: from.line, ch: Math.max(0, from.ch - before.length) }, from);
      const post = cm.getRange(to, { line: to.line, ch: to.ch + after.length });
      // "*" must not mistake the inner star of "**bold**" for italics
      const single = before.length === 1 && before === after;
      const doubled = single && cm.getRange({ line: from.line, ch: Math.max(0, from.ch - 2) }, from) === before + before
        && cm.getRange({ line: from.line, ch: Math.max(0, from.ch - 3) }, from) !== before.repeat(3);
      if (pre === before && post === after && !doubled) {
        cm.replaceRange('', to, { line: to.line, ch: to.ch + after.length });
        cm.replaceRange('', { line: from.line, ch: from.ch - before.length }, from);
        const d = before.length;
        cm.setSelection({ line: from.line, ch: from.ch - d }, { line: to.line, ch: to.ch - (to.line === from.line ? d : 0) });
      } else if (sel.length >= before.length + after.length && sel.startsWith(before) && sel.endsWith(after)) {
        cm.replaceRange(sel.slice(before.length, sel.length - after.length), from, to, '+input');
        cm.setSelection(from, { line: to.line, ch: to.ch - after.length - (to.line === from.line ? before.length : 0) });
      } else {
        cm.replaceRange(before + sel + after, from, to, '+input');
        cm.setSelection({ line: from.line, ch: from.ch + before.length },
          { line: to.line, ch: to.ch + (to.line === from.line ? before.length : 0) });
      }
    });
    cm.focus();
  }

  const PREFIX = {
    quote: /^(\s*)>\s?/,
    task: /^(\s*)[-*+]\s+\[[ xX]\]\s+/,
    ul: /^(\s*)[-*+]\s+(?!\[[ xX]\]\s)/,
    ol: /^(\s*)\d+[.)]\s+/,
    heading: /^(\s*)#{1,6}\s+/,
  };
  function toggleLinePrefix(kind, level) {
    const [a, b] = selectedLines();
    const lines = [];
    for (let i = a; i <= b; i++) lines.push(cm.getLine(i));
    const content = lines.filter((l) => l.trim());
    const re = PREFIX[kind];
    const marker = (i) => ({ quote: '> ', task: '- [ ] ', ul: '- ', ol: `${i + 1}. `, heading: '#'.repeat(level) + ' ' }[kind]);
    let has = content.length > 0 && content.every((l) => re.test(l));
    if (kind === 'heading' && has) has = content.every((l) => new RegExp(`^\\s*#{${level}}\\s`).test(l));
    let n = 0;
    const out = lines.map((l) => {
      if (!l.trim() && lines.length > 1) return l;
      if (has) return l.replace(re, '$1');
      let rest = l;
      if (kind !== 'quote') for (const k of ['task', 'ul', 'ol', 'heading']) rest = rest.replace(PREFIX[k], '$1');
      const indent = /^\s*/.exec(rest)[0];
      return indent + marker(n++) + rest.slice(indent.length);
    });
    cm.operation(() => {
      cm.replaceRange(out.join('\n'), { line: a, ch: 0 }, { line: b, ch: lines[lines.length - 1].length }, '+input');
      if (a === b) cm.setCursor({ line: a, ch: out[0].length });
      else cm.setSelection({ line: a, ch: 0 }, { line: b, ch: out[out.length - 1].length });
    });
    cm.focus();
  }

  /** Insert `text` as its own block. `select` = [start, end] offsets inside text to select. */
  function insertBlock(text, select) {
    cm.operation(() => {
      const cur = cm.getCursor('to');
      const lineText = cm.getLine(cur.line);
      let prefix = '';
      let at = cur;
      if (lineText.trim()) { at = { line: cur.line, ch: lineText.length }; prefix = '\n\n'; }
      else if (cur.line > 0 && cm.getLine(cur.line - 1).trim()) prefix = '\n';
      const next = cm.getLine(at.line + 1);
      const suffix = next === undefined ? '\n' : next.trim() ? '\n\n' : '\n';
      if (cm.somethingSelected()) cm.setCursor(cur);
      const start = cm.indexFromPos(at) + prefix.length;
      cm.replaceRange(prefix + text + suffix, at, lineText.trim() ? at : { line: at.line, ch: lineText.length }, '+input');
      if (select) cm.setSelection(cm.posFromIndex(start + select[0]), cm.posFromIndex(start + select[1]));
      else cm.setCursor(cm.posFromIndex(start + text.length));
      cm.scrollIntoView(null, 80);
    });
    cm.focus();
  }

  function link() {
    const sel = cm.getSelection();
    const from = cm.getCursor('from');
    if (/^(https?:\/\/|www\.)\S+$/i.test(sel)) {
      cm.replaceSelection(`[link text](${sel})`);
      cm.setSelection({ line: from.line, ch: from.ch + 1 }, { line: from.line, ch: from.ch + 10 });
    } else if (sel) {
      cm.replaceSelection(`[${sel}](url)`);
      const end = cm.getCursor();
      cm.setSelection({ line: end.line, ch: end.ch - 4 }, { line: end.line, ch: end.ch - 1 });
    } else {
      cm.replaceSelection('[link text](url)');
      cm.setSelection({ line: from.line, ch: from.ch + 1 }, { line: from.line, ch: from.ch + 10 });
    }
    cm.focus();
  }
  function insertImage(path, alt = '') {
    const url = /[\s()]/.test(path) ? `<${path}>` : path;
    const from = cm.getCursor('from');
    cm.replaceSelection(`![${alt}](${url})`);
    cm.setSelection({ line: from.line, ch: from.ch + 2 }, { line: from.line, ch: from.ch + 2 + alt.length });
    cm.focus();
  }
  function codeBlock() {
    const sel = cm.getSelection();
    if (sel) {
      const [a, b] = selectedLines();
      const body = cm.getRange({ line: a, ch: 0 }, { line: b, ch: cm.getLine(b).length });
      cm.replaceRange('```\n' + body + '\n```', { line: a, ch: 0 }, { line: b, ch: cm.getLine(b).length }, '+input');
      cm.setCursor({ line: a, ch: 3 });
      cm.focus();
    } else insertBlock('```\n\n```', [3, 3]);
  }
  function table(rows, cols) {
    const head = '| ' + Array.from({ length: cols }, (_, i) => `Column ${i + 1}`).join(' | ') + ' |';
    const sep = '| ' + Array(cols).fill('--------').join(' | ') + ' |';
    const row = '| ' + Array(cols).fill('        ').join(' | ') + ' |';
    insertBlock([head, sep, ...Array(rows).fill(row)].join('\n'), [2, 10]);
  }

  /** Re-align the Markdown table under the cursor. Returns false if there is none. */
  function formatTable() {
    const isRow = (n) => { const l = cm.getLine(n); return l !== undefined && l.includes('|') && l.trim() !== ''; };
    const cur = cm.getCursor().line;
    if (!isRow(cur)) return false;
    let a = cur, b = cur;
    while (isRow(a - 1)) a--;
    while (isRow(b + 1)) b++;
    const split = (l) => l.trim().replace(/^\|/, '').replace(/\|$/, '').split(/(?<!\\)\|/).map((c) => c.trim());
    const rows = [];
    for (let i = a; i <= b; i++) rows.push(split(cm.getLine(i)));
    if (rows.length < 2 || !rows[1].every((c) => /^:?-+:?$/.test(c))) return false;
    const cols = Math.max(...rows.map((r) => r.length));
    const align = Array.from({ length: cols }, (_, i) => {
      const c = rows[1][i] || '---';
      return c.startsWith(':') && c.endsWith(':') ? 'c' : c.endsWith(':') ? 'r' : c.startsWith(':') ? 'l' : '';
    });
    const width = Array.from({ length: cols }, (_, i) => Math.max(3, ...rows.map((r, n) => (n === 1 ? 3 : (r[i] || '').length))));
    const pad = (s, w, al) => al === 'r' ? s.padStart(w) : al === 'c' ? s.padStart(Math.floor((w + s.length) / 2)).padEnd(w) : s.padEnd(w);
    const out = rows.map((r, n) => '| ' + width.map((w, i) => {
      if (n === 1) {
        const al = align[i];
        return (al === 'l' || al === 'c' ? ':' : '-') + '-'.repeat(w - 2) + (al === 'r' || al === 'c' ? ':' : '-');
      }
      return pad(r[i] || '', w, align[i]);
    }).join(' | ') + ' |');
    cm.replaceRange(out.join('\n'), { line: a, ch: 0 }, { line: b, ch: cm.getLine(b).length }, '+input');
    return true;
  }

  // -------------------------------------------------- find & replace
  const finder = { re: null, overlay: null, marks: null, matches: [], index: -1 };
  function clearFind() {
    if (finder.overlay) cm.removeOverlay(finder.overlay);
    finder.marks?.clear();
    Object.assign(finder, { re: null, overlay: null, marks: null, matches: [], index: -1 });
  }
  function find(query, opts) {
    clearFind();
    const re = buildSearchRegex(query, opts);
    if (!re) return 0;
    finder.re = re;
    const tokenRe = new RegExp(re.source, re.flags.replace('m', ''));
    finder.overlay = {
      token(stream) {
        tokenRe.lastIndex = stream.pos;
        const m = tokenRe.exec(stream.string);
        if (m && m[0].length && m.index === stream.pos) { stream.pos += m[0].length; return 'find-match'; }
        if (m && m[0].length) stream.pos = m.index; else stream.skipToEnd();
        return null;
      },
    };
    cm.addOverlay(finder.overlay);
    const cursor = cm.getSearchCursor(re, { line: 0, ch: 0 });
    while (cursor.findNext() && finder.matches.length < 10000) {
      const from = cursor.from(), to = cursor.to();
      if (from.line === to.line && from.ch === to.ch) break;
      finder.matches.push({ from, to });
    }
    if (cm.showMatchesOnScrollbar) finder.marks = cm.showMatchesOnScrollbar(re);
    return finder.matches.length;
  }
  function findStep(dir) {
    const n = finder.matches.length;
    if (!n) return -1;
    const cmp = CodeMirror.cmpPos;
    if (finder.index < 0) {
      const cur = cm.getCursor(dir > 0 ? 'from' : 'to');
      const first = finder.matches.findIndex((m) => cmp(m.from, cur) >= 0);
      finder.index = dir > 0 ? (first < 0 ? 0 : first) : ((first < 0 ? n : first) - 1 + n) % n;
    } else finder.index = (finder.index + dir + n) % n;
    const m = finder.matches[finder.index];
    cm.setSelection(m.from, m.to);
    cm.scrollIntoView({ from: m.from, to: m.to }, 140);
    return finder.index;
  }
  const expand = (match, replacement) =>
    match.replace(new RegExp(finder.re.source, finder.re.flags.replace('g', '')), replacement);
  function replaceOne(replacement, literal) {
    if (finder.index < 0) return false;
    const m = finder.matches[finder.index];
    const text = cm.getRange(m.from, m.to);
    cm.replaceRange(literal ? replacement : expand(text, replacement), m.from, m.to, '+input');
    return true;
  }
  function replaceAll(replacement, literal) {
    const n = finder.matches.length;
    cm.operation(() => {
      for (let i = n - 1; i >= 0; i--) {
        const m = finder.matches[i];
        const text = cm.getRange(m.from, m.to);
        cm.replaceRange(literal ? replacement : expand(text, replacement), m.from, m.to, '+input');
      }
    });
    return n;
  }

  // ------------------------------------------------------ scroll sync
  /** Fractional 1-based source line at the top of the editor viewport. */
  function topLine() {
    const top = cm.getScrollInfo().top;
    const l = cm.lineAtHeight(top, 'local');
    const t = cm.heightAtLine(l, 'local'), b = cm.heightAtLine(l + 1, 'local');
    return l + 1 + (b > t ? clamp((top - t) / (b - t), 0, 1) : 0);
  }
  function scrollToLine(line) {
    const l = clamp(Math.floor(line) - 1, 0, cm.lastLine());
    const t = cm.heightAtLine(l, 'local'), b = cm.heightAtLine(l + 1, 'local');
    cm.scrollTo(null, t + (b - t) * (line - Math.floor(line)));
  }
  function revealLine(line) {
    const l = clamp(line - 1, 0, cm.lastLine());
    cm.setCursor({ line: l, ch: 0 });
    cm.scrollIntoView({ line: l, ch: 0 }, cm.getScrollInfo().clientHeight / 3);
    cm.focus();
  }

  // ------------------------------------------------------------ paste
  cm.on('paste', (_cm, e) => {
    const cd = e.clipboardData;
    if (!cd) return;
    const file = Array.from(cd.files || []).find((f) => f.type.startsWith('image/'));
    if (file) {
      e.preventDefault();
      ink.pasteImage?.(file);
      return;
    }
    const text = cd.getData('text/plain').trim();
    const sel = cm.getSelection();
    if (sel && !sel.includes('\n') && /^https?:\/\/\S+$/i.test(text) && !/^https?:\/\//i.test(sel)) {
      e.preventDefault();
      cm.replaceSelection(`[${sel}](${text})`);
    }
  });

  return {
    cm, newDoc, applyKind, setErrorLine, toggleWrap, toggleLinePrefix, insertBlock, link, insertImage, codeBlock, table, formatTable,
    find, findStep, clearFind, replaceOne, replaceAll, topLine, scrollToLine, revealLine,
    get findCount() { return finder.matches.length; },
    get indentLabel() { return indentLabel; },
    get findIndex() { return finder.index; },
  };
})();
