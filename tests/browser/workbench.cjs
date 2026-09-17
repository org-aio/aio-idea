const assert = require('node:assert/strict');
const { createServer } = require('node:http');
const { readFile, mkdir, writeFile } = require('node:fs/promises');
const { resolve, extname } = require('node:path');
const { chromium } = require('playwright');

const root = resolve('target/dx/aio-idea/release/web/public');
const output = resolve('target/workbench-test');
let account = 'alice', loggedIn = true, mode = 'normal', marketCalls = 0;
const forbidden = [];
const revision = 'a'.repeat(64);
const plugin = (title, installed, number) => ({
  git: 'https://github.com/example/plugin-' + number + '.git',
  rev: revision, title, summary: '连接日常资料与工作流', license: 'MIT', tags: ['笔记'],
  installed, source_id: 'source-' + number, state: installed ? 'active' : null,
  active_revision: installed ? revision : null, runtime: 'process',
  capabilities: { network: [], filesystem: [], database: false },
});
let entries;
function reset() {
  account = 'alice'; loggedIn = true; mode = 'normal'; marketCalls = 0;
  entries = [plugin('记忆图谱', true, 1), plugin('数据大屏', false, 2)];
}
function session() {
  return { user_id: account, account, display_name: account === 'alice' ? '小林' : '另一账户',
    tenant_id: 'workspace', tenant_label: '我的工作区', permissions: ['plugin:manage'] };
}
function catalog() {
  return {session_context: account, context: 'workspace-' + account,
    tenant: {id: 'workspace', label: '我的工作区'},
    user: {label: session().display_name, handle: '@' + account, initials: 'ZJ'},
    plugins: [], account_items: [], page_versions: {},
    pages: [{id: 'dashboard', label: '数据总览', scene: {id: 'business', label: '我的业务'},
      menu_path: [{id: 'operations', label: '运营中心'}, {id: 'analysis', label: '数据分析'}],
      body: {kind: 'text', title: '数据总览', content: '欢迎使用工作台'}}],
  };
}
const server = createServer(async (req, res) => {
  const path = new URL(req.url, 'http://localhost').pathname;
  const send = (data, status = 200) => res.writeHead(status, {'content-type': 'application/json', 'cache-control': 'no-store'}).end(JSON.stringify({data}));
  try {
    if (path.includes('/registries')) { forbidden.push(path); return res.writeHead(404).end(); }
    if (path === '/api/auth/session') return send(loggedIn ? session() : null);
    if (path === '/api/runtime/bootstrap') return loggedIn ? send({catalog: catalog(), permissions: session().permissions}) : send(null);
    if (path === '/api/runtime/catalog') return send(catalog());
    if (path === '/api/auth/logout') { loggedIn = false; return send(null); }
    if (path === '/api/auth/login') { loggedIn = true; return send(session()); }
    if (path === '/api/runtime/marketplace') {
      marketCalls++;
      if (mode === 'denied') return send(null, 403);
      if (mode === 'failure') return res.writeHead(503, {'content-type': 'application/json'}).end(JSON.stringify({error: '市场暂时不可用'}));
      if (mode === 'empty') return send([]);
      if (mode === 'slow') return setTimeout(() => send(entries), 1200);
      return send(entries);
    }
    if (path.endsWith('/details')) return send({
      readme: '# 开始使用\n\n资料在当前工作区集中管理。\n\n' + '一段用于验证阅读位置的说明。\n\n'.repeat(45),
      version: '1.2.0', source_revision: 'b'.repeat(40),
      versions: [{revision, version: '1.2.0', source_revision: 'b'.repeat(40), created_at: '2026-09-14'}],
      builds: [{id: 1, state: 'failed', error: 'BUILD_LOG_DETAIL', source_revision: 'c'.repeat(40), updated_at: '2026-09-14'}],
    });
    if (path.startsWith('/api/')) return send([]);
    if (path === '/favicon.ico') return res.writeHead(204).end();
    const file = resolve(root, '.' + (path === '/' ? '/index.html' : path));
    assert(file.startsWith(root + '/'));
    const mime = {'.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.woff2': 'font/woff2'};
    res.writeHead(200, {'content-type': mime[extname(file)] || 'application/octet-stream'}).end(await readFile(file));
  } catch (error) { res.writeHead(500).end(error.message); }
});
async function openAccount(page, name, width) {
  if (width < 768) await page.getByRole('button', {name: '打开菜单', exact: true}).click();
  await page.locator('button[aria-label$="的账户菜单"]').last().click();
  await page.getByRole('menuitem', {name, exact: true}).click();
}
async function screenshot(page, name) {
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), '页面横向溢出');
  await page.screenshot({path: resolve(output, name + '.png'), fullPage: true, animations: 'disabled'});
}
async function run(browser, width, base) {
  reset();
  const context = await browser.newContext({viewport: {width, height: 960}, colorScheme: 'light'});
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.goto(base);
    await page.getByRole('button', {name: '我的业务', exact: true}).click();
    if (width < 768) await page.getByRole('button', {name: '打开菜单', exact: true}).click();
    await page.getByRole('button', {name: '数据总览', exact: true}).last().click();
    await page.getByRole('navigation', {name: '当前位置'}).getByText('数据总览').waitFor();
    assert.equal(await page.getByRole('dialog').count(), 0);
    await screenshot(page, width + '-navigation');
    await openAccount(page, '设置中心', width);
    await page.getByRole('heading', {name: '外观', exact: true}).waitFor();
    await page.getByRole('button', {name: '深色', exact: true}).click();
    await page.waitForFunction(() => document.documentElement.dataset.theme === 'dark');
    await page.getByRole('button', {name: '紧凑', exact: true}).click();
    await page.getByText('外观已保存到当前设备', {exact: true}).waitFor();
    await screenshot(page, width + '-settings-dark');
    assert.equal(await page.getByText('plugin:manage', {exact: true}).isVisible(), false);
    await page.getByText('技术详情', {exact: true}).click();
    await page.getByText('plugin:manage', {exact: true}).waitFor();
    await page.reload();
    await openAccount(page, '设置中心', width);
    await page.waitForFunction(() => document.documentElement.dataset.theme === 'dark' && document.documentElement.dataset.density === 'compact');
    await page.getByRole('button', {name: '跟随系统', exact: true}).click();
    await page.emulateMedia({colorScheme: 'light'});
    await page.waitForFunction(() => document.documentElement.dataset.theme === 'light');
    await page.emulateMedia({colorScheme: 'dark'});
    await page.waitForFunction(() => document.documentElement.dataset.theme === 'dark');
    await page.getByRole('button', {name: '浅色', exact: true}).click();
    await page.getByRole('button', {name: '舒适', exact: true}).click();
    await screenshot(page, width + '-settings-light');
    await page.getByRole('button', {name: '返回主后台', exact: true}).click();
    await page.getByRole('navigation', {name: '当前位置'}).getByText('数据总览').waitFor();
    await openAccount(page, '插件市场', width);
    await page.getByRole('treeitem').first().waitFor();
    await page.getByRole('button', {name: '已安装', exact: true}).click();
    assert.equal(await page.getByRole('treeitem').count(), 1);
    await page.getByRole('button', {name: '全部', exact: true}).click();
    await page.getByRole('textbox', {name: '搜索插件', exact: true}).fill('不存在');
    await page.getByText('没有找到匹配的插件', {exact: true}).waitFor();
    await screenshot(page, width + '-market-no-results');
    await page.getByRole('button', {name: '清空搜索', exact: true}).click();
    await page.getByRole('treeitem').first().click();
    await page.getByRole('tab', {name: '版本与发布', exact: true}).click();
    await page.getByText('构建失败', {exact: true}).waitFor();
    assert.equal(await page.getByText('BUILD_LOG_DETAIL', {exact: true}).isVisible(), false);
    await page.getByText('构建技术详情', {exact: true}).click();
    await page.getByText('BUILD_LOG_DETAIL', {exact: true}).waitFor();
    await screenshot(page, width + '-market-releases');
    await page.getByRole('tab', {name: '介绍', exact: true}).click();
    await page.getByRole('heading', {name: '开始使用', exact: true}).waitFor();
    await page.locator('.extension-browser__detail').evaluate(el => { el.scrollTop = 480; });
    const before = await page.locator('.extension-browser__detail').evaluate(el => el.scrollTop);
    const calls = marketCalls;
    await page.waitForFunction(() => true);
    await page.waitForTimeout(15500);
    assert(marketCalls > calls);
    assert.equal(await page.locator('.extension-browser__detail').evaluate(el => el.scrollTop), before);
    await page.getByRole('button', {name: '返回主后台', exact: true}).click();
    await openAccount(page, '设置中心', width);
    await page.getByRole('button', {name: '深色', exact: true}).click();
    await page.getByText('外观已保存到当前设备', {exact: true}).waitFor();
    account = 'bob';
    await page.emulateMedia({colorScheme: 'light'});
    await page.reload();
    await page.waitForFunction(() => document.documentElement.dataset.theme === 'light' && document.documentElement.dataset.density === 'comfortable');
    account = 'alice';
    await page.reload();
    await page.waitForFunction(() => document.documentElement.dataset.theme === 'dark');
    await openAccount(page, '插件市场', width);
    await page.getByRole('treeitem').first().waitFor();
    if (width < 768) await page.getByRole('treeitem').first().click();
    await screenshot(page, width + '-market-dark');
    assert.deepEqual(errors, []);
    return {width, navigation: true, preferences: true, userIsolation: true, systemTheme: true, filters: true, releases: true, refreshScroll: true};
  } catch (error) {
    await screenshot(page, width + '-failure');
    console.error((await page.locator('body').innerText()).slice(0, 3000));
    throw error;
  } finally { await context.close(); }
}
async function states(browser, base) {
  reset();
  const context = await browser.newContext({viewport: {width: 390, height: 844}, colorScheme: 'light'});
  const page = await context.newPage();
  loggedIn = false;
  await page.goto(base);
  await page.getByRole('heading', {name: '登录你的工作台'}).waitFor();
  await screenshot(page, 'mobile-login');
  loggedIn = true;
  for (const state of ['slow', 'empty', 'failure', 'denied']) {
    mode = state;
    await page.reload();
    await openAccount(page, '插件市场', 390);
    const expected = {slow: '正在加载', empty: '还没有发布的插件', failure: '市场暂时不可用', denied: '你没有执行此操作的权限，请联系工作区管理员'}[state];
    await page.getByText(expected, {exact: true}).first().waitFor();
    await screenshot(page, 'mobile-market-' + state);
    if (state === 'failure') {
      mode = 'normal';
      await page.getByRole('button', {name: '重试', exact: true}).first().click();
      await page.getByRole('treeitem').first().waitFor();
    }
  }
  await context.close();
  return {login: true, loading: true, empty: true, failureRetry: true, denied: true};
}
(async () => {
  await mkdir(output, {recursive: true});
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const base = 'http://127.0.0.1:' + server.address().port;
  const browser = await chromium.launch({channel: 'chrome', headless: true});
  try {
    const results = [];
    for (const width of process.env.AIO_UI_STATES_ONLY ? [] : [390, 768, 1440]) results.push(await run(browser, width, base));
    results.push(await states(browser, base));
    assert.deepEqual(forbidden, []);
    await writeFile(resolve(output, 'report.json'), JSON.stringify({results, registryRequests: forbidden}, null, 2));
    console.log(JSON.stringify(results));
  } finally { await browser.close(); server.closeAllConnections(); server.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
