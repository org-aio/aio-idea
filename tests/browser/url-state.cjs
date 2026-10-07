const assert = require('node:assert/strict');
const { mkdir, readFile, writeFile } = require('node:fs/promises');
const { resolve } = require('node:path');
const { randomUUID } = require('node:crypto');
const { chromium } = require('playwright-core');

const base = process.env.AIO_URL || 'http://127.0.0.1:4183';
assert.equal(new URL(base).hostname, '127.0.0.1', '仅允许隔离测试实例');
const platform = resolve(process.env.AIO_PLATFORM_ROOT || '../aio-platform');
const output = resolve('target/url-state-test');
const errors = [];
const files = Array.from({ length: 120 }, (_, i) => ({
  id: `file-${i}`, name: `package-${String(i).padStart(3, '0')}.apk`,
  content_type: 'application/octet-stream', size_bytes: 1024 + i,
  sha256: 'a'.repeat(64), uploaded_by: 'tester', created_at: `2026-10-07T00:00:${String(i % 60).padStart(2, '0')}Z`, image_token: null,
}));
files.push({ ...files[0], id: 'document', name: 'readme.txt', content_type: 'text/plain' });
const runtimePage = abi => ({ id: `guest-${abi}`, label: `插件 ABI ${abi}`,
  scene: { id: 'community', label: '社区插件' }, menu_path: [], required_permission: null,
  body: { kind: 'frontend', entry: 'index.html' },
});
const session = { user_id: 'tester', account: 'tester', display_name: 'Tester', tenant_id: 'test', tenant_label: 'Test', permissions: ['file:manage'] };
const catalog = { session_context: 'login-a', context: 'session-a', tenant: { id: 'test', label: 'Test' },
  user: { label: 'Tester', handle: '@tester', initials: 'T' }, plugins: [],
  pages: [runtimePage(1), runtimePage(2)], page_versions: { 'guest-1': 'v1', 'guest-2': 'v2' }, account_items: [],
};

async function fixtures(context) {
  const grants = new Map();
  const sources = await Promise.all(['sdk/web/lifecycle.js', 'sdk/web/navigation.js',
    'lib/plugin/host/src/runtime/server/frontend_guest.js', 'sdk/web/guest.js'].map(name => readFile(resolve(platform, name), 'utf8')));
  await context.route('**/api/**', async route => {
    const path = new URL(route.request().url()).pathname;
    const json = data => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ data }) });
    if (path === '/api/runtime/bootstrap') return json({ catalog, permissions: session.permissions });
    if (path === '/api/auth/session') return json(session);
    if (path === '/api/runtime/catalog') return json(catalog);
    if (path === '/api/files') return json({ files, max_file_bytes: 10485760 });
    if (path === '/api/runtime/components/bridge.js') return route.fulfill({ contentType: 'text/javascript', body: await readFile(resolve(platform, 'sdk/web/host.mjs'), 'utf8') });
    if (path === '/api/runtime/frontend/mount') {
      const { page_id } = route.request().postDataJSON();
      const abi = Number(page_id.split('-')[1]);
      const token = randomUUID().replaceAll('-', '');
      grants.set(token, abi);
      return json({ abi, token, src: `/api/runtime/${abi === 2 ? 'components' : 'frontend'}/assets/${token}/index.html`,
        revision: `v${abi}`, generation: `v${abi}`, session_context: catalog.session_context, context: catalog.context, assets: {},
      });
    }
    const asset = path.match(/^\/api\/runtime\/(?:components|frontend)\/assets\/([^/]+)\/(index.html|guest.js)$/);
    if (asset) {
      const token = asset[1], abi = grants.get(token);
      assert(abi, '未知挂载');
      const bridge = [sources[0], sources[1], sources[abi === 2 ? 3 : 2]].join('\n');
      if (asset[2] === 'guest.js') return route.fulfill({ contentType: 'text/javascript', body: bridge });
      return route.fulfill({ contentType: 'text/html', body: `<!doctype html><html><head><script src="guest.js" data-token="${token}"></script></head><body>
        <button id="filter">Filter open</button><button id="details">Details</button><p id="route"></p>
        <div data-url-scroll="items" style="height:200px;overflow:auto"><div style="height:2500px">Items</div></div>
        <script>aioPlugin.onNavigationChange(value => document.getElementById('route').textContent = value);
        document.getElementById('filter').onclick = () => aioPlugin.navigate('#/items?status=open');
        document.getElementById('details').onclick = () => { location.hash = '/details?tab=history'; };</script></body></html>` });
    }
    if (path.endsWith('/renew') || route.request().method() === 'DELETE') return json(null);
    return route.fulfill({ status: 404, contentType: 'application/json', body: JSON.stringify({ error: `未提供夹具: ${path}` }) });
  });
}

async function contextFor(browser, mobile) {
  const context = await browser.newContext({ viewport: mobile ? { width: 390, height: 844 } : { width: 1440, height: 900 }, isMobile: mobile, hasTouch: mobile });
  await context.addInitScript(() => {
    window.__navigationProbe = [];
    addEventListener('message', event => {
      if (event.data?.channel !== 'aio-navigation') return;
      const frame = document.querySelector('[data-aio-page-active="true"] iframe');
      window.__navigationProbe.push({ origin: event.origin, owns: event.source === frame?.contentWindow,
        controller: Boolean(window.__adminUrlState), page: frame?.closest('[data-aio-page-active]')?.dataset.aioPage,
        active: frame?.closest('[data-aio-page-active]')?.dataset.aioPageActive });
    });
  });
  await fixtures(context);
  return context;
}
function watch(page) {
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
}
async function parameter(page, name, value) {
  await page.waitForFunction(({ name, value }) => new URL(location.href).searchParams.get(name) === value, { name, value });
}
async function menu(page, name, mobile) {
  if (mobile) await page.getByRole('button', { name: '打开菜单', exact: true }).click();
  const nav = mobile ? page.getByRole('dialog') : page.locator('.application-shell__sidebar');
  await nav.getByRole('button', { name, exact: true }).click();
}
async function assertFiles(page) {
  await page.getByRole('heading', { name: '文件管理', exact: true }).waitFor();
  await page.getByRole('searchbox', { name: '搜索文件' }).waitFor();
  await page.getByRole('listbox', { name: '文件分类' }).waitFor();
}
async function guestScroll(frame, y) {
  await frame.locator('[data-url-scroll="items"]').evaluate((element, y) => new Promise((resolve, reject) => {
    const deadline = performance.now() + 5000;
    const check = () => {
      if (element.scrollTop === y) return resolve();
      if (performance.now() > deadline) return reject(new Error(`滚动恢复失败: ${element.scrollTop}, 期望 ${y}`));
      requestAnimationFrame(check);
    };
    check();
  }), y);
}
async function filesScenario(browser, mobile) {
  const context = await contextFor(browser, mobile);
  const page = await context.newPage(); watch(page);
  try {
    await page.goto(`${base}/?page=file-list&external=x&external=y`);
    await assertFiles(page);
    const categories = page.getByRole('listbox', { name: '文件分类' });
    await categories.getByRole('option', { name: /安装包/ }).click();
    await parameter(page, 'view.category', 'application');
    const search = page.getByRole('searchbox', { name: '搜索文件' });
    const before = await page.evaluate(() => history.length);
    await search.pressSequentially('package', { delay: 30 });
    await parameter(page, 'view.q', 'package');
    assert.equal(await page.evaluate(() => history.length), before + 1, '连续搜索只应创建一个历史节点');
    await page.goBack(); await parameter(page, 'view.q', null);
    await page.waitForFunction(() => document.querySelector('input[type="search"]').value === '');
    await page.goForward(); await parameter(page, 'view.q', 'package');
    await page.getByRole('button', { name: '文件排序', exact: true }).click();
    await page.getByRole('option', { name: '按名称', exact: true }).click();
    await parameter(page, 'view.sort', 'name');
    const list = page.locator('[data-aio-page-active="true"] [data-url-scroll="files-list"]');
    await list.evaluate(el => { el.scrollTop = 420; });
    await page.waitForFunction(() => new URL(location.href).searchParams.get('scroll')?.includes('files-list:0:420'));
    const shared = page.url();
    assert.deepEqual(new URL(shared).searchParams.getAll('external'), ['x', 'y']);
    const assertRestored = async target => {
      await assertFiles(target);
      await target.waitForFunction(() => document.querySelector('input[type="search"]').value === 'package');
      await target.waitForFunction(() => document.querySelector('[data-aio-page-active="true"] [data-url-scroll="files-list"]')?.scrollTop === 420);
      assert.equal(await target.getByRole('listbox', { name: '文件分类' }).getByRole('option', { name: /安装包/ }).getAttribute('aria-selected'), 'true');
      assert.equal(new URL(target.url()).searchParams.get('view.sort'), 'name');
    };
    await page.reload(); await assertRestored(page);
    const fresh = await contextFor(browser, mobile);
    const recipient = await fresh.newPage(); watch(recipient);
    await recipient.goto(shared); await assertRestored(recipient);
    await recipient.screenshot({ path: resolve(output, `${mobile ? 'mobile' : 'desktop'}-files.png`) });
    assert(await recipient.evaluate(() => document.documentElement.scrollWidth <= innerWidth), '页面横向溢出');
    await fresh.close();
    await page.getByRole('navigation', { name: '场景' }).getByRole('button', { name: '社区插件', exact: true }).click();
    await parameter(page, 'page', 'guest-1');
    await page.goBack(); await assertRestored(page);
    await menu(page, '文件列表', mobile);
    assert.equal(page.url(), shared, '重复选择不应改变 URL');
    await page.goto(`${base}/?page=missing-page`);
    await page.getByRole('alert').filter({ hasText: '页面不存在或没有访问权限' }).waitFor();
    await menu(page, '文件列表', mobile); await assertFiles(page);
    await page.goto(`${base}/?page=file-list&view.category=invalid&view.sort=invalid`);
    await assertFiles(page); await parameter(page, 'view.category', null); await parameter(page, 'view.sort', null);
    return { viewport: mobile ? 'mobile' : 'desktop', filters: true, searchHistory: true, sort: true, scroll: true, refresh: true, freshShare: true, backForward: true, invalidRecovery: true };
  } catch (error) { console.error('文件场景失败:', page.url(), errors); await page.screenshot({ path: resolve(output, 'failure-files.png'), timeout: 5000 }).catch(() => {}); throw error; }
  finally { await context.close(); }
}

async function guestScenario(browser, abi) {
  const context = await contextFor(browser, false);
  const page = await context.newPage(); watch(page);
  const frame = page.frameLocator('[data-aio-page-active="true"] iframe');
  try {
    await page.goto(`${base}/?page=guest-${abi}`);
    await frame.getByRole('button', { name: 'Filter open' }).click();
    await parameter(page, 'route', '#/items?status=open');
    await frame.getByRole('button', { name: 'Details' }).click();
    await parameter(page, 'route', '#/details?tab=history');
    await frame.locator('[data-url-scroll="items"]').evaluate(el => { el.scrollTop = 300; });
    await page.waitForFunction(() => new URL(location.href).searchParams.get('scroll')?.includes('guest-items:0:300'));
    const shared = page.url();
    assert(!/[?&](token|context|__aio_prepare)=/.test(shared), '链接泄露挂载信息');
    await page.evaluate(() => history.back()); await parameter(page, 'route', '#/items?status=open');
    await frame.locator('#route').filter({ hasText: '#/items?status=open' }).waitFor();
    await guestScroll(frame, 0);
    await page.evaluate(() => history.back()); await parameter(page, 'route', null);
    await page.evaluate(() => history.forward()); await parameter(page, 'route', '#/items?status=open');
    await page.evaluate(() => history.forward()); await parameter(page, 'route', '#/details?tab=history');
    await guestScroll(frame, 300);
    await page.reload();
    await frame.locator('#route').filter({ hasText: '#/details?tab=history' }).waitFor();
    await guestScroll(frame, 300);
    const fresh = await contextFor(browser, false);
    const recipient = await fresh.newPage(); watch(recipient);
    await recipient.goto(shared);
    const guest = recipient.frameLocator('[data-aio-page-active="true"] iframe');
    await guest.locator('#route').filter({ hasText: '#/details?tab=history' }).waitFor();
    await guestScroll(guest, 300);
    await fresh.close();
    return { abi, outerRoute: true, guestScroll: true, refresh: true, freshShare: true, backForward: true };
  } catch (error) { console.error('插件场景失败:', page.url(), errors, await page.evaluate(() => window.__navigationProbe)); await page.screenshot({ path: resolve(output, `failure-guest-${abi}.png`), timeout: 5000 }).catch(() => {}); throw error; }
  finally { await context.close(); }
}

(async () => {
  await mkdir(output, { recursive: true });
  const browser = await chromium.launch({ headless: true, args: ['--no-sandbox'] });
  try {
    const report = [];
    if (process.env.AIO_TEST_ONLY !== 'guest') report.push(await filesScenario(browser, false), await filesScenario(browser, true));
    report.push(await guestScenario(browser, 1), await guestScenario(browser, 2));
    assert.deepEqual(errors, []);
    await writeFile(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report, null, 2));
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
