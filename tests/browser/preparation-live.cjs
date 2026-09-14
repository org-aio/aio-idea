const assert = require('node:assert/strict');
const { mkdir, writeFile } = require('node:fs/promises');
const { resolve } = require('node:path');
const { PNG } = require('pngjs');
const { launchBrowser, contextFor, closeContext, closeBrowser, getJson, select, viewportScreenshot } = require('./live-session.cjs');

const base = process.env.AIO_URL || 'https://aio.addzero.site';
const baseline = process.argv.includes('baseline');
const output = resolve('target/preparation-test/public');
const redact = value => value.replace(/\/(frontend|components)\/(assets\/)?[^/\s]+\//g, '/$1/[ticket]/');

async function rolling(browser) {
  const context = await contextFor(browser, base, false);
  try {
    const page = await context.newPage();
    await page.goto(base, { waitUntil: 'domcontentloaded', timeout: 120000 });
    await page.locator('.application-shell:visible').waitFor();
    const fingerprint = html => html.match(/aio-idea_bg-dxh[a-f0-9]+\.wasm/)?.[0];
    const initial = fingerprint(await page.content());
    assert(initial);
    console.log(JSON.stringify({ stage: 'old-shell-ready', initial }));
    const deadline = Date.now() + 900000;
    let current = initial;
    while (current === initial && Date.now() < deadline) {
      await page.waitForTimeout(5000);
      const response = await context.request.get(base, { timeout: 20000 }).catch(() => null);
      if (response?.ok()) current = fingerprint(await response.text()) || initial;
    }
    assert.notEqual(current, initial, 'No new host deployment detected');
    const catalog = (await getJson(context, `${base}/api/runtime/catalog`)).data;
    const plugin = catalog.pages.find(item => item.id === 'kmp-fullstack');
    await page.getByRole('navigation', { name: '场景' }).getByRole('button', { name: '社区插件', exact: true }).click();
    await select(page, false, plugin.label);
    const frame = page.frameLocator(`iframe[title="${plugin.label}"]`);
    await frame.getByRole('button', { name: 'Counter', exact: true }).waitFor({ timeout: 120000 });
    const backend = await frame.locator('body').evaluate(() => window.aioPlugin.json('GET', '/tasks'));
    assert(Array.isArray(backend.items));
    assert.equal(fingerprint(await page.content()), initial, 'The browser must keep its original shell');
    await viewportScreenshot(page, resolve(output, 'rolling.png'));
    await writeFile(resolve(output, 'rolling.json'), JSON.stringify({ initial, current, reloaded: false, realBackend: true }, null, 2));
    console.log('Existing shell opened a plugin and called the new host without reloading');
  } finally { await closeContext(context); }
}

async function run(browser, mobile) {
  const context = await contextFor(browser, base, mobile);
  const catalog = (await getJson(context, `${base}/api/runtime/catalog`)).data;
  const plugin = catalog.pages.find(item => item.id === 'kmp-fullstack');
  assert(plugin);
  await context.addInitScript(id => { if (window === top) sessionStorage.setItem('aio-plugin-recent', JSON.stringify([id])); }, plugin.id);
  const page = await context.newPage();
  page.setDefaultTimeout(60000);
  const frame = page.frameLocator(`iframe[title="${plugin.label}"]`);
  const iframe = page.locator(`iframe[title="${plugin.label}"]`);
  const errors = [], mounts = [], released = new Set(), measurements = [];
  let backgroundBusiness = 0, warming = true, wasmDownloads = 0;
  page.on('pageerror', error => errors.push(redact(error.message)));
  page.on('request', request => {
    const pathname = new URL(request.url()).pathname;
    if (/\/assets\/.*\.wasm$/.test(pathname)) wasmDownloads++;
    if (warming && /\/(frontend|components)\/[^/]+\/request$/.test(pathname)) backgroundBusiness++;
    if (request.method() === 'DELETE' && pathname.startsWith('/api/runtime/frontend/')) released.add(pathname.split('/').at(-1));
  });
  page.on('response', response => {
    if (response.url().endsWith('/api/runtime/frontend/mount') && response.ok()) {
      void response.json().then(({ data }) => mounts.push({ page: response.request().postDataJSON().page_id, token: data.token })).catch(() => {});
    }
  });
  try {
    for (const mode of ['first', 'refresh']) {
      warming = true;
      const mountStart = mounts.length, businessStart = backgroundBusiness;
      await page.goto(base, { waitUntil: 'domcontentloaded', timeout: 120000 });
      await page.locator('.application-shell:visible').waitFor();
      console.log(JSON.stringify({ viewport: mobile ? 'mobile' : 'desktop', mode, stage: 'shell' }));
      const started = Date.now();
      if (baseline) {
        const deadline = Date.now() + 240000;
        while (!mounts.slice(mountStart).some(item => item.page === plugin.id && released.has(item.token)) && Date.now() < deadline) await page.waitForTimeout(500);
        assert(mounts.slice(mountStart).some(item => item.page === plugin.id && released.has(item.token)), 'Byte warming did not finish');
      } else {
        await iframe.locator('..').waitFor({ state: 'attached', timeout: 240000 });
        await page.waitForFunction(label => document.querySelector(`iframe[title="${label}"]`)?.dataset.aioPrepared === 'true', plugin.label, { timeout: 120000 });
      }
      const preparedMs = Date.now() - started;
      console.log(JSON.stringify({ viewport: mobile ? 'mobile' : 'desktop', mode, stage: 'prepared', preparedMs }));
      assert.equal(backgroundBusiness - businessStart, 0);
      const marker = baseline ? null : await frame.locator('body').evaluate(() => window.__preparedMarker = Math.random());
      const mountCount = mounts.filter(item => item.page === plugin.id).length;
      const beforeDownloads = wasmDownloads;
      warming = false;
      const opened = Date.now();
      await page.getByRole('navigation', { name: '场景' }).getByRole('button', { name: '社区插件', exact: true }).click();
      await select(page, mobile, plugin.label);
      console.log(JSON.stringify({ viewport: mobile ? 'mobile' : 'desktop', mode, stage: 'selected' }));
      await frame.getByRole('button', { name: 'Counter', exact: true }).waitFor();
      await page.waitForTimeout(400);
      await frame.getByRole('button', { name: 'Counter', exact: true }).click({ force: true });
      await frame.getByRole('button', { name: '+1', exact: true }).waitFor();
      const interactiveMs = Date.now() - opened;
      assert.equal(await iframe.evaluate(node => node.closest('[data-aio-page]').inert), false);
      if (!baseline) {
        assert.equal(await frame.locator('body').evaluate(() => window.__preparedMarker), marker);
        assert.equal(mounts.filter(item => item.page === plugin.id).length, mountCount);
      }
      assert.equal(wasmDownloads - beforeDownloads, 0, 'Opening repeated a warmed Wasm download');
      const backend = await frame.locator('body').evaluate(() => window.aioPlugin.json('GET', '/tasks'));
      assert(Array.isArray(backend.items), 'Real Ktor backend response missing tasks');
      await page.waitForTimeout(400);
      const canvas = frame.locator('canvas').first();
      const initial = PNG.sync.read(await canvas.screenshot());
      const button = await frame.getByRole('button', { name: '+1', exact: true }).boundingBox();
      await page.mouse.click(button.x + button.width / 2, button.y + button.height / 2);
      await frame.getByText('1', { exact: true }).waitFor();
      const changed = PNG.sync.read(await canvas.screenshot());
      let pixels = 0;
      for (let i = 0; i < initial.data.length; i += 4) if (initial.data.readUInt32BE(i) !== changed.data.readUInt32BE(i)) pixels++;
      assert(pixels > 30);
      const returning = Date.now();
      await page.getByRole('navigation', { name: '场景' }).getByRole('button', { name: '系统', exact: true }).click();
      await page.getByRole('navigation', { name: '场景' }).getByRole('button', { name: '社区插件', exact: true }).click();
      await select(page, mobile, plugin.label);
      await frame.getByText('1', { exact: true }).waitFor();
      const returnMs = Date.now() - returning;
      assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      await viewportScreenshot(page, resolve(output, `${baseline ? 'baseline' : 'prepared'}-${mobile ? 'mobile' : 'desktop'}-${mode}.png`));
      const result = { mode, preparedMs, interactiveMs, returnMs, changedCanvasPixels: pixels, backgroundBusiness: 0, newMountsOnOpen: mounts.filter(item => item.page === plugin.id).length - mountCount, repeatedWasmDownloads: 0, realBackend: true };
      measurements.push(result);
      console.log(JSON.stringify({ viewport: mobile ? 'mobile' : 'desktop', ...result }));
    }
    assert.deepEqual(errors, []);
    return { viewport: mobile ? 'mobile' : 'desktop', version: catalog.page_versions[plugin.id], measurements, errors };
  } catch (error) {
    console.error(JSON.stringify({ mounts: mounts.map(item => ({ page: item.page, released: released.has(item.token) })), backgroundBusiness, wasmDownloads, errors }));
    await viewportScreenshot(page, resolve(output, 'failure.png')).catch(() => {});
    throw new Error(redact(error.message.split('Call log:')[0]));
  } finally { await closeContext(context); }
}

(async () => {
  await mkdir(output, { recursive: true });
  const browser = await launchBrowser();
  try {
    if (process.argv.includes('rolling')) return await rolling(browser);
    const results = [];
    for (const mobile of [false, true]) results.push(await run(browser, mobile));
    await writeFile(resolve(output, `${baseline ? 'baseline' : 'prepared'}.json`), JSON.stringify({ base, baseline, proxy: Boolean(process.env.AIO_BROWSER_PROXY), results }, null, 2));
  } finally { await closeBrowser(browser); }
})().catch(error => { console.error(error.message); process.exitCode = 1; });
