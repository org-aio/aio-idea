const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const os = require('node:os');
const {spawn} = require('node:child_process');
const {contextFor, launchBrowser, closeBrowser} = require('./live-session.cjs');

const base = process.env.AIO_URL;
const cli = process.env.AIO_SPACE_TEST_CLI;
assert(base && cli && process.env.AIO_COOKIE_FILE);
const output = path.resolve('target/worker-device-identity');
async function until(read, accept, timeout = 60000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const value = await read();
    if (accept(value)) return value;
    await new Promise(resolve => setTimeout(resolve, 250));
  }
  throw new Error('Device acceptance timed out');
}
async function stop(worker) {
  if (worker.exitCode !== null || worker.signalCode !== null) return;
  const exited = new Promise(resolve => worker.once('exit', resolve));
  worker.kill('SIGTERM');
  await exited;
}
async function main() {
  await fs.mkdir(output, {recursive:true});
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'aio-device-identity-')));
  const browser = await launchBrowser();
  const context = await contextFor(browser, base, false);
  const page = await context.newPage();
  const workers = [];
  const devices = new Set();
  const errors = [];
  const report = {pairings:[], viewports:[]};
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => {if (message.type() === 'error') errors.push(message.text());});
  const list = async () => {
    const response = await context.request.get(base + '/api/runtime/workers');
    assert.equal(response.status(), 200);
    return (await response.json()).data;
  };
  async function pair(config) {
    let stderr = '';
    const worker = spawn(process.execPath, [cli, 'connect', '--server', base, '--root', root, '--no-browser', '--foreground'], {
      env:{...process.env, AIO_SPACE_CONFIG_DIR:config}, stdio:['ignore','ignore','pipe'],
    });
    workers.push(worker);
    worker.stderr.on('data', bytes => {stderr += bytes;});
    const link = await until(() => stderr.match(/https?:\/\/\S+worker_pair=[a-f0-9]+/)?.[0], Boolean);
    await page.goto(link);
    await page.getByRole('button', {name:'授权这台设备', exact:true}).click();
    const id = await until(async () => {
      try {return JSON.parse(await fs.readFile(path.join(config, 'worker.json'), 'utf8')).deviceId;}
      catch {return null;}
    }, Boolean);
    devices.add(id);
    await until(list, items => items.some(item => item.id === id && item.status === 'online'));
    report.pairings.push(id);
    return {id, worker};
  }
  try {
    await page.setViewportSize({width:1440, height:900});
    const first = await pair(path.join(root, 'primary'));
    await stop(first.worker);
    const second = await pair(path.join(root, 'primary'));
    assert.notEqual(second.id, first.id);
    const afterPair = await list();
    assert(!afterPair.some(item => item.id === first.id));
    assert(afterPair.some(item => item.id === second.id));
    const other = await pair(path.join(root, 'other'));
    const items = await list();
    assert.equal(items.find(item => item.id === second.id).label, items.find(item => item.id === other.id).label);
    const refreshed = page.waitForResponse(response => response.url() === base + '/api/runtime/workers' && response.request().method() === 'GET');
    await page.getByRole('button', {name:'刷新设备', exact:true}).click();
    assert.equal((await refreshed).status(), 200);
    for (const [name, viewport] of [['desktop',{width:1440,height:900}], ['mobile',{width:390,height:844}]]) {
      await page.setViewportSize(viewport);
      const dialog = page.getByRole('dialog').first();
      const bounds = await dialog.boundingBox();
      assert(bounds.x >= 0 && bounds.y >= 0 && bounds.x + bounds.width <= viewport.width + 1 && bounds.y + bounds.height <= viewport.height + 1);
      assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      await page.screenshot({path:path.join(output, name + '.png')});
      report.viewports.push({name, ...viewport, bounds});
    }
    await page.setViewportSize({width:1440,height:900});
    const row = page.locator('section').filter({hasText:second.id});
    await row.getByRole('button', {name:'删除设备', exact:true}).click();
    await page.getByRole('button', {name:'取消', exact:true}).click();
    assert((await list()).some(item => item.id === second.id));
    await row.getByRole('button', {name:'删除设备', exact:true}).click();
    await page.getByRole('button', {name:'确认删除', exact:true}).click();
    await until(list, items => !items.some(item => item.id === second.id));
    await row.waitFor({state:'detached'});
    report.deleted = second.id;
    report.errors = errors;
    assert.deepEqual(errors, []);
    await fs.writeFile(path.join(output, 'report.json'), JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report));
  } finally {
    await Promise.all(workers.map(stop));
    for (const id of devices) await context.request.delete(base + '/api/runtime/workers/' + id);
    await closeBrowser(browser);
    await fs.rm(root, {recursive:true, force:true});
  }
}
main().catch(error => {console.error(error.message);process.exitCode=1;});
