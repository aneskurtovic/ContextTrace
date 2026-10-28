// Direct W3C WebDriver against tauri-driver/WebKitWebDriver, with no app test plugin.
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

const [binary, expectedArchive, screenshot] = process.argv.slice(2);
if (!binary || !expectedArchive || !path.isAbsolute(binary) || !path.isAbsolute(expectedArchive)) {
  throw new Error('Usage: node smoke-linux-webdriver.mjs <absolute app binary> <absolute archive root> [screenshot]');
}

// Reserve two distinct ephemeral ports long enough to choose them; a bind race
// remains possible after release and is reported as a failed smoke.
async function port() {
  const server = createServer();
  await new Promise((resolve, reject) => server.once('error', reject).listen(0, '127.0.0.1', resolve));
  return { server, number: server.address().port };
}
const first = await port(), second = await port();
await Promise.all([first, second].map(({server}) => new Promise(resolve => server.close(resolve))));
const base = `http://127.0.0.1:${first.number}`;
const child = spawn('tauri-driver', ['--port', String(first.number), '--native-port', String(second.number)], {
  detached: true, stdio: 'inherit',
});
let childError;
child.on('error', error => { childError = error; });
let session;
async function request(method, route, payload, timeout = 10000) {
  const response = await fetch(`${base}${route}`, {
    method, headers: { 'content-type': 'application/json' },
    ...(payload === undefined ? {} : { body: JSON.stringify(payload) }),
    signal: AbortSignal.timeout(timeout),
  });
  const body = await response.json();
  if (!response.ok || body.value?.error) throw new Error(`${method} ${route}: ${JSON.stringify(body.value)}`);
  return body.value;
}
try {
  const deadline = Date.now() + 15000;
  while (true) {
    if (childError || child.exitCode !== null) throw childError || new Error(`tauri-driver exited: ${child.exitCode}`);
    try { await request('GET', '/status', undefined, 1000); break; }
    catch (error) { if (Date.now() >= deadline) throw new Error(`tauri-driver did not become ready: ${error.message}`); await delay(250); }
  }
  const created = await request('POST', '/session', {
    capabilities: { alwaysMatch: { 'tauri:options': { application: binary } } },
  }, 45000);
  session = created.sessionId;
  if (typeof session !== 'string' || !session) throw new Error(`Missing WebDriver session: ${JSON.stringify(created)}`);
  const route = `/session/${encodeURIComponent(session)}`;
  const pageDeadline = Date.now() + 30000;
  let page;
  do {
    page = await request('POST', `${route}/execute/sync`, {
      script: `const text = document.body?.innerText || ''; const shell = [...document.querySelectorAll('strong')].find(e => e.textContent.trim() === 'ContextTrace' && e.getBoundingClientRect().height > 0); return {ready: document.readyState, shell: !!shell, sessions: text.includes('Your sessions')};`, args: [],
    });
    if (page.shell && page.sessions) break;
    await delay(250);
  } while (Date.now() < pageDeadline);
  if (!page?.shell || !page.sessions) throw new Error(`Rendered app shell did not become ready: ${JSON.stringify(page)}`);
  const result = await request('POST', `${route}/execute/async`, {
    script: `const done = arguments[arguments.length - 1]; if (!window.__TAURI_INTERNALS__) { done({error: 'No native Tauri IPC'}); return; } window.__TAURI_INTERNALS__.invoke('get_startup').then(value => done({value}), error => done({error: String(error)}));`, args: [],
  }, 20000);
  const startup = result?.value;
  if (result?.error || !startup || startup.archiveRoot !== expectedArchive || !Array.isArray(startup.roots) || !Array.isArray(startup.warnings)) {
    throw new Error(`Native get_startup IPC failed or returned unexpected data: ${JSON.stringify(result)}`);
  }
  if (screenshot) {
    const png = await request('GET', `${route}/screenshot`);
    if (typeof png !== 'string' || !png) throw new Error('WebDriver screenshot was empty');
    await writeFile(screenshot, Buffer.from(png, 'base64'));
    console.log(`WebDriver screenshot: ${screenshot}`);
  }
  console.log('Linux WebKit rendered the ContextTrace shell and native get_startup IPC returned the isolated archive root.');
} finally {
  if (session) {
    try { await request('DELETE', `/session/${encodeURIComponent(session)}`, undefined, 5000); }
    catch (error) { console.error(`WebDriver session cleanup: ${error.message}`); }
  }
  // The detached process group belongs to this smoke and contains its driver
  // and browser children; never kill unrelated system processes.
  if (child.pid) {
    const signalGroup = signal => { try { process.kill(-child.pid, signal); } catch (error) { if (error.code !== 'ESRCH') throw error; } };
    signalGroup('SIGTERM');
    await delay(1000);
    signalGroup('SIGKILL');
  }
}
