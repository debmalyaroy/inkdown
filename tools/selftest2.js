(async () => {
  const out = [];
  const ok = (name, cond, extra = '') => out.push((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : '  -> ' + String(extra).slice(0, 300)));
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const click = (sel) => document.querySelector(sel).click();
  const doc = ink.activeDoc();
  const src = () => doc.cm.getValue();
  if (document.body.dataset.mode !== 'write') { click('button[data-mode=write]'); await wait(1500); }
  const ed = Rich.editor;
  const original = src();

  // typing + formatting
  ed.chain().focus('end').insertContent('<p>Rich paragraph alpha beta</p>').run();
  await wait(500);
  ok('edit syncs to markdown', src().includes('Rich paragraph alpha beta') && src() !== original);
  ok('dirty after rich edit', document.querySelector('.tab.active').classList.contains('dirty'));
  const end = ed.state.doc.content.size;
  ed.chain().setTextSelection({ from: end - 5, to: end - 1 }).run();
  document.querySelector('#toolbar [data-cmd=bold]').click();
  await wait(450);
  ok('bold via toolbar', src().includes('alpha **beta**'), src().slice(-80));
  ok('toolbar shows bold active', document.querySelector('#toolbar [data-cmd=bold]').classList.contains('on'));
  document.querySelector('#toolbar [data-cmd=italic]').click();
  await wait(450);
  ok('bold+italic nesting', src().includes('alpha ***beta***'), src().slice(-80));
  Rich.exec('heading', 2);
  await wait(450);
  ok('heading via command', /\n## Rich paragraph alpha \*\*\*beta\*\*\*\n/.test(src()), src().slice(-80));
  ok('style label', document.querySelector('.style-drop span').textContent === 'Heading 2', document.querySelector('.style-drop span').textContent);
  Rich.exec('paragraph');
  Rich.exec('ul');
  await wait(450);
  ok('bullet list', src().includes('- Rich paragraph alpha ***beta***'), src().slice(-80));
  Rich.exec('ul');

  // table
  ed.chain().focus('end').run();
  Rich.exec('table', { rows: 2, cols: 3 });
  await wait(450);
  ok('table inserted', /\|\s+\|\s+\|\s+\|\n\| -+ \| -+ \| -+ \|/.test(src()), src().slice(-200));
  ok('table tools appear', !document.querySelector('.icon-btn.accent').hidden);
  ed.chain().insertContent('Name').run();
  Rich.exec('align', 'center');
  Rich.exec('rowBelow');
  await wait(450);
  ok('cell text + centre align + row added', /\| Name \|/.test(src()) && /\| :--: \|/.test(src()) && src().trimEnd().split('\n').slice(-5).filter((l) => l.startsWith('|')).length === 5, src().slice(-260));

  // diagram via Insert
  ed.chain().insertContentAt(ed.state.doc.content.size, { type: 'paragraph' }).focus('end').run();
  await Rich.insertMarkdown('```mermaid\ngraph LR\n  P-->Q\n```');
  await wait(1500);
  const boxes = document.querySelectorAll('.rich-code.has-preview');
  ok('inserted diagram is drawn live', boxes[boxes.length - 1].querySelector('svg') && src().includes('P-->Q'));

  // undo
  const before = src();
  Rich.exec('undo');
  await wait(450);
  ok('undo in write mode', src() !== before && !src().includes('P-->Q'));
  Rich.exec('redo');
  await wait(450);

  // escaping
  ed.chain().focus('end').insertContent({ type: 'paragraph', content: [{ type: 'text', text: '2 * 3 = 6, snake_case, a_b_ and <tag> # not heading' }] }).run();
  await wait(450);
  ok('special characters escaped', src().includes('2 \\* 3 = 6, snake_case, a_b\\_ and \\<tag> # not heading'), src().slice(-90));

  // find in write mode
  ok('find in write mode', Rich.find('diagram', {}) >= 3 && Rich.findStep(1) >= 0);
  Rich.clearFind();

  // switch to reader: preview reflects rich edits
  click('button[data-mode=read]');
  await wait(1800);
  ok('reader shows rich edits', document.querySelector('#preview').textContent.includes('snake_case, a_b_ and <tag>') && [...document.querySelectorAll('#preview .diagram')].some((d) => d._src.includes('P-->Q')));
  ok('escaped text renders literally', !document.querySelector('#preview tag'));

  // tools
  click('button[data-mode=write]');
  await wait(1200);
  const palette = async (label) => {
    const before2 = src();
    document.dispatchEvent(new KeyboardEvent('keydown', { code: 'F1', key: 'F1', bubbles: true }));
    await wait(120);
    const input = document.querySelector('.palette input');
    input.value = label; input.dispatchEvent(new Event('input'));
    await wait(60);
    document.querySelector('.palette-item').click();
    await wait(1800);
    return before2;
  };
  await palette('table of contents');
  ok('table of contents inserted', /<!-- toc -->\n- \[Writing\]\(#writing\)\n- \[Code\]\(#code\)/.test(src()), src().slice(0, 600));
  ok('toc visible in rich editor as links', !!document.querySelector('#rich a[href="#writing"]'));
  await palette('statistics');
  const stats = document.querySelector('.stats');
  ok('statistics dialog', stats && /Words/.test(stats.textContent) && /Diagrams and charts/.test(stats.textContent));
  document.querySelector('.scrim')?.remove();

  // export assets
  click('button[data-mode=read]');
  await wait(1500);
  const assets = await Exporter.collect();
  ok('diagram images', assets.diagrams.length >= 6 && assets.diagrams.every((a) => a.data.length > 800 && a.w > 50), JSON.stringify(assets.diagrams.map((a) => [a.key.slice(0, 12), a.data.length, Math.round(a.w)])));
  ok('math images', assets.math.length === 2 && assets.math.every((a) => a.data.length > 400), JSON.stringify(assets.math.map((a) => [a.key, a.data.length, a.w, a.h, a.depth])));
  ok('local image', assets.images.some((a) => a.key === 'icon.png' && a.data.length > 1000), JSON.stringify(assets.images.map((a) => [a.key, a.data.length])));
  window.__assets = assets;
  const dir = doc.dir;
  await host.invoke('export', { format: 'docx', path: dir + '\\out.docx', text: src(), assets, title: 'Tour' });
  await host.invoke('export', { format: 'rtf', path: dir + '\\out.rtf', text: src(), assets, title: 'Tour' });
  ok('docx + rtf written', await host.invoke('exists', { path: dir + '\\out.docx' }) && await host.invoke('exists', { path: dir + '\\out.rtf' }));
  const html = await Exporter.richHtml(assets);
  ok('rich clipboard html', html.includes('data:image/png;base64') && html.includes('<table style=') && !html.includes('class=') && !html.includes('data-sourcepos'), html.length);
  await host.invoke('save', { path: dir + '\\clip.html', text: html, crlf: false });
  return out.join('\n');
})()
