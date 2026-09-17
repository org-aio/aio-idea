const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const os = require('node:os');
const {spawn, execFileSync} = require('node:child_process');
const {randomUUID} = require('node:crypto');
const {contextFor, launchBrowser, closeBrowser} = require('./live-session.cjs');

const base = process.env.AIO_URL;
assert(base && process.env.AIO_COOKIE_FILE && process.env.AIO_SPACE_TEST_CLI);
const output = path.resolve('target/worker-acceptance');
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(read, accept, timeout = 60000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const value = await read();
    if (accept(value)) return value;
    await pause(500);
  }
  throw new Error('Worker acceptance timed out');
}
async function main() {
  await fs.mkdir(output, {recursive:true});
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'aio-worker-browser-')));
  const config = path.join(root, 'config');
  const source = path.join(root, 'fixture');
  await fs.mkdir(source);
  await fs.writeFile(path.join(source, 'acceptance.txt'), 'AIO worker browser acceptance\n');
  let stderr = '';
  const background = process.env.AIO_WORKER_BACKGROUND_TEST==='1';
  const service = `gui/${process.getuid()}/site.addzero.aio-space.worker`;
  const plist = path.join(os.homedir(),'Library/LaunchAgents/site.addzero.aio-space.worker.plist');
  if(background){
    assert.equal(process.platform,'darwin');
    await assert.rejects(fs.access(plist));
  }
  const worker = spawn(process.execPath, [process.env.AIO_SPACE_TEST_CLI, 'connect', '--server', base, '--root', root, '--no-browser', ...(background?[]:['--foreground'])], {env:{...process.env, AIO_SPACE_CONFIG_DIR:config}, stdio:['ignore','ignore','pipe']});
  worker.stderr.on('data', bytes => {stderr += bytes;});
  const browser = await launchBrowser();
  let context;
  let device;
  const report = {background,viewports:[], tasks:[]};
  try {
    const link = await until(async () => stderr.match(/https?:\/\/\S+worker_pair=[a-f0-9]+/)?.[0], Boolean);
    context = await contextFor(browser, base, false);
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => {if(message.type()==='error')errors.push(message.text());});
    await page.setViewportSize({width:1440,height:900});
    await page.goto(link);
    await page.getByRole('button', {name:'授权这台设备',exact:true}).waitFor();
    await page.screenshot({path:path.join(output,'desktop-pairing.png')});
    await page.getByRole('button', {name:'授权这台设备',exact:true}).click();
    device = await until(async () => {try{return JSON.parse(await fs.readFile(path.join(config,'worker.json'),'utf8')).deviceId;}catch{return null;}},Boolean);
    const readTasks = async () => (await (await context.request.get(base+'/api/runtime/workers/tasks')).json()).data.filter(task => task.worker_id===device);
    async function task(capability, input) {
      const previous = new Set((await readTasks()).map(task=>task.id));
      const response = await context.request.post(base+'/api/runtime/workers/tasks', {
        data:{id:randomUUID(),worker_id:device,capability,input},
      });
      assert.equal(response.status(),200);
      const finished = await until(readTasks, tasks => tasks.some(task=>!previous.has(task.id) && ['complete','failed','interrupted'].includes(task.state)),120000);
      const result = finished.find(task=>!previous.has(task.id));
      assert.equal(result.state,'complete',JSON.stringify(result));
      report.tasks.push({capability:result.capability,state:result.state});
      return result.result;
    }
    await task('space.scan', {path:source});
    const archive = await task('space.archive', {path:source});
    const restored = path.join(root,'restored');
    await task('space.archive-restore', {path:restored,snapshot:archive.snapshotId});
    assert.equal(await fs.readFile(path.join(restored,source.slice(1),'acceptance.txt'),'utf8'),'AIO worker browser acceptance\n');
    if(background){
      assert.equal(worker.exitCode,0,stderr);
      assert.match(execFileSync('launchctl',['print',service],{encoding:'utf8'}),/state = running/);
    }
    for (const [label,viewport] of [['desktop',{width:1440,height:900}],['mobile',{width:390,height:844}]]) {
      await page.setViewportSize(viewport);
      const dialog = page.getByRole('dialog').first();
      const bounds = await dialog.boundingBox();
      assert(bounds.x>=0 && bounds.y>=0 && bounds.x+bounds.width<=viewport.width+1 && bounds.y+bounds.height<=viewport.height+1);
      assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
      await page.screenshot({path:path.join(output,`${label}-devices.png`)});
      report.viewports.push({...viewport,bounds});
    }
    assert.deepEqual(errors,[]);
    await fs.writeFile(path.join(output,'report.json'),JSON.stringify(report,null,2));
    console.log(JSON.stringify(report));
  } finally {
    if (device && context) await context.request.delete(`${base}/api/runtime/workers/${device}`);
    worker.kill('SIGTERM');
    if(background && (await fs.readFile(plist,'utf8').catch(()=>'' )).includes(config)){
      execFileSync('launchctl',['bootout',service]);
      await fs.rm(plist);
    }
    await closeBrowser(browser);
    await fs.rm(config,{recursive:true,force:true});
  }
}
main().catch(error=>{console.error(error);process.exitCode=1;});
