'use strict';
/* Rich ("Write") mode: a WYSIWYG editor on Tiptap/ProseMirror.
   Markdown -> HTML comes from the Rust renderer (render_rich); this file turns the
   edited document back into Markdown. The source is only rewritten after a real edit. */

function createRich(hooks) {
  const { Editor: TipEditor, Node, Mark, mergeAttributes, StarterKit, CodeBlock, Link, Image, Table, TableRow, TableCell,
    TableHeader, TaskList, TaskItem, Underline, Highlight, Subscript, Superscript, Placeholder, EditorState } = TT;
  const pane = $('#rich-pane');
  const mount = $('#rich');
  let editor = null;
  let doc = null;
  let frontMatter = '';
  let baseDir = '';
  let loading = false;
  let pending = false;
  let loadSeq = 0;

  // ================================================== Markdown serializer
  const isWord = (c) => !!c && /[\p{L}\p{N}]/u.test(c);
  function esc(text) {
    return text
      .replace(/\\/g, '\\\\')
      .replace(/[*`$]/g, '\\$&')
      .replace(/_/g, (m, i, s) => (isWord(s[i - 1]) && isWord(s[i + 1]) ? '_' : '\\_'))
      .replace(/<(?=[a-zA-Z/!?])/g, '\\<')
      .replace(/&(?=#?\w+;)/g, '\\&')
      .replace(/~/g, '\\~')
      .replace(/(?<!\[)\^/g, '\\^')
      .replace(/==/g, '\\==')
      .replace(/\+\+/g, '\\++');
  }
  function escStart(s) {
    return s
      .replace(/^[ \t]+/, '')
      .replace(/^(#{1,6})(?=\s|$)/, '\\$1')
      .replace(/^([-+>])(?=\s|$)/, '\\$1')
      .replace(/^(\d+)([.)])(?=\s|$)/, '$1\\$2')
      .replace(/^([-=_])(\1{2,}\s*)$/, '\\$1$2')
      .replace(/^(\|)/, '\\$1');
  }
  function codeSpan(text) {
    let ticks = '`';
    while (text.includes(ticks)) ticks += '`';
    const pad = text.startsWith('`') || text.endsWith('`') ? ' ' : '';
    return ticks + pad + text + pad + ticks;
  }
  const dest = (url) => (/[\s()<>]/.test(url) ? `<${url.replace(/[<>]/g, encodeURIComponent)}>` : url);
  const titlePart = (t) => (t ? ` "${String(t).replace(/"/g, '\\"')}"` : '');

  const ORDER = ['link', 'bold', 'italic', 'strike', 'highlight', 'underline', 'superscript', 'subscript', 'kbd'];
  const DELIM = {
    bold: ['**', '**'], italic: ['*', '*'], strike: ['~~', '~~'], highlight: ['==', '=='], underline: ['++', '++'],
    superscript: ['^', '^'], subscript: ['~', '~'], kbd: ['<kbd>', '</kbd>'],
  };
  const sameMark = (a, b) => a.type === b.type && (a.type !== 'link' || a.attrs.href === b.attrs.href);

  function atom(n) {
    const a = n.attrs || {};
    switch (n.type) {
      case 'image': return `![${(a.alt || '').replace(/[[\]]/g, '\\$&')}](${dest(a.src || '')}${titlePart(a.title)})`;
      case 'mathInline': return a.display ? `$$${a.tex}$$` : a.code ? `$\`${a.tex}\`$` : `$${a.tex}$`;
      case 'htmlInline': return a.html;
      case 'softBreak': return '\n';
      case 'hardBreak': return '\\\n';
      default: return '';
    }
  }

  function inline(nodes = []) {
    let out = '';
    const open = [];
    const closeTo = (depth) => {
      if (open.length <= depth) return;
      let ws = '';
      const m = /[ \t]+$/.exec(out);
      if (m) { ws = m[0]; out = out.slice(0, -ws.length); }
      while (open.length > depth) {
        const mk = open.pop();
        if (mk.type === 'link') {
          const href = mk.attrs.href || '';
          const inner = out.slice(mk.start + 1);
          if ((inner === href || inner === esc(href)) && /^[a-z][a-z0-9+.-]*:\S+$/i.test(href) && !/[<>]/.test(href)) out = out.slice(0, mk.start) + `<${href}>`;
          else out += `](${dest(href)}${titlePart(mk.attrs.title)})`;
        } else out += DELIM[mk.type][1];
      }
      out += ws;
    };
    for (const n of nodes) {
      if (n.type === 'softBreak' || n.type === 'hardBreak') {
        const m = /[ \t]+$/.exec(out);
        if (m) out = out.slice(0, -m[0].length);
        if (n.type === 'hardBreak' && !open.length) out += '\\\n';
        else if (n.type === 'hardBreak') out += '<br>';
        else out += '\n';
        continue;
      }
      const all = n.marks || [];
      const isCode = n.type === 'text' && all.some((m) => m.type === 'code');
      const marks = all.filter((m) => ORDER.includes(m.type)).sort((a, b) => ORDER.indexOf(a.type) - ORDER.indexOf(b.type));
      let text = n.type === 'text' ? n.text : atom(n);
      if (n.type === 'text' && !isCode && !text.trim()) { out += text; continue; }
      let k = 0;
      while (k < open.length && k < marks.length && sameMark(open[k], marks[k])) k++;
      closeTo(k);
      let lead = '', trail = '';
      if (n.type === 'text' && !isCode) {
        lead = /^\s*/.exec(text)[0];
        text = text.slice(lead.length);
        trail = /\s*$/.exec(text)[0];
        text = esc(text.slice(0, text.length - trail.length));
      } else if (isCode) text = codeSpan(text);
      out += lead;
      for (let i = k; i < marks.length; i++) {
        const mk = { type: marks[i].type, attrs: marks[i].attrs || {}, start: out.length };
        open.push(mk);
        out += mk.type === 'link' ? '[' : DELIM[mk.type][0];
      }
      out += text + trail;
    }
    closeTo(0);
    return out;
  }

  const indent = (text, pad) => text.split('\n').map((l, i) => (i && l ? pad + l : l)).join('\n');
  const prefix = (text, p) => text.split('\n').map((l) => (l ? p + l : p.trimEnd())).join('\n');
  const textOf = (n) => (n.content || []).map((c) => c.text || '').join('');

  function list(n, marker) {
    const items = n.content || [];
    const simple = (it) => {
      const kids = it.content || [];
      return kids.filter((k) => k.type === 'paragraph').length <= 1
        && kids.every((k, i) => (k.type === 'paragraph' && i === 0) || /List$/.test(k.type));
    };
    const tight = items.every(simple);
    return items.map((it, i) => {
      const m = marker(i, it);
      const pad = ' '.repeat(/^\d/.test(m) ? m.length : 2);
      const body = (it.content || []).map(block).filter((b) => b !== null).join(tight ? '\n' : '\n\n');
      return m + indent(body, pad);
    }).join(tight ? '\n' : '\n\n');
  }

  function table(n) {
    const rows = (n.content || []).map((r) => (r.content || []).map((c) => ({
      text: (c.content || []).map((p) => (p.type === 'paragraph' ? inline(p.content) : block(p) || '')).filter(Boolean).join('<br>')
        .replace(/\\\n|\n/g, '<br>').replace(/\|/g, '\\|'),
      align: c.attrs && c.attrs.align,
    })));
    if (!rows.length) return '';
    const cols = Math.max(...rows.map((r) => r.length));
    const width = Array.from({ length: cols }, (_, i) => Math.max(3, ...rows.map((r) => (r[i] ? r[i].text.length : 0))));
    const line = (cells) => '| ' + width.map((w, i) => cells[i].padEnd(w)).join(' | ') + ' |';
    const sep = width.map((w, i) => {
      const a = rows[0][i] && rows[0][i].align;
      return (a === 'center' || a === 'left' ? ':' : '-') + '-'.repeat(w - 2) + (a === 'center' || a === 'right' ? ':' : '-');
    });
    const cellsOf = (r) => Array.from({ length: cols }, (_, i) => (r[i] ? r[i].text : ''));
    return [line(cellsOf(rows[0])), line(sep), ...rows.slice(1).map((r) => line(cellsOf(r)))].join('\n');
  }

  function block(n) {
    const a = n.attrs || {};
    switch (n.type) {
      case 'paragraph': {
        const s = inline(n.content);
        return s.trim() ? escStart(s) : null;
      }
      case 'heading': return '#'.repeat(a.level || 1) + ' ' + inline(n.content).replace(/\\?\n/g, ' ');
      case 'blockquote': return prefix(blocks(n.content), '> ');
      case 'alert': return prefix(`[!${String(a.type || 'note').toUpperCase()}]\n` + blocks(n.content), '> ');
      case 'codeBlock': {
        const text = textOf(n);
        let fence = '```';
        while (text.includes(fence)) fence += '`';
        return `${fence}${a.language || ''}\n${text}${text.endsWith('\n') || !text ? '' : '\n'}${fence}`;
      }
      case 'horizontalRule': return '---';
      case 'bulletList': return list(n, () => '- ');
      case 'orderedList': return list(n, (i) => `${(a.start || 1) + i}. `);
      case 'taskList': return list(n, (i, it) => `- [${it.attrs && it.attrs.checked ? 'x' : ' '}] `);
      case 'table': return table(n);
      case 'mathBlock': return a.fence ? '```math\n' + a.tex.trim() + '\n```' : '$$\n' + a.tex.trim() + '\n$$';
      case 'htmlBlock': return a.html;
      case 'descList': return (n.content || []).map(block).join('\n');
      case 'descTerm': return escStart(inline(n.content));
      case 'descDetails': return ': ' + indent(blocks(n.content), '  ');
      default: return n.content ? blocks(n.content) : null;
    }
  }
  const blocks = (nodes = []) => nodes.map(block).filter((b) => b !== null && b !== '').join('\n\n');

  function toMarkdown() {
    const body = blocks(editor.getJSON().content);
    const fm = frontMatter ? frontMatter.replace(/\s+$/, '') + '\n\n' : '';
    return fm + body + (body ? '\n' : '');
  }

  // ============================================== HTML -> editor content
  function prepare(html) {
    const tpl = document.createElement('template');
    tpl.innerHTML = html;
    const root = tpl.content;
    for (const el of root.querySelectorAll('div.markdown-alert')) {
      el.setAttribute('data-alert', (/markdown-alert-(\w+)/.exec(el.className) || [, 'note'])[1]);
      el.querySelector(':scope > p.markdown-alert-title')?.remove();
    }
    for (const li of root.querySelectorAll('li')) {
      const cb = li.querySelector(':scope > input[type="checkbox"], :scope > p:first-child > input[type="checkbox"]');
      if (!cb) continue;
      li.setAttribute('data-type', 'taskItem');
      li.setAttribute('data-checked', cb.hasAttribute('checked') ? 'true' : 'false');
      cb.remove();
      li.parentElement.setAttribute('data-type', 'taskList');
    }
    for (const li of root.querySelectorAll('ul[data-type="taskList"] > li:not([data-type])')) {
      li.setAttribute('data-type', 'taskItem');
      li.setAttribute('data-checked', 'false');
    }
    for (const el of root.querySelectorAll('[data-math-style]')) {
      const display = el.getAttribute('data-math-style') === 'display';
      const tex = el.textContent;
      const parent = el.parentElement;
      const alone = parent && Array.from(parent.childNodes).every((c) => c === el || (c.nodeType === 3 && !c.nodeValue.trim()));
      if (el.tagName === 'CODE' && parent && parent.tagName === 'PRE') {
        parent.replaceWith(h('div', { 'data-math-block': '', 'data-tex': tex, 'data-fence': '1' }));
      } else if (display && parent && parent.tagName === 'P' && alone) {
        parent.replaceWith(h('div', { 'data-math-block': '', 'data-tex': tex }));
      } else {
        el.replaceWith(h('span', { 'data-math-inline': '', 'data-tex': tex, 'data-display': display ? '1' : null, 'data-code': el.tagName === 'CODE' ? '1' : null }));
      }
    }
    return tpl.innerHTML;
  }

  function resolveSrc(src) {
    if (!src || /^(https?:|data:|blob:)/i.test(src)) return src || '';
    let p = src.split(/[?#]/)[0];
    try { p = decodeURIComponent(p); } catch { /* raw */ }
    if (/^file:\/\//i.test(p)) p = pathx.fromFileUrl(p);
    if (pathx.isAbs(p)) return pathx.fileUrl(pathx.norm(p));
    return baseDir ? pathx.fileUrl(pathx.join(baseDir, p)) : src;
  }

  // ====================================================== custom nodes
  const attr = (name, data, fallback = null, bool = false) => ({
    [name]: {
      default: fallback,
      parseHTML: (el) => (bool ? el.hasAttribute(data) : el.getAttribute(data)),
      renderHTML: (a) => (a[name] ? { [data]: bool ? '' : a[name] } : {}),
    },
  });
  const setAttrs = (getPos, node, patch) => {
    const pos = getPos();
    if (typeof pos === 'number') editor.view.dispatch(editor.state.tr.setNodeMarkup(pos, undefined, { ...node.attrs, ...patch }));
  };

  /** Small popover with a textarea for editing a node's source. */
  function sourcePopover(anchor, { title, value, rows = 4, preview }, apply) {
    const area = h('textarea', { rows, spellcheck: 'false', class: 'src-area' });
    area.value = value;
    const live = preview ? h('div', { class: 'src-preview' }) : null;
    const paint = () => { if (live) preview(area.value, live); };
    area.addEventListener('input', paint);
    const done = () => { popover.close(); apply(area.value); editor.commands.focus(); };
    area.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && (e.ctrlKey || rows === 1)) { e.preventDefault(); done(); }
      e.stopPropagation();
    });
    popover.open(anchor, [
      h('div', { class: 'menu-title', style: 'padding-left:0', text: title }), area, live,
      h('div', { class: 'src-actions' }, h('small', { text: 'Ctrl+Enter to apply' }), h('button', { class: 'btn sm primary', text: 'Apply', onclick: done })),
    ], { cls: 'src-popover' });
    paint();
    area.focus();
    area.select();
  }
  const texPreview = (display) => (tex, el) => {
    try { katex.render(tex, el, { displayMode: display, throwOnError: true }); el.classList.remove('math-error'); }
    catch (e) { el.textContent = String(e.message || e).replace(/^KaTeX parse error: /, ''); el.classList.add('math-error'); }
  };

  const mathView = (display) => ({ node, getPos }) => {
    const dom = h(display ? 'div' : 'span', { class: display ? 'rich-math math-display' : 'rich-math', 'data-tip': 'Double-click to edit' });
    const paint = (n) => {
      try { katex.render(n.attrs.tex || '', dom, { displayMode: display || !!n.attrs.display, throwOnError: false }); }
      catch { dom.textContent = n.attrs.tex; }
    };
    paint(node);
    dom.addEventListener('dblclick', (e) => {
      e.preventDefault();
      sourcePopover(dom, { title: 'LaTeX', value: node.attrs.tex, rows: display ? 5 : 2, preview: texPreview(display) }, (tex) => setAttrs(getPos, node, { tex }));
    });
    return { dom, update(n) { if (n.type !== node.type) return false; node = n; paint(n); return true; }, ignoreMutation: () => true };
  };

  const MathInline = Node.create({
    name: 'mathInline', group: 'inline', inline: true, atom: true,
    addAttributes: () => ({ ...attr('tex', 'data-tex', ''), ...attr('display', 'data-display', false, true), ...attr('code', 'data-code', false, true) }),
    parseHTML: () => [{ tag: 'span[data-math-inline]' }],
    renderHTML: ({ HTMLAttributes }) => ['span', mergeAttributes(HTMLAttributes, { 'data-math-inline': '' })],
    addNodeView: () => mathView(false),
  });
  const MathBlock = Node.create({
    name: 'mathBlock', group: 'block', atom: true,
    addAttributes: () => ({ ...attr('tex', 'data-tex', ''), ...attr('fence', 'data-fence', false, true) }),
    parseHTML: () => [{ tag: 'div[data-math-block]' }],
    renderHTML: ({ HTMLAttributes }) => ['div', mergeAttributes(HTMLAttributes, { 'data-math-block': '' })],
    addNodeView: () => mathView(true),
  });

  const htmlView = (isBlock) => ({ node, getPos }) => {
    const dom = h(isBlock ? 'div' : 'span', { class: isBlock ? 'rich-html' : 'rich-tag', 'data-tip': 'Double-click to edit HTML' });
    const paint = (n) => {
      const html = n.attrs.html || '';
      if (isBlock || /^<img\s/i.test(html)) {
        const frag = DOMPurify.sanitize(html, { RETURN_DOM_FRAGMENT: true, FORBID_TAGS: ['style', 'form', 'link', 'meta', 'base'] });
        dom.replaceChildren(frag);
        if (!dom.textContent.trim() && !dom.querySelector('img,video,svg,hr,table')) dom.replaceChildren(h('code', { text: html.length > 80 ? html.slice(0, 80) + '…' : html }));
      } else dom.textContent = html;
    };
    paint(node);
    dom.addEventListener('dblclick', (e) => {
      e.preventDefault();
      sourcePopover(dom, { title: 'HTML', value: node.attrs.html, rows: isBlock ? 7 : 1 }, (html) => html.trim() && setAttrs(getPos, node, { html }));
    });
    return { dom, update(n) { if (n.type !== node.type) return false; node = n; paint(n); return true; }, ignoreMutation: () => true, stopEvent: (e) => e.type === 'click' && !!e.target.closest('summary') };
  };
  const HtmlBlock = Node.create({
    name: 'htmlBlock', group: 'block', atom: true,
    addAttributes: () => attr('html', 'data-raw-html', ''),
    parseHTML: () => [{ tag: 'div[data-raw-html]' }],
    renderHTML: ({ HTMLAttributes }) => ['div', HTMLAttributes],
    addNodeView: () => htmlView(true),
  });
  const HtmlInline = Node.create({
    name: 'htmlInline', group: 'inline', inline: true, atom: true,
    addAttributes: () => attr('html', 'data-raw-html', ''),
    parseHTML: () => [{ tag: 'span[data-raw-html]' }],
    renderHTML: ({ HTMLAttributes }) => ['span', HTMLAttributes],
    addNodeView: () => htmlView(false),
  });
  const SoftBreak = Node.create({
    name: 'softBreak', group: 'inline', inline: true, atom: true, selectable: false,
    parseHTML: () => [{ tag: 'span[data-soft-break]' }],
    renderHTML: () => ['span', { 'data-soft-break': '', class: 'soft-break' }, ' '],
  });
  const Alert = Node.create({
    name: 'alert', group: 'block', content: 'block+', defining: true,
    addAttributes: () => attr('type', 'data-alert', 'note'),
    parseHTML: () => [{ tag: 'div[data-alert]' }],
    renderHTML: ({ node, HTMLAttributes }) => ['div', mergeAttributes(HTMLAttributes, { class: `markdown-alert markdown-alert-${node.attrs.type}` }), 0],
  });
  const DescList = Node.create({ name: 'descList', group: 'block', content: '(descTerm | descDetails)+', parseHTML: () => [{ tag: 'dl' }], renderHTML: () => ['dl', 0] });
  const DescTerm = Node.create({ name: 'descTerm', content: 'inline*', defining: true, parseHTML: () => [{ tag: 'dt' }], renderHTML: () => ['dt', 0] });
  const DescDetails = Node.create({ name: 'descDetails', content: 'block+', defining: true, parseHTML: () => [{ tag: 'dd' }], renderHTML: () => ['dd', 0] });
  const Kbd = Mark.create({ name: 'kbd', parseHTML: () => [{ tag: 'kbd' }], renderHTML: () => ['kbd', 0] });

  const RichCode = CodeBlock.extend({
    addNodeView() {
      return ({ node, getPos }) => {
        const lang = h('input', { class: 'rich-code-lang', placeholder: 'plain text', spellcheck: 'false', 'data-tip': 'Language (mermaid, dot and chart are drawn live)' });
        lang.value = node.attrs.language || '';
        const code = h('code');
        const pre = h('pre', {}, code);
        const stage = h('div', { class: 'diagram-stage' });
        const err = h('pre', { class: 'diagram-error', hidden: true });
        const design = h('button', { class: 'btn sm rich-design', text: 'Edit drawing', hidden: true });
        design.addEventListener('mousedown', (e) => { e.preventDefault(); e.stopPropagation(); ink.editDrawing?.(current.textContent.trim()); });
        const preview = h('div', { class: 'diagram rich-diagram', contenteditable: 'false', hidden: true }, design, stage, err);
        const dom = h('div', { class: 'rich-code' }, h('div', { class: 'rich-code-head', contenteditable: 'false' }, lang), pre, preview);
        let current = node, timer = 0;
        const draw = (delay = 350) => {
          const kind = Preview.DIAGRAM_LANGS[(current.attrs.language || '').toLowerCase()];
          preview.hidden = !kind;
          design.hidden = !(kind === 'svg' && Designer.isEditable(current.textContent));
          dom.classList.toggle('has-preview', !!kind);
          clearTimeout(timer);
          if (!kind) return;
          timer = setTimeout(async () => {
            const src = current.textContent.trim();
            try {
              await Preview.renderGraphic(stage, kind, src);
              err.hidden = true; stage.style.opacity = '';
            } catch (e) {
              err.hidden = false;
              err.textContent = String(e.message || e);
              stage.style.opacity = stage.firstChild ? '.45' : '';
            }
          }, delay);
        };
        lang.addEventListener('change', () => setAttrs(getPos, current, { language: lang.value.trim() || null }));
        lang.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); lang.blur(); editor.commands.focus(); } });
        preview.addEventListener('mousedown', (e) => {
          e.preventDefault();
          const pos = getPos();
          if (typeof pos === 'number') editor.chain().focus().setTextSelection(pos + 1).run();
        });
        draw(0);
        return {
          dom, contentDOM: code,
          update(n) {
            if (n.type !== current.type) return false;
            const changed = n.attrs.language !== current.attrs.language || n.textContent !== current.textContent;
            current = n;
            if (document.activeElement !== lang) lang.value = n.attrs.language || '';
            if (changed) draw();
            return true;
          },
          stopEvent: (e) => e.target === lang,
          ignoreMutation: (m) => m.type !== 'selection' && !code.contains(m.target),
          destroy() { clearTimeout(timer); stage._chart?.dispose(); },
        };
      };
    },
  });

  const RichImage = Image.extend({
    addNodeView() {
      return ({ node }) => {
        const img = h('img');
        const paint = (n) => {
          img.src = resolveSrc(n.attrs.src);
          img.alt = n.attrs.alt || '';
          if (n.attrs.title) img.title = n.attrs.title; else img.removeAttribute('title');
        };
        paint(node);
        return { dom: img, update(n) { if (n.type !== node.type) return false; node = n; paint(n); return true; } };
      };
    },
  });
  const alignAttr = {
    align: {
      default: null,
      parseHTML: (el) => el.getAttribute('align') || el.style.textAlign || null,
      renderHTML: (a) => (a.align ? { style: `text-align:${a.align}` } : {}),
    },
  };
  // GFM table cells hold inline content only
  const withAlign = (ext) => ext.extend({ content: 'paragraph+', addAttributes() { return { ...this.parent?.(), ...alignAttr }; } });

  // ============================================================ editor
  function markEditingBlock() {
    const { $from } = editor.state.selection;
    let active = null;
    for (let d = $from.depth; d > 0; d--) {
      if ($from.node(d).type.name === 'codeBlock') { active = editor.view.nodeDOM($from.before(d)); break; }
    }
    for (const el of mount.querySelectorAll('.rich-code.editing')) if (el !== active) el.classList.remove('editing');
    active?.classList?.add('editing');
  }

  function ensure() {
    if (editor) return;
    editor = new TipEditor({
      element: mount,
      extensions: [
        StarterKit.configure({ codeBlock: false, heading: { levels: [1, 2, 3, 4, 5, 6] } }),
        RichCode,
        Underline.extend({ parseHTML: () => [{ tag: 'u' }, { tag: 'ins' }] }),
        Highlight, Subscript, Superscript, Kbd,
        Link.extend({ addAttributes() { return { ...this.parent?.(), title: { default: null } }; } })
          .configure({ openOnClick: false, autolink: true, HTMLAttributes: { target: null, rel: null, class: null } }),
        RichImage.configure({ inline: true }),
        Table.configure({ resizable: false }), TableRow, withAlign(TableHeader), withAlign(TableCell),
        TaskList, TaskItem.configure({ nested: true }),
        MathInline, MathBlock, HtmlBlock, HtmlInline, SoftBreak, Alert, DescList, DescTerm, DescDetails,
        Placeholder.configure({ placeholder: 'Start writing…  (type # for a heading, - for a list, > for a quote)' }),
      ],
      editorProps: {
        attributes: { class: 'markdown-body rich-body', spellcheck: 'true' },
        handleClick(view, pos, event) {
          const a = event.target.closest?.('a[href]');
          if (a && (event.ctrlKey || event.metaKey)) { ink.followLink(a.getAttribute('href'), a); return true; }
          return false;
        },
        handlePaste(view, event) {
          const file = Array.from(event.clipboardData?.files || []).find((f) => f.type.startsWith('image/'));
          if (!file) return false;
          ink.pasteImage?.(file);
          return true;
        },
      },
      onUpdate() {
        if (loading) return;
        pending = true;
        hooks.onEdit?.();
        sync();
      },
      onSelectionUpdate() { markEditingBlock(); },
      onTransaction() { if (!loading) hooks.onSelection?.(); },
      onFocus() { hooks.onSelection?.(); },
    });
  }

  // Clicking the empty page area below the text always gives a place to type.
  pane.addEventListener('mousedown', (e) => {
    if (!editor || e.target.closest('.ProseMirror') || e.button !== 0) return;
    e.preventDefault();
    const last = editor.state.doc.lastChild;
    if (!last || last.type.name !== 'paragraph') editor.chain().insertContentAt(editor.state.doc.content.size, { type: 'paragraph' }).run();
    editor.commands.focus('end');
  });

  function flush() {
    sync.cancel();
    if (!pending || !doc || !editor) return false;
    pending = false;
    hooks.onSync?.(doc, toMarkdown());
    return true;
  }
  const sync = debounce(flush, 260);

  /** Show `d` in the rich editor (re-reading its Markdown). */
  async function load(d) {
    ensure();
    const mine = ++loadSeq;
    sync.cancel();
    pending = false;
    const text = d.cm.getValue();
    const res = await host.invoke('render_rich', { text });
    if (mine !== loadSeq) return false;
    doc = d;
    baseDir = d.dir || '';
    frontMatter = res.frontMatter || '';
    loading = true;
    try {
      editor.commands.setContent(prepare(res.html), false);
      // fresh state: undo history must not reach into the previously shown document
      editor.view.updateState(EditorState.create({ doc: editor.state.doc, plugins: editor.state.plugins }));
    } finally { loading = false; }
    d.richGen = d.gen;
    hooks.onSelection?.();
    return true;
  }
  const isSynced = (d) => !!editor && doc === d && d.richGen === d.gen;
  function detach() { loadSeq++; sync.cancel(); pending = false; doc = null; }

  // ---- commands
  const chain = () => editor.chain().focus();
  function alignColumn(align) {
    const { $from } = editor.state.selection;
    let cellDepth = -1;
    for (let d = $from.depth; d > 0; d--) if (/^table(Cell|Header)$/.test($from.node(d).type.name)) { cellDepth = d; break; }
    if (cellDepth < 0) return false;
    const col = $from.index(cellDepth - 1);
    const tableNode = $from.node(cellDepth - 2);
    const tablePos = $from.before(cellDepth - 2);
    const tr = editor.state.tr;
    tableNode.forEach((row, rowOff) => {
      let off = 0;
      row.forEach((cell, cellOff, i) => {
        if (i === col) tr.setNodeMarkup(tablePos + 1 + rowOff + 1 + cellOff, undefined, { ...cell.attrs, align });
        off += cell.nodeSize;
      });
    });
    editor.view.dispatch(tr);
    editor.commands.focus();
    return true;
  }
  const ACTIONS = {
    bold: () => chain().toggleBold().run(),
    italic: () => chain().toggleItalic().run(),
    strike: () => chain().toggleStrike().run(),
    underline: () => chain().toggleUnderline().run(),
    highlight: () => chain().toggleHighlight().run(),
    code: () => chain().toggleCode().run(),
    sub: () => chain().toggleSubscript().run(),
    sup: () => chain().toggleSuperscript().run(),
    paragraph: () => chain().setParagraph().run(),
    heading: (level) => chain().toggleHeading({ level }).run(),
    ul: () => chain().toggleBulletList().run(),
    ol: () => chain().toggleOrderedList().run(),
    task: () => chain().toggleTaskList().run(),
    quote: () => chain().toggleBlockquote().run(),
    codeBlock: () => chain().toggleCodeBlock().run(),
    hr: () => chain().setHorizontalRule().run(),
    indent: () => chain().sinkListItem('listItem').run() || chain().sinkListItem('taskItem').run(),
    outdent: () => chain().liftListItem('listItem').run() || chain().liftListItem('taskItem').run(),
    undo: () => chain().undo().run(),
    redo: () => chain().redo().run(),
    clear: () => chain().unsetAllMarks().clearNodes().run(),
    table: ({ rows, cols }) => chain().insertTable({ rows: rows + 1, cols, withHeaderRow: true }).run(),
    rowAbove: () => chain().addRowBefore().run(),
    rowBelow: () => chain().addRowAfter().run(),
    colLeft: () => chain().addColumnBefore().run(),
    colRight: () => chain().addColumnAfter().run(),
    delRow: () => chain().deleteRow().run(),
    delCol: () => chain().deleteColumn().run(),
    delTable: () => chain().deleteTable().run(),
    align: (a) => alignColumn(a),
    image: ({ src, alt }) => chain().setImage({ src, alt }).run(),
    text: (t) => chain().insertContent({ type: 'text', text: t }).run(),
    mathInline: () => chain().insertContent({ type: 'mathInline', attrs: { tex: 'x^2' } }).run(),
    selectAll: () => chain().selectAll().run(),
  };
  const exec = (name, arg) => { ensure(); return ACTIONS[name] ? ACTIONS[name](arg) : false; };

  const MARKS = { bold: 'bold', italic: 'italic', strike: 'strike', underline: 'underline', highlight: 'highlight', code: 'code', sub: 'subscript', sup: 'superscript', link: 'link' };
  const NODES = { ul: 'bulletList', ol: 'orderedList', task: 'taskList', quote: 'blockquote', codeBlock: 'codeBlock' };
  function state() {
    if (!editor) return null;
    const active = {};
    for (const [k, v] of Object.entries(MARKS)) active[k] = editor.isActive(v);
    for (const [k, v] of Object.entries(NODES)) active[k] = editor.isActive(v);
    let style = 'Normal text';
    for (let l = 1; l <= 6; l++) if (editor.isActive('heading', { level: l })) style = 'Heading ' + l;
    if (active.codeBlock) style = 'Code block';
    return { active, style, inTable: editor.isActive('table'), canUndo: editor.can().undo(), canRedo: editor.can().redo() };
  }

  async function insertMarkdown(md) {
    ensure();
    const res = await host.invoke('render_rich', { text: md });
    chain().insertContent(prepare(res.html)).run();
  }

  function linkDialog(anchor) {
    ensure();
    const current = editor.getAttributes('link').href || '';
    const { from, to } = editor.state.selection;
    const selected = editor.state.doc.textBetween(from, to, ' ');
    const input = h('input', { type: 'text', placeholder: 'https://example.com  or  other-file.md', spellcheck: 'false', class: 'link-input' });
    input.value = current || (/^(https?:\/\/|www\.)\S+$/i.test(selected) ? selected : '');
    const apply = () => {
      const href = input.value.trim();
      popover.close();
      if (!href) return editor.chain().focus().extendMarkRange('link').unsetLink().run();
      if (from === to && !current) chain().insertContent({ type: 'text', text: href, marks: [{ type: 'link', attrs: { href } }] }).run();
      else chain().extendMarkRange('link').setLink({ href }).run();
    };
    input.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); apply(); } e.stopPropagation(); });
    popover.open(anchor, [
      h('div', { class: 'menu-title', style: 'padding-left:0', text: current ? 'Edit link' : 'Insert link' }), input,
      h('div', { class: 'src-actions' },
        current ? h('button', { class: 'btn sm', text: 'Open', onclick: () => { popover.close(); ink.followLink(current); } }) : h('span'),
        current ? h('button', { class: 'btn sm danger', text: 'Remove', onclick: () => { input.value = ''; apply(); } }) : null,
        h('button', { class: 'btn sm primary', text: 'Apply', onclick: apply })),
    ], { cls: 'src-popover' });
    input.focus();
    input.select();
  }

  function headings() {
    const out = [];
    if (!editor) return out;
    editor.state.doc.forEach((node, offset) => {
      if (node.type.name === 'heading') out.push({ level: node.attrs.level, text: node.textContent.trim(), pos: offset, el: editor.view.nodeDOM(offset) });
    });
    return out;
  }
  function selectionText() {
    if (!editor) return '';
    const { from, to } = editor.state.selection;
    return editor.state.doc.textBetween(from, to, '\n');
  }
  function replaceSelection(text) {
    const { from, to } = editor.state.selection;
    editor.chain().focus().insertContentAt({ from, to }, { type: 'text', text }).run();
  }

  const finder = makeDomFinder(mount, pane);
  const api = {
    pane, mount, load, flush, detach, isSynced, exec, state, insertMarkdown, linkDialog, headings, selectionText, replaceSelection,
    toMarkdown: () => { ensure(); return toMarkdown(); },
    focus: () => editor?.commands.focus(),
    get ready() { return !!editor; },
    get editor() { return editor; },
    get hasPending() { return pending; },
    find: finder.find, findStep: finder.findStep, clearFind: finder.clearFind,
    get findCount() { return finder.findCount; },
  };
  return api;
}


/* Facade: the editor engine (Tiptap, ~360 KB) and KaTeX are only loaded the first
   time Write mode is used, so plain viewing starts fast and stays small. */
const Rich = (() => {
  const hooks = { onEdit: null, onSync: null, onSelection: null };
  let impl = null, starting = null;
  const init = () => (starting ??= Promise.all([loadScript('vendor/tiptap.bundle.js'), loadKatex()]).then(() => { impl = createRich(hooks); return impl; }));
  return {
    pane: $('#rich-pane'), mount: $('#rich'),
    load: async (d) => (await init()).load(d),
    insertMarkdown: async (md) => (await init()).insertMarkdown(md),
    flush: () => (impl ? impl.flush() : false),
    detach: () => impl?.detach(),
    isSynced: (d) => !!impl && impl.isSynced(d),
    exec: (name, arg) => impl?.exec(name, arg),
    state: () => (impl ? impl.state() : null),
    linkDialog: (a) => impl?.linkDialog(a),
    headings: () => (impl ? impl.headings() : []),
    selectionText: () => (impl ? impl.selectionText() : ''),
    replaceSelection: (t) => impl?.replaceSelection(t),
    toMarkdown: () => impl.toMarkdown(),
    focus: () => impl?.focus(),
    find: (q, o) => (impl ? impl.find(q, o) : 0),
    findStep: (d) => (impl ? impl.findStep(d) : -1),
    clearFind: () => impl?.clearFind(),
    get findCount() { return impl ? impl.findCount : 0; },
    get ready() { return !!impl && impl.ready; },
    get editor() { return impl?.editor; },
    set onEdit(f) { hooks.onEdit = f; }, set onSync(f) { hooks.onSync = f; }, set onSelection(f) { hooks.onSelection = f; },
  };
})();