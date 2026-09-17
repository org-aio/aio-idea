const assert = require('node:assert/strict');
const {randomUUID} = require('node:crypto');
const {mkdir, writeFile} = require('node:fs/promises');
const {resolve} = require('node:path');
const {contextFor, marketplace, launchBrowser, closeBrowser, viewportScreenshot} = require('./live-session.cjs');

const base = process.env.AIO_URL || 'https://aio.addzero.site';
const output = resolve('target/cli-removal-live');

(async () => {
  await mkdir(output, {recursive: true});
  const browser = await launchBrowser();
  const report = [];
  try {
    for (const mobile of [false, true]) {
      const context = await contextFor(browser, base, mobile);
      const access = await context.request.get(`${base}/api/runtime/tools/access`);
      assert(access.ok());
      assert.equal((await access.json()).data, true, '验收账号需要平台发布管理权限');
      const page = await context.newPage();
      const errors = [];
      let hostInstalls = 0;
      page.on('pageerror', error => errors.push(error.message));
      page.on('request', request => {
        if (request.method() === 'POST' && /\/api\/runtime\/plugins\/install$/.test(request.url())) hostInstalls++;
      });
      const catalog = await context.request.get(`${base}/api/runtime/marketplace`);
      assert(catalog.ok());
      const entries = (await catalog.json()).data;
      const existing = entries.find(entry => entry.cli?.id === 'codex-model-sync') || entries.find(entry => entry.cli);
      assert(existing);
      await page.goto(base, {waitUntil: 'domcontentloaded', timeout: 60000});
      await page.locator('.application-shell:visible').waitFor({timeout: 60000});
      await marketplace(page, mobile);
      await page.getByRole('textbox', {name: '搜索插件', exact: true}).fill(existing.title);
      await page.getByRole('treeitem').filter({hasText: existing.title}).first().click();
      await page.getByRole('button', {name: '从插件市场移除', exact: true}).click();
      await page.getByRole('dialog').getByRole('heading', {name: '从插件市场移除', exact: true}).waitFor();
      await viewportScreenshot(page, resolve(output, mobile ? 'mobile-confirm.png' : 'desktop-confirm.png'));
      await page.getByRole('button', {name: '取消', exact: true}).click();
      assert.equal((await context.request.get(`${base}/api/runtime/tools/${existing.cli.id}/${existing.cli.version}`)).status(), 200);
      assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      await viewportScreenshot(page, resolve(output, mobile ? 'mobile.png' : 'desktop.png'));

      // 只对本次创建的独立验收条目执行实际移除，已有市场条目仅打开确认框。
      const title = `CLI 移除验收 ${randomUUID()}`;
      const registration = await context.request.post(`${base}/api/runtime/tools/register`, {data: {
        command: `node -e "console.log('${title}')"`, platforms: ['macos'], uninstall: '', detect: '',
        metadata: {title, summary: '市场移除流程临时验收', git: ''},
      }});
      assert(registration.ok(), `登记验收条目失败：HTTP ${registration.status()}`);
      const fixture = (await registration.json()).data;
      try {
        await page.reload({waitUntil: 'domcontentloaded'});
        await page.locator('.application-shell:visible').waitFor({timeout: 60000});
        await marketplace(page, mobile);
        await page.getByRole('textbox', {name: '搜索插件', exact: true}).fill(title);
        await page.getByRole('treeitem').filter({hasText: title}).click();
        await page.getByRole('button', {name: '从插件市场移除', exact: true}).click();
        await page.getByRole('button', {name: '确认移除', exact: true}).click();
        await page.getByRole('dialog').waitFor({state: 'hidden'});
        const after = await context.request.get(`${base}/api/runtime/marketplace`);
        assert(after.ok());
        assert(!(await after.json()).data.some(entry => entry.cli?.id === fixture.id));
        assert.equal((await context.request.get(`${base}/api/runtime/tools/${fixture.id}/${fixture.version}`)).status(), 404);
        assert.equal(hostInstalls, 0);
        assert.deepEqual(errors, []);
        report.push({mobile, administratorAccess: true, existingEntryPreserved: true, fixtureRemoved: true, consoleErrors: errors});
      } finally {
        const cleanup = await context.request.delete(`${base}/api/runtime/tools/${fixture.id}`);
        assert(cleanup.ok());
      }
    }
    await writeFile(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report));
  } finally { await closeBrowser(browser); }
})().catch(error => { console.error(error.message); process.exitCode = 1; });
