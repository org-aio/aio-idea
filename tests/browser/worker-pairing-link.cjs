const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const {contextFor, launchBrowser, closeBrowser} = require('./live-session.cjs');

const base = process.env.AIO_URL;
assert(base && process.env.AIO_COOKIE_FILE);
const output = path.resolve('target/worker-pairing-link');

async function main() {
  await fs.mkdir(output, {recursive:true});
  const browser = await launchBrowser();
  const context = await contextFor(browser, base, false);
  const page = await context.newPage();
  page.setDefaultTimeout(15000);
  const created = [];
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  async function createPairing() {
    const response = await context.request.post(base+'/api/runtime/workers/pairings', {
      data:{label:'配对链接回归 '+Date.now(),platform:'linux',capabilities:['space.scan']},
    });
    assert.equal(response.status(),200);
    const pairing = (await response.json()).data;
    created.push(pairing);
    return pairing;
  }
  const link = pairing => `${base}/?keep=pairing-test&worker_pair=${pairing.code}#pairing-test`;
  const authorize = () => page.getByRole('button',{name:'授权这台设备',exact:true});
  async function cleared() {
    await page.waitForURL(url=>!url.searchParams.has('worker_pair'));
    assert.equal(new URL(page.url()).searchParams.get('keep'),'pairing-test');
    assert.equal(new URL(page.url()).hash,'#pairing-test');
  }
  async function openDevices() {
    await page.locator('button[aria-label$="的账户菜单"]').click();
    await page.getByRole('menuitem',{name:'我的设备',exact:true}).click();
    await page.getByRole('heading',{name:'我的设备',exact:true}).waitFor();
  }
  try {
    await page.setViewportSize({width:1440,height:900});
    const pairing = await createPairing();
    await page.goto(link(pairing));
    await authorize().waitFor();
    await page.reload();
    await authorize().waitFor();
    assert.equal(new URL(page.url()).searchParams.get('worker_pair'),pairing.code);
    await page.evaluate(()=>history.replaceState({pairingRegression:true},'',location.href));
    await authorize().click();
    await cleared();
    assert.equal(await page.evaluate(()=>history.state.pairingRegression),true);
    await page.getByRole('status').filter({hasText:'设备已配对'}).waitFor();
    const poll = await context.request.post(base+'/api/runtime/workers/pairings/poll', {
      headers:{authorization:'Bearer '+pairing.token},
    });
    assert.equal((await poll.json()).data,'active');
    await page.getByRole('button',{name:'关闭',exact:true}).click();
    await openDevices();
    assert.equal(await authorize().count(),0);
    await page.reload();
    await page.locator('button[aria-label$="的账户菜单"]').waitFor();
    assert.equal(await page.getByRole('heading',{name:'我的设备',exact:true}).count(),0);

    // 已消费链接只提示一次，不能把已有设备说成失效。
    for (const viewport of [{width:1440,height:900},{width:390,height:844}]) {
      await page.setViewportSize(viewport);
      await page.goto(link(pairing));
      await page.getByRole('status').filter({hasText:'不影响已配对设备'}).waitFor();
      await cleared();
      assert.equal(await page.getByRole('alert').count(),0);
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
      const bounds = await page.getByRole('dialog').first().boundingBox();
      assert(bounds.x>=0 && bounds.y>=0 && bounds.x+bounds.width<=viewport.width+1 && bounds.y+bounds.height<=viewport.height+1);
      await page.screenshot({path:path.join(output,`${viewport.width}-expired-link.png`)});
    }

    await page.setViewportSize({width:1440,height:900});
    const pending = await createPairing();
    const endpoint = base+'/api/runtime/workers/pairings/'+pending.code;
    await page.route(endpoint,route=>route.fulfill({status:503,contentType:'application/json',body:'{"error":"temporary test outage"}'}));
    await page.goto(link(pending));
    await page.getByRole('alert').filter({hasText:'HTTP 503'}).waitFor();
    assert.equal(new URL(page.url()).searchParams.get('worker_pair'),pending.code);
    await page.unroute(endpoint);
    await page.reload();
    await authorize().waitFor();
    await page.getByRole('button',{name:'关闭',exact:true}).click();
    await cleared();

    // 确认页打开后被另一页面消费，提交也需结束旧配对流程。
    await page.goto(link(pending));
    await authorize().waitFor();
    assert.equal((await context.request.post(endpoint)).status(),200);
    await authorize().click();
    await page.getByRole('status').filter({hasText:'不影响已配对设备'}).waitFor();
    await cleared();
    assert.deepEqual(errors,[]);
    console.log(JSON.stringify({pendingReload:true,approvedRefresh:true,consumedLink:true,dismissedLink:true,transientFailurePreserved:true,concurrentApproval:true,viewports:[1440,390],pageErrors:errors}));
  } finally {
    for (const pairing of created) {
      await context.request.post(`${base}/api/runtime/workers/pairings/${pairing.code}`);
      await context.request.delete(`${base}/api/runtime/workers/${pairing.device_id}`);
    }
    await closeBrowser(browser);
  }
}
main().catch(error=>{console.error(error);process.exitCode=1;});
