// Evaluated inside the running app by `node tools/cdp.mjs eval "$(cat tools/selftest.js)"`.
(async () => {
  const out = [];
  const ok = (name, cond, extra = '') => out.push((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : '  -> ' + extra));
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const cm = Editor.cm;
  const click = (sel) => document.querySelector(sel).click();

  click('button[data-mode=split]');
  await wait(300);
  ok('split mode', document.body.dataset.mode === 'split');

  // --- live reconcile keeps untouched blocks
  const firstDiagram = document.querySelector('.diagram');
  const h1 = document.querySelector('#preview h1');
  cm.setCursor({ line: cm.lastLine(), ch: 0 });
  cm.replaceRange('\n\nA brand new paragraph with **bold** text.\n', { line: cm.lastLine(), ch: cm.getLine(cm.lastLine()).length });
  await wait(700);
  ok('new paragraph rendered', [...document.querySelectorAll('#preview p')].some((p) => p.textContent.startsWith('A brand new paragraph')));
  ok('untouched diagram node reused', document.querySelector('.diagram') === firstDiagram && firstDiagram.isConnected);
  ok('heading node reused', document.querySelector('#preview h1') === h1);
  ok('tab marked dirty', document.querySelector('.tab.active').classList.contains('dirty'));

  // insert near the top: nodes below must be reused and shifted
  const before = firstDiagram.getAttribute('data-sourcepos');
  cm.replaceRange('Inserted line one.\n\nInserted line two.\n\n', { line: 6, ch: 0 });
  await wait(700);
  const after = firstDiagram.getAttribute('data-sourcepos');
  ok('suffix reused + sourcepos shifted by 4', firstDiagram.isConnected && parseInt(after) === parseInt(before) + 4, `${before} => ${after}`);

  // --- formatting
  const lineNo = cm.lastLine();
  cm.replaceRange('\nhello world', { line: lineNo, ch: cm.getLine(lineNo).length });
  const L = cm.lastLine();
  cm.setSelection({ line: L, ch: 0 }, { line: L, ch: 5 });
  Editor.toggleWrap('**');
  ok('bold on', cm.getLine(L) === '**hello** world', cm.getLine(L));
  Editor.toggleWrap('**');
  ok('bold off', cm.getLine(L) === 'hello world', cm.getLine(L));
  Editor.toggleWrap('*');
  ok('italic on', cm.getLine(L) === '*hello* world', cm.getLine(L));
  Editor.toggleWrap('**');
  ok('bold over italic', cm.getLine(L) === '***hello*** world', cm.getLine(L));
  cm.setCursor({ line: L, ch: 3 });
  Editor.toggleLinePrefix('heading', 2);
  ok('heading on', cm.getLine(L).startsWith('## '), cm.getLine(L));
  Editor.toggleLinePrefix('heading', 3);
  ok('heading level change', cm.getLine(L).startsWith('### '), cm.getLine(L));
  Editor.toggleLinePrefix('heading', 3);
  ok('heading off', !cm.getLine(L).startsWith('#'), cm.getLine(L));
  Editor.toggleLinePrefix('task');
  ok('task on', cm.getLine(L).startsWith('- [ ] '), cm.getLine(L));
  Editor.toggleLinePrefix('ul');
  ok('task -> bullet', /^- (?!\[)/.test(cm.getLine(L)), cm.getLine(L));
  Editor.toggleLinePrefix('ul');
  ok('bullet off', !cm.getLine(L).startsWith('-'), cm.getLine(L));
  cm.setCursor({ line: L, ch: cm.getLine(L).length });
  Editor.table(2, 3);
  const tblLine = cm.getCursor().line;
  ok('table inserted', cm.getLine(tblLine).startsWith('| Column 1 | Column 2 | Column 3 |'), cm.getLine(tblLine));
  cm.replaceRange('| a very long first cell | b | c |', { line: tblLine + 2, ch: 0 }, { line: tblLine + 2, ch: cm.getLine(tblLine + 2).length });
  cm.setCursor({ line: tblLine + 2, ch: 3 });
  ok('format table', Editor.formatTable() && cm.getLine(tblLine).length === cm.getLine(tblLine + 2).length, cm.getLine(tblLine) + ' / ' + cm.getLine(tblLine + 2));
  cm.setCursor({ line: cm.lastLine(), ch: cm.getLine(cm.lastLine()).length });
  Editor.insertBlock('```mermaid\ngraph TD\n  X-->Y\n```');
  await wait(1200);
  const diagrams = document.querySelectorAll('.diagram');
  ok('inserted mermaid rendered', diagrams[diagrams.length - 1].querySelector('svg') && !diagrams[diagrams.length - 1].querySelector('.diagram-error'));
  // break it: error shown, stale drawing kept
  const last = cm.lastLine();
  let fenceLine = last; while (!cm.getLine(fenceLine).includes('X-->Y')) fenceLine--;
  cm.replaceRange('  X--', { line: fenceLine, ch: 0 }, { line: fenceLine, ch: cm.getLine(fenceLine).length });
  await wait(1200);
  const broken = [...document.querySelectorAll('.diagram')].pop();
  ok('broken diagram shows error + keeps stale svg', !!broken.querySelector('.diagram-error') && !!broken.querySelector('svg'), broken.innerHTML.slice(0, 200));
  cm.replaceRange('  X-->Z', { line: fenceLine, ch: 0 }, { line: fenceLine, ch: cm.getLine(fenceLine).length });
  await wait(1200);
  const fixed = [...document.querySelectorAll('.diagram')].pop();
  ok('diagram recovers', !fixed.querySelector('.diagram-error') && fixed.textContent.includes('Z'));

  // --- task checkbox writes back to the source
  const cb = document.querySelector('#preview input[type=checkbox]:not(:checked)');
  const cbLine = parseInt(cb.closest('[data-sourcepos]').getAttribute('data-sourcepos')) - 1;
  cb.click();
  await wait(100);
  ok('task toggle updates source', /\[x\]/.test(cm.getLine(cbLine)), cm.getLine(cbLine));

  // --- find / replace in editor
  ok('editor find count', Editor.find('Inserted line', { matchCase: false }) === 2, String(Editor.findCount));
  Editor.findStep(1);
  ok('find selects', cm.getSelection().toLowerCase() === 'inserted line', cm.getSelection());
  Editor.find('inserted (line) (\\w+)', { regex: true });
  Editor.findStep(1);
  Editor.replaceOne('$2 $1', false);
  ok('regex replace with groups', cm.getValue().includes('one line.') || cm.getValue().includes('two line.'));
  Editor.find('Inserted line', {});
  ok('replace all', Editor.replaceAll('Replaced', true) === 1 && !cm.getValue().includes('Inserted line'));
  Editor.clearFind();

  // --- scroll sync
  await wait(500);
  let target = 0; while (!cm.getLine(target).startsWith('## Diagrams')) target++;
  Editor.scrollToLine(target + 1);
  await wait(500);
  const hd = [...document.querySelectorAll('#preview h2')].find((x) => x.textContent.trim() === 'Diagrams');
  const dy = hd.getBoundingClientRect().top - Preview.pane.getBoundingClientRect().top;
  ok('preview follows editor scroll', Math.abs(dy) < 60, 'offset ' + dy);

  // --- reader find
  click('button[data-mode=read]');
  await wait(400);
  const n = Preview.find('diagram', {});
  ok('preview find', n >= 3 && Preview.findStep(1) >= 0 && CSS.highlights.has('ink-find-current'), String(n));
  Preview.clearFind();

  // --- outline
  click('#btn-sidebar');
  await wait(200);
  if (document.body.dataset.sidebar !== 'on') { click('#btn-sidebar'); await wait(200); }
  ok('outline lists headings', document.querySelectorAll('.outline-item').length >= 8, String(document.querySelectorAll('.outline-item').length));

  // --- export
  const html = await Preview.exportHtml('Test');
  ok('export html', html.includes('<article class="markdown-body') && html.includes('<svg') && !html.includes('data-sourcepos') && !html.includes('/__file/') && !html.includes('block-tools"'), String(html.length));

  // --- sanitising
  const d = ink.activeDoc();
  d.cm.replaceRange('\n\n<img src=x onerror="window.__pwned=1"><script>window.__pwned=2</script>\n\n[bad](javascript:window.__pwned=3)\n', { line: d.cm.lastLine(), ch: d.cm.getLine(d.cm.lastLine()).length });
  await wait(900);
  ok('scripts and handlers stripped', !window.__pwned && !document.querySelector('#preview script') && !document.querySelector('#preview [onerror]') && ![...document.querySelectorAll('#preview a')].some((a) => /^javascript:/i.test(a.getAttribute('href') || '')));

  // --- image resolution
  const img = document.querySelector('#preview img[data-src="icon.png"]');
  await wait(300);
  ok('relative image served from disk', img && img.src.includes('/__file/') && img.naturalWidth === 256, img ? img.src + ' ' + img.naturalWidth : 'none');

  return out.join('\n');
})()
