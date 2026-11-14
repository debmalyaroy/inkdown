'use strict';
/* Visual diagram designer: a block palette, connectors and arrange tools on an SVG
   canvas. Drawings are saved as plain SVG that any program can show; the editable
   model travels inside the file as a comment, so Inkdown can reopen it. */

const Designer = (() => {
  const NS = 'http://www.w3.org/2000/svg';
  const FONT = "'Segoe UI', Arial, sans-serif";
  const GRID = 10;
  const TAG = 'inkdown-diagram:';
  Object.assign(ICONS, {
    cursor: '<path d="m4 4 7 17 2.500-7.500L21 11z"/>',
    connect: '<circle cx="5" cy="6" r="2.500"/><circle cx="19" cy="18" r="2.500"/><path d="M7.500 6H14a3 3 0 0 1 3 3v6.500"/>',
    trash: '<path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6"/>',
    grid: '<path d="M3 9h18M3 15h18M9 3v18M15 3v18"/>',
    layers: '<path d="m12 2 9 5-9 5-9-5zM3 12l9 5 9-5M3 17l9 5 9-5"/>',
  });

  const SHAPES = {
    rect: { label: 'Process', w: 140, h: 60, d: (w, h) => `M0,0H${w}V${h}H0Z` },
    round: { label: 'Rounded', w: 140, h: 60, d: (w, h) => { const r = Math.min(14, w / 2, h / 2); return `M${r},0H${w - r}Q${w},0 ${w},${r}V${h - r}Q${w},${h} ${w - r},${h}H${r}Q0,${h} 0,${h - r}V${r}Q0,0 ${r},0Z`; } },
    pill: { label: 'Start / end', w: 140, h: 50, d: (w, h) => { const r = Math.min(h / 2, w / 2); return `M${r},0H${w - r}A${r},${r} 0 0 1 ${w - r},${h}H${r}A${r},${r} 0 0 1 ${r},0Z`; } },
    diamond: { label: 'Decision', w: 150, h: 90, d: (w, h) => `M${w / 2},0L${w},${h / 2}L${w / 2},${h}L0,${h / 2}Z` },
    ellipse: { label: 'Ellipse', w: 140, h: 70, d: (w, h) => `M0,${h / 2}A${w / 2},${h / 2} 0 1 0 ${w},${h / 2}A${w / 2},${h / 2} 0 1 0 0,${h / 2}Z` },
    para: { label: 'Input / output', w: 150, h: 60, d: (w, h) => { const k = Math.min(20, w / 4); return `M${k},0H${w}L${w - k},${h}H0Z`; } },
    hex: { label: 'Preparation', w: 150, h: 60, d: (w, h) => { const k = Math.min(22, w / 4); return `M${k},0H${w - k}L${w},${h / 2}L${w - k},${h}H${k}L0,${h / 2}Z`; } },
    cylinder: { label: 'Database', w: 110, h: 80, d: (w, h) => { const e = Math.min(12, h / 4); return `M0,${e}A${w / 2},${e} 0 0 1 ${w},${e}V${h - e}A${w / 2},${e} 0 0 1 0,${h - e}ZM0,${e}A${w / 2},${e} 0 0 0 ${w},${e}`; } },
    doc: { label: 'Document', w: 140, h: 76, d: (w, h) => `M0,0H${w}V${h - 10}Q${w * 0.75},${h - 24} ${w / 2},${h - 10}T0,${h - 10}Z` },
    note: { label: 'Note', w: 140, h: 90, fill: '#fff4d6', stroke: '#d99a00', d: (w, h) => `M0,0H${w - 14}L${w},14V${h}H0ZM${w - 14},0V14H${w}` },
    text: { label: 'Text', w: 120, h: 32, fill: 'none', stroke: 'none', d: (w, h) => `M0,0H${w}V${h}H0Z` },
  };
  const COLORS = [['#eef0ff', '#6366f1'], ['#f5ebff', '#a855f7'], ['#e7f7ee', '#22a06b'], ['#fff4d6', '#d99a00'], ['#ffe9e7', '#e5484d'], ['#e6f4ff', '#2b8de3'], ['#f3f4f6', '#6b7280'], ['#ffffff', '#1f2328']];

  const s = (tag, attrs = {}, ...kids) => {
    const el = document.createElementNS(NS, tag);
    for (const [k, v] of Object.entries(attrs)) if (v != null && v !== false) el.setAttribute(k, v);
    el.append(...kids.filter((k) => k != null));
    return el;
  };
  const ctx = document.createElement('canvas').getContext('2d');
  const measure = (t, size) => { ctx.font = `${size}px ${FONT}`; return ctx.measureText(t).width; };
  function wrap(text, maxW, size) {
    const lines = [];
    for (const para of String(text).split('\n')) {
      let line = '';
      for (const word of para.split(/\s+/)) {
        const t = line ? line + ' ' + word : word;
        if (!line || measure(t, size) <= maxW) line = t; else { lines.push(line); line = word; }
      }
      lines.push(line);
    }
    return lines;
  }
  const b64 = (str) => btoa(unescape(encodeURIComponent(str)));
  const unb64 = (str) => decodeURIComponent(escape(atob(str)));
  const snap = (v, on) => (on ? Math.round(v / GRID) * GRID : Math.round(v));
  const center = (n) => ({ x: n.x + n.w / 2, y: n.y + n.h / 2 });

  /** Extract the editable model from an SVG produced by the designer. */
  function parse(svgText) {
    const m = new RegExp('<!--' + TAG + '([A-Za-z0-9+/=]+)-->').exec(svgText || '');
    if (!m) return null;
    try {
      const model = JSON.parse(unb64(m[1]));
      return Array.isArray(model.nodes) && Array.isArray(model.edges) ? model : null;
    } catch { return null; }
  }
  const isEditable = (svgText) => !!parse(svgText);

  // ------------------------------------------------------------ drawing
  function border(n, toward) {
    const c = center(n), dx = toward.x - c.x, dy = toward.y - c.y, hw = n.w / 2, hh = n.h / 2;
    if (!dx && !dy) return c;
    let t;
    if (n.type === 'ellipse') t = 1 / Math.sqrt((dx * dx) / (hw * hw) + (dy * dy) / (hh * hh));
    else if (n.type === 'diamond') t = 1 / (Math.abs(dx) / hw + Math.abs(dy) / hh);
    else t = Math.min(hw / (Math.abs(dx) || 1e-9), hh / (Math.abs(dy) || 1e-9));
    return { x: c.x + dx * t, y: c.y + dy * t };
  }
  function edgePoints(e, byId) {
    const a = byId.get(e.from), b = byId.get(e.to);
    if (!a || !b) return null;
    const ca = center(a), cb = center(b);
    if (e.route === 'elbow') {
      if (Math.abs(cb.x - ca.x) >= Math.abs(cb.y - ca.y)) {
        const sx = cb.x > ca.x ? a.x + a.w : a.x, ex = cb.x > ca.x ? b.x : b.x + b.w, mx = (sx + ex) / 2;
        return [{ x: sx, y: ca.y }, { x: mx, y: ca.y }, { x: mx, y: cb.y }, { x: ex, y: cb.y }];
      }
      const sy = cb.y > ca.y ? a.y + a.h : a.y, ey = cb.y > ca.y ? b.y : b.y + b.h, my = (sy + ey) / 2;
      return [{ x: ca.x, y: sy }, { x: ca.x, y: my }, { x: cb.x, y: my }, { x: cb.x, y: ey }];
    }
    return [border(a, cb), border(b, ca)];
  }
  function arrowHead(p, q, color) {
    const len = Math.hypot(q.x - p.x, q.y - p.y) || 1, ux = (q.x - p.x) / len, uy = (q.y - p.y) / len;
    const bx = q.x - ux * 11, by = q.y - uy * 11;
    return s('path', { d: `M${q.x},${q.y}L${bx - uy * 4.5},${by + ux * 4.5}L${bx + uy * 4.5},${by - ux * 4.5}Z`, fill: color });
  }
  function textEl(lines, cx, cy, size, color, weight) {
    const lh = size * 1.3;
    const t = s('text', { x: cx, 'text-anchor': 'middle', 'font-family': FONT, 'font-size': size, fill: color, 'font-weight': weight });
    lines.forEach((line, i) => t.append(s('tspan', { x: cx, y: cy - ((lines.length - 1) * lh) / 2 + i * lh + size * 0.35 }, line || ' ')));
    return t;
  }
  function nodeEl(n) {
    const shape = SHAPES[n.type] || SHAPES.rect;
    const g = s('g', { transform: `translate(${n.x},${n.y})`, class: 'dz-node', 'data-id': n.id });
    g.append(s('path', { d: shape.d(n.w, n.h), fill: n.fill, stroke: n.stroke, 'stroke-width': n.stroke === 'none' ? 0 : 1.6, 'stroke-linejoin': 'round', 'fill-rule': 'nonzero', 'pointer-events': 'all' }));
    if (n.text) g.append(textEl(wrap(n.text, Math.max(20, n.w - (n.type === 'diamond' ? n.w * 0.4 : 18)), n.size || 14), n.w / 2, n.h / 2 + (n.type === 'cylinder' ? 5 : 0), n.size || 14, n.color || '#1f2328', n.bold ? 600 : null));
    return g;
  }
  function edgeEl(e, byId) {
    const pts = edgePoints(e, byId);
    if (!pts) return null;
    const color = e.stroke || '#4b5563';
    const g = s('g', { class: 'dz-edge', 'data-id': e.id });
    const d = pts.map((p, i) => (i ? 'L' : 'M') + p.x + ',' + p.y).join('');
    g.append(s('path', { d, fill: 'none', stroke: color, 'stroke-width': 1.6, 'stroke-dasharray': e.dash ? '6 5' : null, 'stroke-linejoin': 'round' }));
    if (e.arrow !== 'none') g.append(arrowHead(pts[pts.length - 2], pts[pts.length - 1], color));
    if (e.arrow === 'both') g.append(arrowHead(pts[1], pts[0], color));
    if (e.text) {
      const i = Math.floor((pts.length - 1) / 2), mx = (pts[i].x + pts[i + 1].x) / 2, my = (pts[i].y + pts[i + 1].y) / 2;
      const w = measure(e.text, 12.5) + 10;
      g.append(s('rect', { x: mx - w / 2, y: my - 10, width: w, height: 20, rx: 4, fill: '#ffffff' }), textEl([e.text], mx, my, 12.5, '#374151'));
    }
    return g;
  }
  function bounds(model) {
    if (!model.nodes.length) return { x: 0, y: 0, w: 320, h: 120 };
    const x1 = Math.min(...model.nodes.map((n) => n.x)), y1 = Math.min(...model.nodes.map((n) => n.y));
    const x2 = Math.max(...model.nodes.map((n) => n.x + n.w)), y2 = Math.max(...model.nodes.map((n) => n.y + n.h));
    return { x: x1 - 16, y: y1 - 16, w: x2 - x1 + 32, h: y2 - y1 + 32 };
  }
  /** Standalone SVG text for a model. */
  function toSvg(model) {
    const b = bounds(model);
    const byId = new Map(model.nodes.map((n) => [n.id, n]));
    const svg = s('svg', { xmlns: NS, width: Math.round(b.w), height: Math.round(b.h), viewBox: `${b.x} ${b.y} ${b.w} ${b.h}` });
    svg.append(document.createComment(TAG + b64(JSON.stringify(model))));
    for (const e of model.edges) { const el = edgeEl(e, byId); if (el) { el.removeAttribute('class'); el.removeAttribute('data-id'); svg.append(el); } }
    for (const n of model.nodes) { const el = nodeEl(n); el.removeAttribute('class'); el.removeAttribute('data-id'); el.firstChild.removeAttribute('pointer-events'); svg.append(el); }
    return new XMLSerializer().serializeToString(svg);
  }
  /** Best-effort Mermaid flowchart for the same structure. */
  function toMermaid(model) {
    const wrapShape = { rect: ['[', ']'], round: ['(', ')'], pill: ['([', '])'], diamond: ['{', '}'], ellipse: ['((', '))'], para: ['[/', '/]'], hex: ['{{', '}}'], cylinder: ['[(', ')]'], doc: ['[', ']'], note: ['>', ']'], text: ['[', ']'] };
    const q = (t) => '"' + String(t || ' ').replace(/"/g, '#quot;').replace(/\n/g, '<br/>') + '"';
    const ids = new Map(model.nodes.map((n, i) => [n.id, 'N' + (i + 1)]));
    const wide = bounds(model);
    const lines = [`flowchart ${wide.w >= wide.h ? 'LR' : 'TD'}`];
    for (const n of model.nodes) { const [a, z] = wrapShape[n.type] || wrapShape.rect; lines.push(`    ${ids.get(n.id)}${a}${q(n.text)}${z}`); }
    for (const e of model.edges) {
      if (!ids.has(e.from) || !ids.has(e.to)) continue;
      const link = e.arrow === 'none' ? (e.dash ? '-.-' : '---') : e.arrow === 'both' ? '<-->' : e.dash ? '-.->' : '-->';
      lines.push(`    ${ids.get(e.from)} ${link}${e.text ? '|' + q(e.text) + '|' : ''} ${ids.get(e.to)}`);
    }
    return lines.join('\n') + '\n';
  }

  // ------------------------------------------------------------- editor
  /** Open the designer. Resolves with SVG text on save, or null when cancelled. */
  function open(svgText) {
    return new Promise((resolve) => {
      let model = parse(svgText) || { nodes: [], edges: [] };
      let seq = 1 + Math.max(0, ...[...model.nodes, ...model.edges].map((x) => parseInt(String(x.id).slice(1), 10) || 0));
      const sel = new Set();
      const view = { x: 0, y: 0, k: 1 };
      let tool = 'select', snapOn = true, clip = null, saved = JSON.stringify(model);
      const undo = [], redo = [];
      let last = JSON.stringify(model);

      const world = s('g'), gridRect = s('rect', { class: 'dz-grid', x: -5000, y: -5000, width: 10000, height: 10000, fill: 'url(#dz-dots)' });
      const gEdges = s('g'), gNodes = s('g'), gOver = s('g');
      world.append(gridRect, gEdges, gNodes, gOver);
      const svg = s('svg', { class: 'dz-canvas' },
        s('defs', {}, s('pattern', { id: 'dz-dots', width: GRID * 2, height: GRID * 2, patternUnits: 'userSpaceOnUse' }, s('circle', { cx: 1, cy: 1, r: 1, class: 'dz-dot' }))), world);
      const stage = h('div', { class: 'dz-stage' }, svg);
      const hint = h('div', { class: 'dz-hint', text: 'Click a block to add it. Drag from a blue dot to connect. Double-click to edit text.' });
      const palette = h('div', { class: 'dz-palette' }, h('div', { class: 'menu-title', text: 'Blocks' }));
      const bar = h('div', { class: 'dz-bar' });
      const root = h('div', { class: 'designer' }, bar, h('div', { class: 'dz-body' }, palette, stage), hint);

      const byId = () => new Map(model.nodes.map((n) => [n.id, n]));
      const node = (id) => model.nodes.find((n) => n.id === id);
      const edge = (id) => model.edges.find((e) => e.id === id);
      const selNodes = () => model.nodes.filter((n) => sel.has(n.id));
      const selEdges = () => model.edges.filter((e) => sel.has(e.id));
      const toWorld = (ev) => { const r = svg.getBoundingClientRect(); return { x: (ev.clientX - r.left - view.x) / view.k, y: (ev.clientY - r.top - view.y) / view.k }; };

      function commit() {
        const now = JSON.stringify(model);
        if (now === last) return;
        undo.push(last); if (undo.length > 120) undo.shift();
        redo.length = 0; last = now;
        paintBar();
      }
      function travel(from, to) {
        if (!from.length) return;
        to.push(last);
        last = from.pop();
        model = JSON.parse(last);
        sel.clear();
        draw(); paintBar();
      }

      function draw() {
        world.setAttribute('transform', `translate(${view.x},${view.y}) scale(${view.k})`);
        const map = byId();
        gEdges.replaceChildren(...model.edges.map((e) => {
          const el = edgeEl(e, map);
          if (!el) return null;
          el.prepend(s('path', { d: el.querySelector('path').getAttribute('d'), fill: 'none', stroke: 'transparent', 'stroke-width': 14, class: 'dz-hit' }));
          if (sel.has(e.id)) el.classList.add('sel');
          return el;
        }).filter(Boolean));
        gNodes.replaceChildren(...model.nodes.map(nodeEl));
        gOver.replaceChildren();
        const picked = selNodes();
        for (const n of picked) gOver.append(s('rect', { x: n.x - 3, y: n.y - 3, width: n.w + 6, height: n.h + 6, class: 'dz-outline', rx: 3 }));
        if (picked.length === 1 && sel.size === 1) {
          const n = picked[0];
          for (const [cx, cy, id] of [[n.x, n.y, 'nw'], [n.x + n.w, n.y, 'ne'], [n.x, n.y + n.h, 'sw'], [n.x + n.w, n.y + n.h, 'se']]) {
            gOver.append(s('rect', { x: cx - 4.5, y: cy - 4.5, width: 9, height: 9, class: 'dz-handle', 'data-h': id, style: `cursor:${id === 'nw' || id === 'se' ? 'nwse' : 'nesw'}-resize` }));
          }
          for (const [px, py] of [[n.x + n.w / 2, n.y - 14], [n.x + n.w + 14, n.y + n.h / 2], [n.x + n.w / 2, n.y + n.h + 14], [n.x - 14, n.y + n.h / 2]]) {
            gOver.append(s('circle', { cx: px, cy: py, r: 5.5, class: 'dz-port', 'data-port': n.id }));
          }
        }
        hint.hidden = model.nodes.length > 1;
      }

      // ---- model operations
      function addNode(type, at, text) {
        const sh = SHAPES[type];
        const c = at || { x: (svg.clientWidth / 2 - view.x) / view.k + (model.nodes.length % 6) * 14, y: (svg.clientHeight / 2 - view.y) / view.k + (model.nodes.length % 6) * 14 };
        const n = { id: 'n' + seq++, type, x: snap(c.x - sh.w / 2, snapOn), y: snap(c.y - sh.h / 2, snapOn), w: sh.w, h: sh.h, text: text ?? (type === 'text' ? 'Text' : sh.label), fill: sh.fill || COLORS[0][0], stroke: sh.stroke || COLORS[0][1], size: 14 };
        model.nodes.push(n);
        sel.clear(); sel.add(n.id);
        return n;
      }
      function connect(from, to) {
        if (from === to || model.edges.some((e) => e.from === from && e.to === to)) return null;
        const e = { id: 'e' + seq++, from, to, arrow: 'end', route: 'straight', text: '' };
        model.edges.push(e);
        return e;
      }
      function remove() {
        if (!sel.size) return;
        model.edges = model.edges.filter((e) => !sel.has(e.id) && !sel.has(e.from) && !sel.has(e.to));
        model.nodes = model.nodes.filter((n) => !sel.has(n.id));
        sel.clear(); commit(); draw();
      }
      function copy() { clip = { nodes: selNodes().map((n) => ({ ...n })), edges: model.edges.filter((e) => sel.has(e.from) && sel.has(e.to)).map((e) => ({ ...e })) }; }
      function paste(offset = 20) {
        if (!clip || !clip.nodes.length) return;
        const ids = new Map();
        sel.clear();
        for (const n of clip.nodes) { const id = 'n' + seq++; ids.set(n.id, id); model.nodes.push({ ...n, id, x: n.x + offset, y: n.y + offset }); sel.add(id); }
        for (const e of clip.edges) model.edges.push({ ...e, id: 'e' + seq++, from: ids.get(e.from), to: ids.get(e.to) });
        clip.nodes.forEach((n) => { n.x += offset; n.y += offset; });
        commit(); draw();
      }
      const apply = (fn) => { [...selNodes(), ...selEdges()].forEach(fn); commit(); draw(); };
      function arrange(kind) {
        const ns = selNodes();
        if (kind === 'front' || kind === 'back') {
          const rest = model.nodes.filter((n) => !sel.has(n.id));
          model.nodes = kind === 'front' ? [...rest, ...ns] : [...ns, ...rest];
        } else if (ns.length > 1) {
          const x1 = Math.min(...ns.map((n) => n.x)), x2 = Math.max(...ns.map((n) => n.x + n.w)), y1 = Math.min(...ns.map((n) => n.y)), y2 = Math.max(...ns.map((n) => n.y + n.h));
          const spread = (axis, size) => {
            if (ns.length < 3) return;
            const sorted = [...ns].sort((a, b) => a[axis] - b[axis]);
            const total = sorted.reduce((t, n) => t + n[size], 0);
            const gap = ((axis === 'x' ? x2 - x1 : y2 - y1) - total) / (sorted.length - 1);
            let pos = axis === 'x' ? x1 : y1;
            for (const n of sorted) { n[axis] = Math.round(pos); pos += n[size] + gap; }
          };
          for (const n of ns) {
            if (kind === 'left') n.x = x1; else if (kind === 'right') n.x = x2 - n.w; else if (kind === 'center') n.x = Math.round((x1 + x2 - n.w) / 2);
            else if (kind === 'top') n.y = y1; else if (kind === 'bottom') n.y = y2 - n.h; else if (kind === 'middle') n.y = Math.round((y1 + y2 - n.h) / 2);
          }
          if (kind === 'spreadH') spread('x', 'w');
          if (kind === 'spreadV') spread('y', 'h');
        }
        commit(); draw();
      }
      function fit() {
        const b = bounds(model), W = svg.clientWidth || 800, H = svg.clientHeight || 500;
        view.k = clamp(Math.min((W - 80) / b.w, (H - 80) / b.h), 0.2, 1.5);
        view.x = (W - b.w * view.k) / 2 - b.x * view.k;
        view.y = (H - b.h * view.k) / 2 - b.y * view.k;
        draw(); paintBar();
      }
      function zoomAt(f, cx, cy) {
        const k = clamp(view.k * f, 0.15, 4);
        view.x = cx - (cx - view.x) * (k / view.k);
        view.y = cy - (cy - view.y) * (k / view.k);
        view.k = k;
        draw(); paintBar();
      }

      // ---- inline text editing
      let editing = null;
      function editText(target, isEdge) {
        finishEdit();
        const r = svg.getBoundingClientRect();
        let box;
        if (isEdge) {
          const pts = edgePoints(target, byId());
          const i = Math.floor((pts.length - 1) / 2), mx = (pts[i].x + pts[i + 1].x) / 2, my = (pts[i].y + pts[i + 1].y) / 2;
          box = { x: mx * view.k + view.x - 70, y: my * view.k + view.y - 16, w: 140, h: 32 };
        } else box = { x: target.x * view.k + view.x, y: target.y * view.k + view.y, w: target.w * view.k, h: target.h * view.k };
        const area = h('textarea', { class: 'dz-edit', spellcheck: 'false' });
        area.value = target.text || '';
        area.style.cssText = `left:${box.x + r.left - stage.getBoundingClientRect().left}px;top:${box.y + r.top - stage.getBoundingClientRect().top}px;width:${Math.max(90, box.w)}px;height:${Math.max(32, box.h)}px;font-size:${(target.size || 14) * view.k}px`;
        stage.append(area);
        editing = { area, target };
        area.addEventListener('keydown', (e) => {
          e.stopPropagation();
          if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); finishEdit(); }
          if (e.key === 'Escape') { e.preventDefault(); area.value = target.text || ''; finishEdit(); }
        });
        area.addEventListener('blur', () => finishEdit());
        area.focus(); area.select();
      }
      function finishEdit() {
        if (!editing) return;
        const { area, target } = editing;
        editing = null;
        target.text = area.value.replace(/\s+$/, '');
        area.remove();
        commit(); draw();
      }

      // ---- pointer interaction
      function drag(ev, onMove, onEnd) {
        const move = (e) => onMove(e);
        const up = (e) => { removeEventListener('mousemove', move); removeEventListener('mouseup', up); onEnd?.(e); };
        addEventListener('mousemove', move);
        addEventListener('mouseup', up);
        ev.preventDefault();
      }
      svg.addEventListener('mousedown', (ev) => {
        if (editing) finishEdit();
        const p0 = toWorld(ev);
        if (ev.button === 1 || (ev.button === 0 && ev.altKey)) {
          const sx = ev.clientX - view.x, sy = ev.clientY - view.y;
          stage.classList.add('panning');
          return drag(ev, (e) => { view.x = e.clientX - sx; view.y = e.clientY - sy; draw(); }, () => stage.classList.remove('panning'));
        }
        if (ev.button !== 0) return;
        const port = ev.target.closest?.('[data-port]');
        const nodeG = ev.target.closest?.('.dz-node');
        const startConnect = (fromId) => {
          const from = node(fromId);
          const ghost = s('path', { class: 'dz-ghost' });
          gOver.append(ghost);
          drag(ev, (e) => {
            const p = toWorld(e), a = border(from, p);
            ghost.setAttribute('d', `M${a.x},${a.y}L${p.x},${p.y}`);
          }, (e) => {
            ghost.remove();
            const p = toWorld(e);
            const over = [...model.nodes].reverse().find((n) => n.id !== fromId && p.x >= n.x && p.x <= n.x + n.w && p.y >= n.y && p.y <= n.y + n.h);
            if (over) connect(fromId, over.id);
            else if (Math.hypot(p.x - p0.x, p.y - p0.y) > 40) { const made = addNode(from.type === 'text' ? 'round' : from.type, p); made.fill = from.fill; made.stroke = from.stroke; connect(fromId, made.id); }
            commit(); draw();
          });
        };
        if (port) return startConnect(port.dataset.port);
        const handle = ev.target.closest?.('[data-h]');
        if (handle) {
          const n = selNodes()[0], o = { ...n }, hId = handle.dataset.h;
          return drag(ev, (e) => {
            const p = toWorld(e), dx = p.x - p0.x, dy = p.y - p0.y;
            const left = hId.includes('w'), top = hId.includes('n');
            n.w = Math.max(40, snap(o.w + (left ? -dx : dx), snapOn));
            n.h = Math.max(24, snap(o.h + (top ? -dy : dy), snapOn));
            if (left) n.x = o.x + o.w - n.w;
            if (top) n.y = o.y + o.h - n.h;
            draw();
          }, commit);
        }
        if (nodeG) {
          const id = nodeG.dataset.id;
          if (tool === 'connect') return startConnect(id);
          if (ev.shiftKey) { sel.has(id) ? sel.delete(id) : sel.add(id); }
          else if (!sel.has(id)) { sel.clear(); sel.add(id); }
          draw(); paintBar();
          const start = selNodes().map((n) => ({ n, x: n.x, y: n.y }));
          let moved = false;
          return drag(ev, (e) => {
            const p = toWorld(e), dx = p.x - p0.x, dy = p.y - p0.y;
            if (!moved && Math.hypot(dx, dy) < 3) return;
            moved = true;
            for (const it of start) { it.n.x = snap(it.x + dx, snapOn); it.n.y = snap(it.y + dy, snapOn); }
            draw();
          }, () => { if (moved) commit(); });
        }
        const edgeG = ev.target.closest?.('.dz-edge');
        if (edgeG) {
          if (!ev.shiftKey) sel.clear();
          sel.add(edgeG.dataset.id);
          draw(); paintBar();
          return ev.preventDefault();
        }
        // empty canvas: marquee selection
        if (!ev.shiftKey) sel.clear();
        const box = s('rect', { class: 'dz-marquee' });
        gOver.append(box);
        draw(); gOver.append(box); paintBar();
        drag(ev, (e) => {
          const p = toWorld(e);
          const x = Math.min(p.x, p0.x), y = Math.min(p.y, p0.y), w = Math.abs(p.x - p0.x), hgt = Math.abs(p.y - p0.y);
          box.setAttribute('x', x); box.setAttribute('y', y); box.setAttribute('width', w); box.setAttribute('height', hgt);
          box._r = { x, y, w, h: hgt };
        }, () => {
          const r = box._r;
          if (r && r.w > 4 && r.h > 4) for (const n of model.nodes) if (n.x >= r.x && n.y >= r.y && n.x + n.w <= r.x + r.w && n.y + n.h <= r.y + r.h) sel.add(n.id);
          draw(); paintBar();
        });
      });
      svg.addEventListener('dblclick', (ev) => {
        const nodeG = ev.target.closest?.('.dz-node'), edgeG = ev.target.closest?.('.dz-edge');
        if (nodeG) return editText(node(nodeG.dataset.id), false);
        if (edgeG) return editText(edge(edgeG.dataset.id), true);
        const n = addNode('round', toWorld(ev), '');
        commit(); draw();
        editText(n, false);
      });
      svg.addEventListener('wheel', (ev) => {
        ev.preventDefault(); ev.stopPropagation();
        const r = svg.getBoundingClientRect();
        zoomAt(Math.exp(-ev.deltaY * 0.0014), ev.clientX - r.left, ev.clientY - r.top);
      }, { passive: false });

      // ---- palette (click to add, or drag onto the canvas)
      for (const [type, sh] of Object.entries(SHAPES)) {
        const k = Math.min(34 / sh.w, 24 / sh.h);
        const thumb = s('svg', { width: 40, height: 30, viewBox: `-3 -3 ${sh.w + 6} ${sh.h + 6}` },
          type === 'text' ? s('text', { x: sh.w / 2, y: sh.h * 0.85, 'text-anchor': 'middle', 'font-size': sh.h, 'font-family': FONT, fill: 'currentColor' }, 'T')
            : s('path', { d: sh.d(sh.w, sh.h), fill: sh.fill || COLORS[0][0], stroke: sh.stroke || COLORS[0][1], 'stroke-width': 1.6 / k }));
        const item = h('button', { class: 'dz-block', 'data-tip': 'Click to add, or drag onto the canvas' }, thumb, h('span', { text: sh.label }));
        item.addEventListener('mousedown', (ev) => {
          let moved = false;
          drag(ev, (e) => { if (Math.hypot(e.clientX - ev.clientX, e.clientY - ev.clientY) > 6) moved = true; }, (e) => {
            const r = svg.getBoundingClientRect();
            const inside = e.clientX > r.left && e.clientX < r.right && e.clientY > r.top && e.clientY < r.bottom;
            if (moved && !inside) return;
            addNode(type, moved ? toWorld(e) : null);
            commit(); draw(); paintBar();
          });
        });
        palette.append(item);
      }

      // ---- toolbar
      function paintBar() {
        const tb = (ic, tip, fn, on, disabled) => h('button', { class: 'icon-btn' + (on ? ' on' : ''), 'data-tip': tip, html: icon(ic), disabled, onclick: fn });
        const sep = () => h('div', { class: 'sep' });
        const menu = (label, items) => {
          const b = h('button', { class: 'icon-btn drop', html: `<span>${label}</span>` + icon('down') });
          b.addEventListener('click', () => showMenu(b, items));
          return b;
        };
        const ns = selNodes(), es = selEdges();
        const swatches = h('div', { class: 'dz-swatches' }, COLORS.map(([fill, stroke]) => h('button', {
          style: `background:${fill};border-color:${stroke}`, 'data-tip': 'Colour of the selection',
          onclick: () => apply((x) => { if (x.from) x.stroke = stroke; else if (x.type === 'text') x.color = stroke; else { x.fill = fill; x.stroke = stroke; } }),
        })));
        const size = (v, label) => ({ label, checked: ns.length && ns.every((n) => (n.size || 14) === v), run: () => apply((x) => { if (!x.from) x.size = v; }) });
        const A = (kind, label) => ({ label, run: () => arrange(kind) });
        const E = (patch, label) => ({ label, run: () => { for (const e of (es.length ? es : model.edges.filter((e) => sel.has(e.from) || sel.has(e.to)))) Object.assign(e, patch); commit(); draw(); } });
        bar.replaceChildren(
          h('b', { class: 'dz-title', text: 'Diagram designer' }), sep(),
          tb('undo', 'Undo (Ctrl+Z)', () => travel(undo, redo), false, !undo.length), tb('redo', 'Redo (Ctrl+Y)', () => travel(redo, undo), false, !redo.length), sep(),
          tb('cursor', 'Select and move (V)', () => { tool = 'select'; paintBar(); }, tool === 'select'),
          tb('connect', 'Connector: drag from one block to another (C)', () => { tool = 'connect'; paintBar(); }, tool === 'connect'), sep(),
          swatches,
          menu('Text', [size(12, 'Small'), size(14, 'Medium'), size(18, 'Large'), size(24, 'Title'), '-', { label: 'Bold', checked: ns.length && ns.every((n) => n.bold), run: () => { const on = !ns.every((n) => n.bold); apply((x) => { if (!x.from) x.bold = on; }); } }]),
          menu('Line', [E({ dash: false }, 'Solid'), E({ dash: true }, 'Dashed'), '-', E({ arrow: 'end' }, 'Arrow at end'), E({ arrow: 'both' }, 'Arrows at both ends'), E({ arrow: 'none' }, 'No arrowhead'), '-', E({ route: 'straight' }, 'Straight'), E({ route: 'elbow' }, 'Right-angled')]),
          menu('Arrange', [A('left', 'Align left'), A('center', 'Align centre'), A('right', 'Align right'), '-', A('top', 'Align top'), A('middle', 'Align middle'), A('bottom', 'Align bottom'), '-', A('spreadH', 'Distribute horizontally'), A('spreadV', 'Distribute vertically'), '-', A('front', 'Bring to front'), A('back', 'Send to back')]),
          sep(),
          tb('copy', 'Duplicate (Ctrl+D)', () => { copy(); paste(); }, false, !ns.length), tb('trash', 'Delete (Del)', remove, false, !sel.size),
          tb('grid', 'Snap to grid', () => { snapOn = !snapOn; paintBar(); }, snapOn), sep(),
          tb('zoomOut', 'Zoom out', () => zoomAt(1 / 1.25, svg.clientWidth / 2, svg.clientHeight / 2)),
          h('button', { class: 'dz-zoom', text: Math.round(view.k * 100) + '%', 'data-tip': 'Fit drawing to window', onclick: fit }),
          tb('zoomIn', 'Zoom in', () => zoomAt(1.25, svg.clientWidth / 2, svg.clientHeight / 2)),
          h('span', { class: 'grow' }),
          h('button', { class: 'btn sm', text: 'Copy as Mermaid', 'data-tip': 'Copy this structure as Mermaid flowchart code', onclick: () => navigator.clipboard.writeText(toMermaid(model)).then(() => toast('Mermaid flowchart copied'), fail) }),
          h('button', { class: 'btn sm', text: 'Cancel', onclick: () => close(false) }),
          h('button', { class: 'btn sm primary', text: 'Save drawing', onclick: () => close(true) }));
      }

      // ---- keyboard
      const onKey = (e) => {
        if (editing || e.target.closest?.('.popover, .scrim')) return;
        const ctrl = e.ctrlKey || e.metaKey, k = e.key.toLowerCase();
        let used = true;
        if (k === 'escape') { if (popover.isOpen) popover.close(); else if (sel.size) { sel.clear(); draw(); paintBar(); } else close(false); }
        else if (k === 'delete' || k === 'backspace') remove();
        else if (ctrl && k === 'z') travel(e.shiftKey ? redo : undo, e.shiftKey ? undo : redo);
        else if (ctrl && k === 'y') travel(redo, undo);
        else if (ctrl && k === 'a') { sel.clear(); model.nodes.forEach((n) => sel.add(n.id)); draw(); paintBar(); }
        else if (ctrl && k === 'c') copy();
        else if (ctrl && k === 'x') { copy(); remove(); }
        else if (ctrl && k === 'v') paste();
        else if (ctrl && k === 'd') { copy(); paste(); }
        else if (ctrl && k === 's') close(true);
        else if (k === 'v' && !ctrl) { tool = 'select'; paintBar(); }
        else if (k === 'c' && !ctrl) { tool = 'connect'; paintBar(); }
        else if (k === 'enter' || k === 'f2') { const n = selNodes()[0], ed = selEdges()[0]; if (n) editText(n, false); else if (ed) editText(ed, true); }
        else if (k.startsWith('arrow') && selNodes().length) {
          const step = e.shiftKey ? GRID * 5 : snapOn ? GRID : 1;
          for (const n of selNodes()) { if (k === 'arrowleft') n.x -= step; if (k === 'arrowright') n.x += step; if (k === 'arrowup') n.y -= step; if (k === 'arrowdown') n.y += step; }
          commit(); draw();
        } else used = false;
        if (used) { e.preventDefault(); e.stopPropagation(); }
      };
      document.addEventListener('keydown', onKey, true);

      async function close(save) {
        finishEdit();
        const changed = JSON.stringify(model) !== saved;
        if (!save && changed) {
          const r = await dialog({ title: 'Discard changes to this drawing?', message: 'Your edits in the designer have not been saved.', buttons: [{ label: 'Keep editing', value: null }, { label: 'Discard', value: 'discard', danger: true }, { label: 'Save drawing', value: 'save', primary: true }] });
          if (!r) return;
          save = r === 'save';
        }
        document.removeEventListener('keydown', onKey, true);
        popover.close();
        root.remove();
        resolve(save ? toSvg(model) : null);
      }

      document.body.append(root);
      paintBar();
      requestAnimationFrame(() => { if (model.nodes.length) fit(); else { view.x = 0; view.y = 0; draw(); } });
    });
  }

  return { open, parse, isEditable, toSvg, toMermaid, EMPTY: () => toSvg({ nodes: [], edges: [] }) };
})();
