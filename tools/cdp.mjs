// Dev helper: drive a running Inkdown (started with INKDOWN_DEBUG_PORT) over the
// Chrome DevTools Protocol on 127.0.0.1.
//   node tools/cdp.mjs reload            reload the page, print console errors
//   node tools/cdp.mjs eval "<js>"       evaluate (awaits promises), print result
//   node tools/cdp.mjs shot <file.png>   screenshot
//   node tools/cdp.mjs key <combo>       e.g. ctrl+e
const port = process.env.INKDOWN_DEBUG_PORT || '9333';
const [cmd, ...rest] = process.argv.slice(2);

const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === 'page');
if (!page) throw new Error('no page target');
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
let seq = 0;
const pending = new Map();
const events = [];
ws.onmessage = (m) => {
  const msg = JSON.parse(m.data);
  if (msg.id) { pending.get(msg.id)?.(msg); pending.delete(msg.id); } else events.push(msg);
};
const send = (method, params = {}) => new Promise((resolve) => {
  const id = ++seq;
  pending.set(id, resolve);
  ws.send(JSON.stringify({ id, method, params }));
});
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const evaluate = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true, userGesture: true });
  if (r.result.exceptionDetails) return 'EXCEPTION: ' + (r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text);
  return r.result.result.value;
};

if (cmd === 'reload') {
  await send('Runtime.enable');
  await send('Log.enable');
  await send('Page.enable');
  await send('Page.reload', { ignoreCache: true });
  await sleep(Number(rest[0]) || 3500);
  for (const e of events) {
    if (e.method === 'Runtime.exceptionThrown') console.log('EXCEPTION', e.params.exceptionDetails.exception?.description || e.params.exceptionDetails.text, e.params.exceptionDetails.url || '', e.params.exceptionDetails.lineNumber);
    else if (e.method === 'Runtime.consoleAPICalled' && ['error', 'warning'].includes(e.params.type)) console.log(e.params.type.toUpperCase(), e.params.args.map((a) => a.value ?? a.description).join(' '));
    else if (e.method === 'Log.entryAdded' && ['error', 'warning'].includes(e.params.entry.level)) console.log('LOG', e.params.entry.level, e.params.entry.text, e.params.entry.url || '');
  }
  console.log('reload done');
} else if (cmd === 'eval') {
  const v = await evaluate(rest.join(' '));
  console.log(typeof v === 'string' ? v : JSON.stringify(v, null, 1));
} else if (cmd === 'shot') {
  const r = await send('Page.captureScreenshot', { format: 'png' });
  (await import('node:fs')).writeFileSync(rest[0], Buffer.from(r.result.data, 'base64'));
  console.log('saved', rest[0]);
} else if (cmd === 'key') {
  const parts = rest[0].toLowerCase().split('+');
  const key = parts.pop();
  const modifiers = (parts.includes('alt') ? 1 : 0) | (parts.includes('ctrl') ? 2 : 0) | (parts.includes('shift') ? 8 : 0);
  const code = /^[a-z]$/.test(key) ? 'Key' + key.toUpperCase() : /^\d$/.test(key) ? 'Digit' + key : key;
  const vk = /^[a-z]$/.test(key) ? key.toUpperCase().charCodeAt(0) : /^\d$/.test(key) ? key.charCodeAt(0) : 0;
  for (const type of ['rawKeyDown', 'keyUp']) await send('Input.dispatchKeyEvent', { type, modifiers, code, key, windowsVirtualKeyCode: vk });
  console.log('sent', rest[0]);
}
ws.close();
