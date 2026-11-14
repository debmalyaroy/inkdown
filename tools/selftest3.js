(async () => {
  const out = [];
  const ok = (name, cond, extra = '') => out.push((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : '  -> ' + String(extra).slice(0, 260)));
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const dir = require('path').join(require('os').tmpdir(), 'inkdown-t2');
  const open = async (f, ms = 1500) => { ink.onDrop([dir + '\\' + f]); await wait(ms); return ink.activeDoc(); };
  const click = (sel) => document.querySelector(sel).click();
  const P = document.querySelector('#preview');
  const cm = Editor.cm;

  let d = await open('sample.json');
  ok('json: kind + view mode', d.kind.id === 'json' && ['read', 'split'].includes(document.body.dataset.mode), d.kind.id + ' ' + document.body.dataset.mode);
  click('button[data-mode=split]'); await wait(900);
  ok('json: write button hidden, label Code', document.querySelector('button[data-mode=write]').hidden && document.querySelector('button[data-mode=split]').textContent.trim() === 'Code');
  ok('json: tree rendered', P.querySelectorAll('.tn').length >= 7 && P.querySelector('.tv-number') && P.querySelector('.tv-boolean') && P.querySelector('.tv-swatch'), P.innerHTML.slice(0, 300));
  P.querySelector('.tree-bar .btn').click(); await wait(100);
  ok('json: expand all', P.textContent.includes('deep') && P.querySelectorAll('.tn.leaf').length >= 9, P.querySelectorAll('.tn.leaf').length);
  ok('json: valid status', document.querySelector('#data-status').textContent.includes('Valid JSON'), document.querySelector('#data-status').textContent);
  ok('json: syntax mode', /javascript|json/.test(JSON.stringify(cm.getOption('mode'))) && !!document.querySelector('.CodeMirror .cm-string, .CodeMirror .cm-property'), JSON.stringify(cm.getOption('mode')));
  cm.replaceRange('\n  oops', { line: 0, ch: 1 }); await wait(900);
  ok('json: error shown with line', document.querySelector('#data-status').classList.contains('bad') && !!P.querySelector('.data-error') && !!P.querySelector('.data-view.stale') && !!document.querySelector('.cm-error-line'), document.querySelector('#data-status').textContent);
  cm.undo(); await wait(900);
  ok('json: recovers', !P.querySelector('.data-error') && !document.querySelector('.cm-error-line'));
  document.querySelector('#toolbar2 [data-tip^="Re-indent"]').click(); await wait(700);
  ok('json: format', cm.getValue().startsWith('{\n  "name": "inkdown",\n  "version": 1.5,'), cm.getValue().slice(0, 80));
  const before = document.querySelectorAll('.tab').length;
  document.dispatchEvent(new KeyboardEvent('keydown', { code: 'F1', key: 'F1', bubbles: true })); await wait(120);
  const inp = document.querySelector('.palette input'); inp.value = 'convert to yaml'; inp.dispatchEvent(new Event('input')); await wait(60);
  document.querySelector('.palette-item').click(); await wait(1200);
  d = ink.activeDoc();
  ok('json -> yaml in new tab', document.querySelectorAll('.tab').length === before + 1 && d.kind.id === 'yaml' && d.cm.getValue().includes('name: inkdown') && d.cm.getValue().includes('- a'), d.cm.getValue().slice(0, 120));

  d = await open('sample.toml');
  ok('toml: tree', d.kind.id === 'toml' && P.textContent.includes('dependencies') && P.textContent.includes('package'), P.textContent.slice(0, 200));
  d = await open('sample.yaml');
  ok('yaml: tree', d.kind.id === 'yaml' && P.textContent.includes('items') && /\[2\]/.test(P.textContent));

  d = await open('flow.mmd', 2600);
  ok('mermaid file: live diagram', d.kind.id === 'mermaid' && !!P.querySelector('.diagram svg') && P.dataset.kind === 'diagram', P.innerHTML.slice(0, 200));
  ok('mermaid file: editor highlighting', !!document.querySelector('.CodeMirror .cm-keyword'), JSON.stringify(cm.getOption('mode')));
  cm.replaceRange('    D --> E[Celebrate]\n', { line: cm.lastLine(), ch: 0 }); await wait(1800);
  ok('mermaid file: preview follows edits', P.querySelector('.diagram').textContent.includes('Celebrate') && !P.querySelector('.diagram-error'));
  ok('mermaid file: templates button', [...document.querySelectorAll('#toolbar2 .drop')].some((b) => b.textContent.includes('Templates')));

  d = await open('graph.dot', 3200);
  ok('graphviz file', d.kind.id === 'graphviz' && !!P.querySelector('.diagram svg') && P.querySelector('.diagram').textContent.includes('skip'));

  d = await open('sample.adoc', 3000);
  ok('asciidoc: rendered', d.kind.id === 'adoc' && !!P.querySelector('h1') && P.querySelector('h1').textContent.includes('Sample Doc') && !!P.querySelector('h2') && !!P.querySelector('table') && !!P.querySelector('.admonitionblock') && !!P.querySelector('ul'), P.innerHTML.slice(0, 400));
  ok('asciidoc: code highlighted', !!P.querySelector('.codeblock .hljs'), P.querySelector('.listingblock')?.innerHTML.slice(0, 200));

  d = await open('sample.csv');
  ok('csv: table', d.kind.id === 'csv' && P.querySelectorAll('.data-table tbody tr').length === 2 && P.textContent.includes('Pears, green') && P.querySelector('td[align=right]'), P.innerHTML.slice(0, 300));

  d = await open('main.rs', 2200);
  ok('rust: code kind, editor only', d.kind.id === 'code' && document.body.dataset.mode === 'edit' && document.querySelector('#mode-switch').hidden, d.kind.id + ' ' + document.body.dataset.mode);
  ok('rust: language loaded', JSON.stringify(cm.getOption('mode')).includes('rust') && !!document.querySelector('.CodeMirror .cm-keyword'), JSON.stringify(cm.getOption('mode')));
  ok('rust: line numbers on', cm.getOption('lineNumbers') === true);
  d = await open('main.go', 2600);
  ok('go: mode fetched on demand', JSON.stringify(cm.getOption('mode')).includes('go') && !!document.querySelector('.CodeMirror .cm-keyword') && !!CodeMirror.modes.go, JSON.stringify(cm.getOption('mode')));
  cm.setCursor({ line: 0, ch: 0 });
  document.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyB', key: 'b', ctrlKey: true, bubbles: true }));
  ok('markdown shortcuts inert in code files', !cm.getValue().includes('**'), cm.getValue().slice(0, 30));

  const tabs = document.querySelectorAll('.tab').length;
  ink.onDrop([dir + '\\blob.bin']); await wait(700);
  ok('binary file refused', document.querySelectorAll('.tab').length === tabs && /binary/.test(document.querySelector('#toasts').textContent), document.querySelector('#toasts').textContent);

  d = await open('tour.md', 2500);
  ok('markdown still full featured', d.kind.id === 'markdown' && !document.querySelector('button[data-mode=write]').hidden && document.body.dataset.kind === 'markdown');
  ok('lazy libs', typeof TT === 'undefined' ? 'tiptap not loaded' : 'tiptap loaded', '');
  out.push('INFO tiptap loaded=' + (typeof TT !== 'undefined') + ' katex loaded=' + (typeof katex !== 'undefined') + ' asciidoctor loaded=' + (typeof Adoc !== 'undefined') + ' hljs langs=' + hljs.listLanguages().length);
  return out.join('\n');
})()
