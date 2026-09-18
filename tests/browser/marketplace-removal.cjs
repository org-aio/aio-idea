const assert = require('node:assert/strict');
const { createServer } = require('node:http');
const { readFile, mkdir } = require('node:fs/promises');
const { resolve, extname } = require('node:path');
const { chromium } = require('playwright');

const root = resolve(process.env.AIO_WEB_ROOT || 'target/dx/aio-idea/debug/web/public');
const output = resolve('target/marketplace-removal-test');
const plugin = { git: 'https://example.com/normal.git', rev: 'a'.repeat(64), title: '普通插件', summary: '宿主插件', license: 'MIT', tags: [], installed: false };
const session = { user_id: 'test', account: 'test', display_name: 'Test', tenant_id: 'test', tenant_label: 'Test', permissions: ['plugin:manage'] };
const catalog = { session_context: 'test', context: 'test', tenant: { id: 'test', label: 'Test' }, user: { label: 'Test', handle: 'test', initials: 'T' }, plugins: [], pages: [], page_versions: {}, account_items: [] };
let canManage = true;
let removed = false;
let fail = false;
let requests = [];
const server = createServer(async (req, res) => {
  const send = data => res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ data }));
  const path = new URL(req.url, 'http://localhost').pathname;
  try {
    if (path === '/api/auth/session') return send(session);
    if (path === '/api/runtime/bootstrap') return send({ catalog, permissions: session.permissions });
    if (path === '/api/runtime/catalog') return send(catalog);
    if (path === '/api/runtime/tools/access') return send(canManage);
    if (path === '/api/runtime/marketplace') {
      if (req.method === 'DELETE') {
        let body = '';
        for await (const chunk of req) body += chunk;
        requests.push(JSON.parse(body));
        if (fail) return res.writeHead(503, { 'content-type': 'application/json' }).end(JSON.stringify({ error: '测试删除失败' }));
        removed = true;
        return send(null);
      }
      return send(removed ? [] : [plugin]);
    }
    if (path.endsWith('/details')) return send({ readme: '# 普通插件', version: '1.0.0', versions: [], builds: [] });
    if (path.startsWith('/api/')) return send([]);
    const file = resolve(root, '.' + (path === '/' ? '/index.html' : path));
    assert(file.startsWith(root + '/'));
    const mime = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm' };
    const content = await readFile(file);
    res.writeHead(200, { 'content-type': mime[extname(file)] || 'application/octet-stream' }).end(content);
  } catch (error) {
    res.writeHead(500).end(error.message);
  }
});

(async () => {
  await mkdir(output, { recursive: true });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  try {
    for (const mobile of [false, true]) {
      for (const manager of [false, true]) {
        canManage = manager; removed = false; fail = false; requests = [];
        const context = await browser.newContext({ viewport: mobile ? { width: 390, height: 844 } : { width: 1440, height: 900 } });
        const page = await context.newPage();
        const errors = [];
        page.on('pageerror', error => errors.push(error.message));
        await page.goto(`http://127.0.0.1:${server.address().port}`);
        if (mobile) await page.getByRole('button', { name: '打开菜单', exact: true }).click();
        await page.locator('button[aria-label$="的账户菜单"]').last().click();
        await page.getByRole('menuitem', { name: '插件市场', exact: true }).click();
        await page.getByRole('tree', { name: '插件', exact: true }).getByRole('treeitem').click();
        await page.getByRole('heading', { name: '普通插件', exact: true }).first().waitFor();
        const remove = page.getByRole('button', { name: '从市场删除', exact: true });
        if (!manager) {
          assert.equal(await remove.count(), 0);
        } else {
          await remove.click();
          await page.getByRole('dialog').waitFor();
          assert.equal(requests.length, 0);
          await page.screenshot({ path: resolve(output, mobile ? 'mobile-confirm.png' : 'desktop-confirm.png'), animations: 'disabled' });
          assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
          await page.getByRole('button', { name: '取消', exact: true }).click();
          assert.equal(requests.length, 0);
          await remove.click();
          fail = true;
          await page.getByRole('button', { name: '确认删除', exact: true }).click();
          await page.getByText('测试删除失败', { exact: false }).waitFor();
          assert.equal(removed, false);
          assert.deepEqual(requests, [{ git: plugin.git }]);
          fail = false;
          await page.getByRole('button', { name: '确认删除', exact: true }).click();
          await page.getByRole('dialog').waitFor({ state: 'hidden' });
          await page.getByRole('heading', { name: '普通插件', exact: true }).first().waitFor({ state: 'hidden' });
          assert.equal(removed, true);
          assert.deepEqual(requests, [{ git: plugin.git }, { git: plugin.git }]);
        }
        assert.deepEqual(errors, []);
        await context.close();
      }
    }
    console.log('Marketplace removal: desktop/mobile, permission visibility, cancel, failure/retry and list refresh passed');
  } finally {
    await browser.close();
    server.close();
  }
})().catch(error => { console.error(error); server.close(); process.exitCode = 1; });
