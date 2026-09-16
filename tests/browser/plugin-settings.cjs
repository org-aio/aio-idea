const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require('playwright');

// 只在两个验收插件都未安装的专用租户执行，结束后恢复安装状态。
async function main() {
  const base = process.env.AIO_URL;
  const output = path.resolve('target/plugin-settings-test');
  fs.mkdirSync(output, { recursive: true });
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const context = await browser.newContext({ viewport: { width: 1440, height: 960 } });
  const cookie = fs.readFileSync(process.env.AIO_COOKIE_FILE, 'utf8').split('\n').find(line => line.includes('\taio_session\t')).trim().split('\t').at(-1);
  await context.addCookies([{ name: 'aio_session', value: cookie, url: base, httpOnly: true, secure: new URL(base).protocol === 'https:' }]);
  const page = await context.newPage();
  page.setDefaultTimeout(60000);
  const errors = [], created = [], reports = [], tickets = new Set();
  const redact = text => text.replace(/\/(components|frontend)\/(assets\/)?[^/]+\/(request|renew|index.html|settings.html)/g, '/$1/[token]/$3');
  page.on('pageerror', error => errors.push(redact(error.message)));
  page.on('console', message => { if (message.type() === 'error') errors.push(redact(message.text())); });
  page.on('response', response => {
    if (response.url().endsWith('/frontend/mount') && response.ok()) void response.json().then(value => tickets.add(value.data.token)).catch(() => {});
  });
  async function api(method, endpoint, data) {
    const response = await context.request.fetch(base + endpoint, { method, data, timeout: 180000 });
    const body = await response.json();
    assert(response.ok(), `${endpoint}: ${response.status()} ${body.error || ''}`);
    return body.data;
  }
  async function plugin(frame, method, endpoint, body) {
    return frame.locator('body').evaluate(async (_, request) => {
      const result = await window.aioPlugin.request({ ...request, body: request.body === undefined ? undefined : new TextEncoder().encode(JSON.stringify(request.body)) });
      if (result.status >= 400) throw Error('Plugin HTTP ' + result.status);
      return result.body.length ? JSON.parse(new TextDecoder().decode(result.body)) : null;
    }, { method, path: endpoint, body });
  }
  async function openSettings(mobile) {
    await page.goto(base);
    await page.locator('.application-shell:visible').waitFor();
    if (mobile) await page.getByRole('button', { name: '打开菜单', exact: true }).click();
    const nav = mobile ? page.getByRole('dialog').last() : page.locator('.application-shell__sidebar');
    await nav.locator('button[aria-label$="的账户菜单"]').click();
    await page.getByRole('menuitem', { name: '设置中心', exact: true }).click();
    await page.getByRole('navigation', { name: '设置分组' }).waitFor();
  }
  const navigation = () => page.getByRole('navigation', { name: '设置分组' });
  const frame = () => page.frameLocator('iframe[title="智能体设置"]');
  const waitPlugin = async () => frame().getByRole('heading', { name: '模型服务', exact: true }).waitFor();
  try {
    const market = await api('GET', '/api/runtime/marketplace');
    const entries = ['aio-plugin-agent', 'aio-plugin-agent-memory'].map(name => market.find(entry => entry.git === `https://github.com/zjarlin/${name}.git`));
    assert(entries.every(entry => entry && !entry.installed), '请使用两个验收插件均未安装的租户');
    await openSettings(false);
    assert.equal(await navigation().getByRole('button', { name: '智能体', exact: true }).count(), 0);
    for (const entry of entries) {
      await api('POST', '/api/runtime/plugins/install', { git: entry.git });
      created.push(entry);
    }
    await navigation().getByRole('button', { name: '智能体', exact: true }).waitFor();
    for (const [name, viewport, mobile] of [['desktop', { width: 1440, height: 960 }, false], ['mobile', { width: 390, height: 844 }, true]]) {
      await page.setViewportSize(viewport);
      if (mobile) await openSettings(true);
      await navigation().getByRole('button', { name: '智能体', exact: true }).click();
      await waitPlugin();
      assert.equal(await page.getByRole('dialog').count(), 0, '插件设置直接显示在设置中心');
      const region = page.getByRole('region', { name: '智能体设置', exact: true });
      const caption = region.getByText('来自插件（智能体）', { exact: true });
      await caption.waitFor();
      assert(await caption.evaluate(element => parseFloat(getComputedStyle(element).fontSize) <= 13));
      await frame().getByRole('button', { name: '添加服务', exact: true }).click();
      assert.equal(await frame().getByLabel('名称', { exact: true }).count(), 0);
      await frame().getByRole('textbox', { name: '服务地址', exact: true }).fill('https://example.test/v1');
      await frame().getByLabel('API Key', { exact: true }).fill('synthetic-model-key');
      await page.screenshot({ path: path.join(output, name + '-provider.png') });
      await frame().getByRole('button', { name: '取消', exact: true }).click();
      if (!mobile) {
        assert.equal((await plugin(frame(), 'GET', '/settings')).webSearch.hasSecret, false);
        await frame().getByRole('button', { name: '配置网页搜索', exact: true }).click();
        await frame().getByLabel('Tavily API Key', { exact: true }).fill('synthetic-settings-acceptance-key');
        await frame().getByRole('button', { name: '保存', exact: true }).click();
        await frame().getByRole('dialog').waitFor({ state: 'hidden' });
        await navigation().getByRole('button', { name: '外观', exact: true }).click();
        assert.equal(await page.locator('iframe[title="智能体设置"]').count(), 0);
        await page.getByRole('button', { name: '深色', exact: true }).waitFor();
        await navigation().getByRole('button', { name: '智能体', exact: true }).click();
        await waitPlugin();
        const settings = await plugin(frame(), 'GET', '/settings');
        assert(settings.webSearch.hasSecret && !settings.webSearch.enabled);
        assert(!JSON.stringify(settings).includes('synthetic-settings-acceptance-key'));
        await frame().getByRole('button', { name: '配置网页搜索', exact: true }).click();
        assert.equal(await frame().getByLabel('Tavily API Key', { exact: true }).inputValue(), '');
        await frame().getByText('清除已保存的密钥', { exact: true }).click();
        await frame().getByRole('button', { name: '保存', exact: true }).click();
        await frame().getByRole('dialog').waitFor({ state: 'hidden' });
        assert.equal((await plugin(frame(), 'GET', '/settings')).webSearch.hasSecret, false);
      }
      await page.waitForTimeout(250);
      await page.screenshot({ path: path.join(output, name + '-settings.png') });
      assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1));
      assert(await frame().locator('body').evaluate(element => element.scrollWidth <= innerWidth + 1));
      reports.push({ viewport: name, inlineSettings: true, pluginSource: true, noName: true, manualUrl: true, overflow: false });
      console.log(name + ': inline settings and plugin source passed');
    }
    while (created.length) {
      const entry = created.at(-1);
      await api('POST', `/api/runtime/plugins/${entry.source_id}/uninstall`);
      created.pop();
    }
    await navigation().getByRole('button', { name: '智能体', exact: true }).waitFor({ state: 'detached' });
    assert.equal(await page.locator('iframe[title="智能体设置"]').count(), 0);
    await page.getByRole('heading', { name: '账户与工作区', exact: true }).waitFor();
    assert.deepEqual(errors, []);
    fs.writeFileSync(path.join(output, 'report.json'), JSON.stringify({ base, reports, dynamicInstallUninstall: true, keyPersistenceNoEchoClear: true, errors }, null, 2));
    console.log('Settings groups, dynamic installation, key persistence and cleanup passed');
  } catch (error) {
    await page.screenshot({ path: path.join(output, 'failure.png') }).catch(() => {});
    console.log('Browser errors: ' + JSON.stringify(errors));
    throw error;
  } finally {
    for (const ticket of tickets) await context.request.delete(base + '/api/runtime/frontend/' + ticket).catch(() => {});
    for (const entry of created.reverse()) await api('POST', `/api/runtime/plugins/${entry.source_id}/uninstall`);
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
