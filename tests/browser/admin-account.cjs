const assert = require('node:assert/strict');
const { mkdir, writeFile } = require('node:fs/promises');
const { resolve } = require('node:path');
const { randomUUID } = require('node:crypto');
const { chromium } = require('playwright');

const base = process.env.AIO_URL;
assert(new URL(base).hostname === '127.0.0.1', 'Only the isolated local test host is allowed');
const output = resolve('target/admin-ui-test');

async function login(page, account, password) {
  await page.goto(base);
  await page.getByLabel('账号', { exact: true }).fill(account);
  await page.getByLabel('密码', { exact: true }).fill(password);
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await page.locator('.application-shell:visible').waitFor();
}
async function open(page, name, mobile) {
  if (mobile) await page.getByRole('button', { name: '打开菜单', exact: true }).click();
  const nav = mobile ? page.getByRole('dialog') : page.locator('.application-shell__sidebar');
  await nav.locator('button[aria-label$="的账户菜单"]').click();
  await page.getByRole('menuitem', { name, exact: true }).click();
  await page.locator('.application-fullscreen:visible').waitFor();
  assert.equal(await page.locator('.application-shell:visible').count(), 0);
}
async function back(page) {
  await page.getByRole('button', { name: '返回主后台', exact: true }).click();
  await page.locator('.application-shell:visible').waitFor();
}
async function saved(dialog) {
  await dialog.getByRole('button', { name: '保存', exact: true }).click();
  await dialog.waitFor({ state: 'detached' });
}

async function scenario(browser, mobile) {
  const context = await browser.newContext({ viewport: mobile ? { width: 390, height: 844 } : { width: 1440, height: 1000 }, isMobile: mobile, hasTouch: mobile });
  const page = await context.newPage();
  const errors = [];
  let documents = 0;
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => { if (request.isNavigationRequest() && request.frame() === page.mainFrame()) documents++; });
  const request = context.request;
  try {
    await login(page, process.env.AIO_BOOTSTRAP_ACCOUNT, process.env.AIO_BOOTSTRAP_PASSWORD);
    const original = (await (await request.get(`${base}/api/auth/session`)).json()).data;
    await open(page, '个人资料', mobile);
    await page.getByRole('heading', { name: '个人资料', exact: true }).waitFor();
    await page.getByRole('button', { name: '修改密码', exact: true }).click();
    const password = page.getByRole('dialog', { name: '修改密码', exact: true });
    await password.waitFor();
    await page.keyboard.press('Escape');
    await password.waitFor({ state: 'detached' });
    await page.screenshot({ path: resolve(output, `${mobile ? 'mobile' : 'desktop'}-profile.png`) });
    await back(page);
    await open(page, '设置中心', mobile);
    await page.getByRole('heading', { name: '设置', exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: '添加市场源', exact: true }).count(), 0);
    assert.equal((await request.get(base + '/api/runtime/registries')).status(), 404);
    await page.getByRole('button', { name: '深色', exact: true }).click();
    await page.waitForFunction(() => document.documentElement.dataset.theme === 'dark');
    await page.getByRole('button', { name: '紧凑', exact: true }).click();
    await page.getByText('外观已保存到当前设备', { exact: true }).waitFor();
    await page.screenshot({ path: resolve(output, (mobile ? 'mobile' : 'desktop') + '-settings.png') });
    await page.getByRole('button', { name: '浅色', exact: true }).click();
    await back(page);
    await open(page, '插件市场', mobile);
    await page.getByRole('heading', { name: '插件市场', exact: true }).waitFor();
    await page.getByRole('textbox', { name: '搜索插件', exact: true }).fill('no-such-package-test');
    await page.getByText(/没有找到匹配的插件|还没有发布的插件/).first().waitFor();
    await page.getByRole('textbox', { name: '搜索插件', exact: true }).fill('');
    await page.screenshot({ path: resolve(output, (mobile ? 'mobile' : 'desktop') + '-marketplace.png') });
    await back(page);
    await open(page, '切换租户', mobile);
    await page.getByRole('button', { name: '新建租户', exact: true }).click();
    const tenant = page.getByRole('dialog', { name: '新建租户', exact: true });
    const name = `${process.env.AIO_ADMIN_TEST_RUN}-${mobile ? 'mobile' : 'desktop'}`;
    await tenant.getByLabel('租户名称', { exact: true }).fill(name);
    await saved(tenant);
    await page.getByRole('searchbox', { name: '搜索租户', exact: true }).fill(name);
    await page.getByRole('button', { name: `切换到 ${name}`, exact: true }).click();
    await page.locator('.application-shell:visible').waitFor();
    const current = (await (await request.get(`${base}/api/auth/session`)).json()).data;
    assert.notEqual(current.tenant_id, original.tenant_id);
    await open(page, '切换租户', mobile);
    await page.getByRole('button', { name: '重命名租户', exact: true }).click();
    const rename = page.getByRole('dialog', { name: '重命名租户', exact: true });
    await rename.getByLabel('租户名称', { exact: true }).fill(`${name}-已修改`);
    await saved(rename);
    assert.equal((await (await request.get(`${base}/api/auth/session`)).json()).data.tenant_label, `${name}-已修改`);
    assert.equal((await request.put(`${base}/api/tenants/${original.tenant_id}`, { data: { label: '越权重命名' } })).status(), 403);
    await page.getByRole('searchbox', { name: '搜索租户', exact: true }).fill(original.tenant_label);
    await page.getByRole('button', { name: `切换到 ${original.tenant_label}`, exact: true }).click();
    await page.locator('.application-shell:visible').waitFor();
    assert.equal((await (await request.get(`${base}/api/auth/session`)).json()).data.tenant_id, original.tenant_id);
    assert.equal(documents, 1, 'Login, fullscreen navigation and tenant switching must not reload the document');
    assert.deepEqual(errors, []);
    return { viewport: mobile ? 'mobile' : 'desktop', login: true, fullscreenReturn: true, appearanceSaved: true, registryEndpointRemoved: true, tenantCreateRenameSwitch: true, noDocumentReload: true };
  } catch (error) {
    await page.screenshot({ path: resolve(output, 'account-failure.png') });
    throw error;
  } finally {
    await context.close();
  }
}

async function passwordScenario(browser) {
  const admin = await browser.newContext();
  const current = await browser.newContext();
  const other = await browser.newContext();
  const account = `password-test-${randomUUID().slice(0, 8)}`;
  const oldPassword = 'old-local-password-123';
  const newPassword = 'new-local-password-123';
  let id;
  try {
    assert((await admin.request.post(`${base}/api/auth/login`, { data: { account: process.env.AIO_BOOTSTRAP_ACCOUNT, password: process.env.AIO_BOOTSTRAP_PASSWORD } })).ok());
    assert((await admin.request.post(`${base}/api/rbac/users`, { data: { account, display_name: '密码验收', password: oldPassword } })).ok());
    id = (await (await admin.request.get(`${base}/api/rbac`)).json()).data.users.find(u => u.account === account).id;
    const page = await current.newPage();
    await login(page, account, oldPassword);
    assert((await other.request.post(`${base}/api/auth/login`, { data: { account, password: oldPassword } })).ok());
    await open(page, '个人资料', false);
    await page.getByRole('button', { name: '修改密码', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '修改密码', exact: true });
    await dialog.getByLabel('当前密码', { exact: true }).fill(oldPassword);
    await dialog.getByLabel('新密码', { exact: true }).fill(newPassword);
    await dialog.getByLabel('确认新密码', { exact: true }).fill(`${newPassword}-different`);
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('alert').filter({ hasText: '两次输入的新密码不一致' }).waitFor();
    await dialog.getByLabel('确认新密码', { exact: true }).fill(newPassword);
    await saved(dialog);
    assert.equal((await (await other.request.get(`${base}/api/auth/session`)).json()).data, null);
    assert(!(await other.request.post(`${base}/api/auth/login`, { data: { account, password: oldPassword } })).ok());
    assert((await other.request.post(`${base}/api/auth/login`, { data: { account, password: newPassword } })).ok());
    assert((await (await current.request.get(`${base}/api/auth/session`)).json()).data);
    return { passwordConfirmation: true, passwordChange: true, otherSessionsRevoked: true, currentSessionPreserved: true };
  } finally {
    if (id) await admin.request.delete(`${base}/api/rbac/users/${id}`);
    await admin.close(); await current.close(); await other.close();
  }
}

(async () => {
  await mkdir(output, { recursive: true });
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  try {
    const report = [await scenario(browser, false), await scenario(browser, true), await passwordScenario(browser)];
    await writeFile(resolve(output, 'account-report.json'), JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report, null, 2));
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
