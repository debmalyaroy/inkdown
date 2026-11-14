'use strict';
/* Export: rasterises diagrams, charts, math and images (always in the light theme),
   then hands them to the Rust DOCX/RTF writers or builds clipboard-ready rich HTML. */

const Exporter = (() => {
  let stageHost = null;
  let mathCss = null;
  const TEXT = '#1f2328';

  function hostEl() {
    if (!stageHost) {
      stageHost = h('div', { class: 'export-host force-light markdown-body', 'aria-hidden': 'true' });
      document.body.append(stageHost);
    }
    stageHost.replaceChildren();
    return stageHost;
  }

  async function svgStringToPng(xml, w, hgt, scale) {
    const img = new Image();
    img.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(xml);
    await img.decode();
    const canvas = h('canvas', { width: Math.max(1, Math.ceil(w * scale)), height: Math.max(1, Math.ceil(hgt * scale)) });
    const ctx = canvas.getContext('2d');
    ctx.fillStyle = '#ffffff';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
    return canvas.toDataURL('image/png').split(',')[1];
  }
  async function svgElementToPng(svgEl, scale = 2) {
    const r = svgEl.getBoundingClientRect();
    const clone = svgEl.cloneNode(true);
    clone.setAttribute('width', r.width);
    clone.setAttribute('height', r.height);
    clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
    clone.style.maxWidth = 'none';
    return { mime: 'image/png', data: await svgStringToPng(new XMLSerializer().serializeToString(clone), r.width, r.height, scale), w: r.width, h: r.height };
  }

  // ---- diagrams and charts
  async function diagramAsset(kind, src) {
    const host = hostEl();
    if (kind === 'chart') {
      await loadScript('vendor/echarts.min.js');
      const option = JSON.parse(src);
      const box = h('div', { style: `width:720px;height:${clamp(Number(option.height) || 380, 120, 1600)}px` });
      host.append(box);
      const chart = echarts.init(box, null, { renderer: 'svg' });
      try {
        chart.setOption({ backgroundColor: '#ffffff', color: Preview.CHART_COLORS, animation: false, ...option });
        return await svgElementToPng(box.querySelector('svg'));
      } finally { chart.dispose(); }
    }
    const stage = h('div', { class: 'diagram-stage', style: 'display:block;padding:0;width:720px' });
    host.append(stage);
    stage.innerHTML = await Preview.RENDERERS[kind](src, 'light');
    const svg = stage.querySelector('svg');
    if (!svg) throw new Error('no svg');
    return svgElementToPng(svg);
  }

  // ---- math (KaTeX HTML drawn through an SVG foreignObject with embedded fonts)
  async function katexCss() {
    if (mathCss) return mathCss;
    let css = await fetch('vendor/katex/katex.min.css').then((r) => r.text());
    css = css.replace(/,url\(fonts\/[^)]+\.(?:woff|ttf)\) format\("[^"]+"\)/g, '');
    const urls = [...new Set(css.match(/url\(fonts\/[^)]+\.woff2\)/g) || [])];
    const encoded = await Promise.all(urls.map(async (u) => {
      const buf = await fetch('vendor/katex/' + u.slice(4, -1)).then((r) => r.arrayBuffer());
      let bin = '';
      const bytes = new Uint8Array(buf);
      for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
      return [u, `url(data:font/woff2;base64,${btoa(bin)})`];
    }));
    for (const [u, data] of encoded) css = css.split(u).join(data);
    // same glyph size as the preview, so the measured box matches what is drawn
    mathCss = css + '.katex{font-size:1.1em!important}.katex-display{margin:0!important}';
    return mathCss;
  }
  async function mathAsset(tex, display) {
    await loadKatex();
    const host = hostEl();
    const span = h('span', { style: `display:inline-block;font-size:16px;line-height:1.25;color:${TEXT};white-space:nowrap;padding:1px 2px` });
    katex.render(tex, span, { displayMode: display, throwOnError: false, strict: 'ignore', output: 'html' });
    const marker = h('span', { style: 'display:inline-block;width:0;height:0' });
    host.append(h('div', { style: 'font-size:16px;line-height:normal;width:max-content' }, span, marker));
    const r = span.getBoundingClientRect();
    const depth = display ? 0 : Math.max(0, r.bottom - marker.getBoundingClientRect().bottom);
    const w = Math.ceil(r.width), hgt = Math.ceil(r.height);
    const xml = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${hgt}"><foreignObject x="0" y="0" width="${w}" height="${hgt}">`
      + `<div xmlns="http://www.w3.org/1999/xhtml"><style><![CDATA[${await katexCss()}]]></style>${new XMLSerializer().serializeToString(span)}</div></foreignObject></svg>`;
    return { mime: 'image/png', data: await svgStringToPng(xml, w, hgt, 3), w, h: hgt, depth };
  }

  // ---- images
  async function imageAsset(img) {
    const src = img.dataset.src || '';
    let el = img;
    if (/^https?:/i.test(src)) {
      // re-request with CORS so that the canvas stays readable; servers that refuse are skipped
      el = new Image();
      el.crossOrigin = 'anonymous';
      el.src = src;
      await el.decode();
    }
    if (!el.complete || !el.naturalWidth) throw new Error('not loaded');
    const nw = el.naturalWidth || 300, nh = el.naturalHeight || 150;
    const shown = img.getBoundingClientRect();
    const w = shown.width > 4 ? shown.width : Math.min(nw, 600);
    const hgt = w * nh / nw;
    const pxW = Math.min(nw, 1800, Math.max(w * 2, 64));
    const canvas = h('canvas', { width: Math.round(pxW), height: Math.round(pxW * nh / nw) });
    const ctx = canvas.getContext('2d');
    const jpeg = /\.jpe?g($|[?#])/i.test(src);
    if (jpeg) { ctx.fillStyle = '#fff'; ctx.fillRect(0, 0, canvas.width, canvas.height); }
    ctx.drawImage(el, 0, 0, canvas.width, canvas.height);
    const mime = jpeg ? 'image/jpeg' : 'image/png';
    return { mime, data: canvas.toDataURL(mime, 0.9).split(',')[1], w, h: hgt };
  }

  /** Rasterise everything the current preview shows. Never throws for a single bad item. */
  async function collect() {
    const root = Preview.root;
    const out = { diagrams: [], math: [], images: [] };
    const seen = new Set();
    for (const box of root.querySelectorAll('.diagram')) {
      const key = (box._src || '').trim();
      if (!key || seen.has('d' + key)) continue;
      seen.add('d' + key);
      try { out.diagrams.push({ key, ...(await diagramAsset(box._kind, key)) }); } catch { /* falls back to the source text */ }
    }
    for (const el of root.querySelectorAll('[data-tex]')) {
      const display = el.classList.contains('math-display');
      const tex = (el.dataset.tex || '').trim();
      const key = `${display ? 1 : 0}|${tex}`;
      if (!tex || seen.has('m' + key)) continue;
      seen.add('m' + key);
      try { out.math.push({ key, ...(await mathAsset(tex, display)) }); } catch { /* falls back to LaTeX text */ }
    }
    for (const img of root.querySelectorAll('img[data-src]')) {
      const key = img.dataset.src;
      if (seen.has('i' + key) || img.closest('.diagram')) continue;
      seen.add('i' + key);
      try {
        const a = await imageAsset(img);
        out.images.push({ key, ...a });
        let decoded = key;
        try { decoded = decodeURI(key); } catch { /* keep */ }
        if (decoded !== key) out.images.push({ key: decoded, ...a });
      } catch { /* exported as a link */ }
    }
    stageHost?.replaceChildren();
    return out;
  }

  // ---- clipboard-ready rich HTML (inline styles only: Word, Outlook, Google Docs, Gmail)
  const STYLES = {
    'h1': 'font-size:22pt;font-weight:700;margin:18pt 0 6pt', 'h2': 'font-size:17pt;font-weight:700;margin:16pt 0 6pt',
    'h3': 'font-size:14pt;font-weight:700;margin:14pt 0 4pt', 'h4': 'font-size:12pt;font-weight:700;margin:12pt 0 4pt',
    'h5': 'font-size:11pt;font-weight:700', 'h6': 'font-size:11pt;font-weight:700;color:#57606a',
    'pre': 'font-family:Consolas,monospace;font-size:9.5pt;background:#f6f8fa;border:1px solid #d0d7de;padding:8pt;white-space:pre-wrap',
    'code': 'font-family:Consolas,monospace;font-size:10pt;background:#eff1f3',
    'kbd': 'font-family:Consolas,monospace;font-size:9.5pt;border:1px solid #d0d7de;padding:0 3pt',
    'blockquote': 'margin:0 0 8pt 0;padding-left:10pt;border-left:3px solid #d0d7de;color:#57606a',
    'table': 'border-collapse:collapse;margin:0 0 8pt', 'th': 'border:1px solid #bfc5cc;padding:4pt 7pt;background:#f2f4f7;font-weight:700',
    'td': 'border:1px solid #bfc5cc;padding:4pt 7pt;vertical-align:top', 'a': 'color:#0563c1', 'mark': 'background:#fff3b0',
    'hr': 'border:0;border-top:1px solid #d0d7de',
  };
  const ALERT_COLORS = { note: '#0969da', tip: '#1a7f37', important: '#8250df', warning: '#9a6700', caution: '#cf222e' };

  async function richHtml(assets) {
    const live = Preview.root;
    const clone = live.cloneNode(true);
    const byKey = (list) => new Map(list.map((a) => [a.key, a]));
    const D = byKey(assets.diagrams), M = byKey(assets.math), I = byKey(assets.images);
    const pic = (a, extra = '') => h('img', { src: `data:${a.mime};base64,${a.data}`, width: Math.round(Math.min(a.w, 620)), height: Math.round(a.h * Math.min(a.w, 620) / a.w), style: extra || null });

    const liveBoxes = live.querySelectorAll('.diagram');
    clone.querySelectorAll('.diagram').forEach((box, i) => {
      const src = (liveBoxes[i]._src || '').trim();
      const a = D.get(src);
      box.replaceWith(a ? h('p', { align: 'center', style: 'text-align:center' }, pic(a)) : h('pre', { text: src }));
    });
    for (const el of clone.querySelectorAll('[data-tex]')) {
      const display = el.classList.contains('math-display');
      const tex = (el.dataset.tex || '').trim();
      const a = M.get(`${display ? 1 : 0}|${tex}`);
      if (!a) { el.replaceWith(h('code', { text: tex })); continue; }
      el.replaceWith(display ? h('p', { align: 'center', style: 'text-align:center' }, pic(a)) : pic(a, `vertical-align:-${a.depth.toFixed(1)}px`));
    }
    for (const img of clone.querySelectorAll('img[data-src]')) {
      const a = I.get(img.dataset.src);
      if (a) img.replaceWith(pic(a));
      else if (/^https?:/i.test(img.dataset.src)) img.setAttribute('src', img.dataset.src);
      else img.replaceWith(h('span', { text: `[image: ${img.alt || img.dataset.src}]` }));
    }
    for (const cb of clone.querySelectorAll('.codeblock')) cb.replaceWith(h('pre', { text: cb.querySelector('pre').textContent.replace(/\n$/, '') }));
    for (const w of clone.querySelectorAll('.table-wrap')) w.replaceWith(...w.childNodes);
    const liveChecks = live.querySelectorAll('input[type="checkbox"]');
    clone.querySelectorAll('input[type="checkbox"]').forEach((cb, i) => cb.replaceWith(liveChecks[i]?.checked ? '☑ ' : '☐ '));
    for (const al of clone.querySelectorAll('.markdown-alert')) {
      const color = ALERT_COLORS[(/markdown-alert-(\w+)/.exec(al.className) || [])[1]] || '#0969da';
      const title = al.querySelector('.markdown-alert-title');
      if (title) title.setAttribute('style', `font-weight:700;color:${color};margin:0 0 4pt`);
      const bq = h('blockquote', { style: `margin:0 0 8pt 0;padding-left:10pt;border-left:3px solid ${color}` });
      bq.append(...al.childNodes);
      al.replaceWith(bq);
    }
    clone.querySelectorAll('.block-tools, .anchor, .frontmatter, .katex-mathml, script, style').forEach((n) => n.remove());
    for (const el of clone.querySelectorAll('*')) {
      const keepStyle = el.getAttribute('style');
      const align = el.getAttribute('align');
      for (const name of el.getAttributeNames()) {
        if (!['href', 'src', 'width', 'height', 'alt', 'title', 'colspan', 'rowspan', 'start', 'align'].includes(name)) el.removeAttribute(name);
      }
      const base = STYLES[el.tagName.toLowerCase()];
      const inPre = el.tagName === 'CODE' && el.parentElement?.tagName === 'PRE';
      const style = [inPre ? 'font-family:Consolas,monospace' : base, el.tagName === 'IMG' || el.tagName === 'BLOCKQUOTE' || el.tagName === 'P' ? keepStyle : '', align && /^T[DH]$/.test(el.tagName) ? `text-align:${align}` : '']
        .filter(Boolean).join(';');
      if (style) el.setAttribute('style', style);
    }
    return `<html><body><div style="font-family:Calibri,'Segoe UI',Arial,sans-serif;font-size:11pt;line-height:1.5;color:${TEXT}">${clone.innerHTML}</div></body></html>`;
  }

  async function copyRich(markdown) {
    const assets = await collect();
    const html = await richHtml(assets);
    window.focus();
    await navigator.clipboard.write([new ClipboardItem({
      'text/html': new Blob([html], { type: 'text/html' }),
      'text/plain': new Blob([markdown], { type: 'text/plain' }),
    })]);
    return html.length;
  }

  /** Write a .docx or .rtf file through the Rust exporter. */
  async function toFile(format, path, markdown, title) {
    const assets = await collect();
    await host.invoke('export', { format, path, text: markdown, assets, title });
    return assets;
  }

  return { collect, richHtml, copyRich, toFile };
})();
