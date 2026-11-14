(async () => {
  const out = [];
  const ok = (name, cond, extra = '') => out.push((cond ? 'PASS ' : 'FAIL ') + name + (cond ? '' : '  -> ' + String(extra).slice(0, 260)));
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const fire = (el, type, x, y, extra = {}) => el.dispatchEvent(new MouseEvent(type, { clientX: x, clientY: y, bubbles: true, cancelable: true, button: 0, ...extra }));
  const mid = (el) => { const r = el.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; };
  const doc = ink.activeDoc();
  document.querySelector('button[data-mode=split]').click(); await wait(800);
  Editor.cm.setCursor({ line: Editor.cm.lastLine(), ch: 0 });
  document.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyD', key: 'D', ctrlKey: true, shiftKey: true, bubbles: true }));
  await wait(400);
  const dz = document.querySelector('.designer');
  ok('designer opens', !!dz && dz.querySelectorAll('.dz-block').length === 11, dz ? dz.querySelectorAll('.dz-block').length : 'none');
  const blocks = [...dz.querySelectorAll('.dz-block')];
  const add = (i) => { const [x, y] = mid(blocks[i]); fire(blocks[i], 'mousedown', x, y); fire(window, 'mouseup', x, y); };
  add(2); add(3); add(7);
  await wait(100);
  ok('blocks added from palette', dz.querySelectorAll('.dz-node').length === 3, dz.querySelectorAll('.dz-node').length);
  // move the third block away by dragging
  let nodes = [...dz.querySelectorAll('.dz-node')];
  let [x, y] = mid(nodes[2]);
  fire(nodes[2], 'mousedown', x, y); fire(window, 'mousemove', x + 220, y + 120); fire(window, 'mouseup', x + 220, y + 120);
  nodes = [...dz.querySelectorAll('.dz-node')];
  [x, y] = mid(nodes[1]);
  fire(nodes[1], 'mousedown', x, y); fire(window, 'mousemove', x + 230, y - 60); fire(window, 'mouseup', x + 230, y - 60);
  await wait(60);
  nodes = [...dz.querySelectorAll('.dz-node')];
  const p0 = mid(nodes[0]), p1 = mid(nodes[1]), p2 = mid(nodes[2]);
  ok('drag moves blocks', Math.abs(p1[0] - p0[0]) > 150 && Math.abs(p2[1] - p0[1]) > 80, JSON.stringify([p0, p1, p2]));
  // connect node0 -> node1 through a port, node0 -> node2 with the connector tool
  fire(nodes[0], 'mousedown', p0[0], p0[1]); fire(window, 'mouseup', p0[0], p0[1]);
  await wait(60);
  const port = dz.querySelector('.dz-port');
  ok('selection shows handles and ports', dz.querySelectorAll('.dz-handle').length === 4 && dz.querySelectorAll('.dz-port').length === 4);
  const pp = mid(port);
  fire(port, 'mousedown', pp[0], pp[1]); fire(window, 'mousemove', p1[0], p1[1]); fire(window, 'mouseup', p1[0], p1[1]);
  await wait(60);
  document.dispatchEvent(new KeyboardEvent('keydown', { key: 'c', bubbles: true }));
  nodes = [...dz.querySelectorAll('.dz-node')];
  fire(nodes[0], 'mousedown', p0[0], p0[1]); fire(window, 'mousemove', p2[0], p2[1]); fire(window, 'mouseup', p2[0], p2[1]);
  await wait(60);
  ok('connectors created', dz.querySelectorAll('.dz-edge').length === 2, dz.querySelectorAll('.dz-edge').length);
  document.dispatchEvent(new KeyboardEvent('keydown', { key: 'z', ctrlKey: true, bubbles: true }));
  await wait(40);
  ok('undo removes last connector', dz.querySelectorAll('.dz-edge').length === 1);
  document.dispatchEvent(new KeyboardEvent('keydown', { key: 'y', ctrlKey: true, bubbles: true }));
  await wait(40);
  ok('redo restores it', dz.querySelectorAll('.dz-edge').length === 2);
  // colour the selection, then save
  document.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', ctrlKey: true, bubbles: true }));
  dz.querySelectorAll('.dz-swatches button')[2].click();
  await wait(60);
  window.__dzShot = true;
  return out.join('\n');
})()
