(async () => {
  const out = [];
  const ok = (name, cond, extra = '') => out.push((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : '  -> ' + String(extra).slice(0, 300)));
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const dz = document.querySelector('.designer');
  [...dz.querySelectorAll('.dz-bar .btn')].find((b) => b.textContent === 'Save drawing').click();
  await wait(1500);
  const src = Editor.cm.getValue();
  ok('designer closed, svg block inserted', !document.querySelector('.designer') && /```svg\n<svg [^\n]*inkdown-diagram:[^\n]*<\/svg>\n```/.test(src), src.slice(-200));
  const svgLine = src.split('\n').find((l) => l.startsWith('<svg '));
  const model = Designer.parse(svgLine);
  ok('model round-trips through the file', model && model.nodes.length === 3 && model.edges.length === 2 && model.nodes.every((n) => n.fill === '#e7f7ee'), JSON.stringify(model).slice(0, 200));
  const box = [...document.querySelectorAll('#preview .diagram[data-kind=svg]')].pop();
  ok('preview shows the drawing with an edit button', box && !!box.querySelector('.diagram-stage svg path') && !!box.querySelector('[data-act=design]') && box.querySelector('.diagram-stage svg').textContent.includes('Decision'), box ? box.innerHTML.slice(0, 200) : 'no box');
  const mm = Designer.toMermaid(model);
  ok('mermaid export', /^flowchart (LR|TD)\n    N1\(\["Start \/ end"\]\)\n    N2\{"Decision"\}\n    N3\[\("Database"\)\]\n    N1 --> N2\n    N1 --> N3\n$/.test(mm), mm);
  box.querySelector('[data-act=design]').click();
  await wait(400);
  const dz2 = document.querySelector('.designer');
  ok('reopens for editing with content', dz2 && dz2.querySelectorAll('.dz-node').length === 3 && dz2.querySelectorAll('.dz-edge').length === 2);
  document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  await wait(200);
  ok('escape closes unchanged designer', !document.querySelector('.designer'));
  return out.join('\n');
})()
