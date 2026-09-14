const assert = require('node:assert/strict');
const path = require('node:path');
const {mkdirSync, writeFileSync} = require('node:fs');
const {PNG} = require('pngjs');
const {launchBrowser, contextFor, closeBrowser, viewportScreenshot} = require('./live-session.cjs');
const {geometry, viewports} = require('./frontend-layout.cjs');

const base = process.env.AIO_URL || 'https://aio.addzero.site';
const directory = path.resolve('target/component-delivery/memory-workbench');
mkdirSync(directory, {recursive:true});
async function run() {
  const browser = await launchBrowser();
  try {
    const context = await contextFor(browser,base,false);
    const page = await context.newPage();
    page.setDefaultTimeout(120000);
    const errors = [];
    let blockedFallbackFonts = 0;
    const responses = [];
    const redact = value=>value.replace(/\/(components|frontend)\/assets\/[^/]+/g,'/$1/assets/[token]').replace(/\/(components|frontend)\/[^/]+\/(request|renew)/g,'/$1/[token]/$2');
    page.on('pageerror',error=>errors.push(redact(error.message)));
    page.on('console',message=>{
      if(message.type()!=='error')return;
      const text=message.text();
      // 沙箱继续禁止 Compose 的外部回退字体；单独记录已有警告，不掩盖其他错误。
      if(text.includes('https://fonts.gstatic.com/s/notosanssc/')&&text.includes('Content Security Policy'))blockedFallbackFonts++;
      else errors.push(redact(text));
    });
    page.on('response',response=>{
      if(response.status()>=400) errors.push(`${redact(new URL(response.url()).pathname)}: HTTP ${response.status()}`);
      if(response.url().endsWith('/request')&&response.ok()) responses.push(response.json().then(value=>{
        if(value.data.status>=400) errors.push(`Memory service: HTTP ${value.data.status}`);
      }));
    });
    await page.goto(base);
    await page.getByRole('navigation',{name:'场景'}).getByRole('button',{name:'社区插件',exact:true}).click();
    const navigation = page.locator('.application-shell__sidebar');
    const memory = navigation.getByRole('button',{name:'记忆图谱',exact:true});
    if(!await memory.isVisible()) await navigation.getByRole('button',{name:'智能体',exact:true}).click();
    const graphReady = page.waitForResponse(response=>response.url().endsWith('/request')&&response.request().postDataJSON()?.path==='/graph');
    await memory.click();
    const frame = page.frameLocator('iframe[title="记忆图谱"]');
    const iframe = page.locator('iframe[title="记忆图谱"]');
    await frame.locator('canvas').first().waitFor();
    await frame.getByRole('button',{name:'新建记忆',exact:true}).first().waitFor();
    const graph = await graphReady;
    assert(graph.request().postDataJSON().query.startsWith('spaceId='));
    assert.equal((await graph.json()).data.status,200);
    const layouts = [];
    const marker = await frame.locator('body').evaluate(()=>window.__memoryLayoutMarker=Math.random());
    for(const [name,viewport] of viewports) {
      await page.setViewportSize(viewport);
      await page.waitForTimeout(1500);
      const bounds = await geometry(iframe);
      assert(Math.abs(bounds.height-bounds.expectedHeight)<=1,JSON.stringify(bounds));
      assert(Math.abs(bounds.width-bounds.expectedWidth)<=1,JSON.stringify(bounds));
      assert(Math.abs(bounds.bottomGap)<=1,JSON.stringify(bounds));
      assert.equal(bounds.overflow,false);
      const canvas = frame.locator('canvas').first();
      assert(Math.abs((await canvas.boundingBox()).height-bounds.height)<=1);
      const png = PNG.sync.read(await canvas.screenshot());
      let colored = 0;
      for(let i=0;i<png.data.length;i+=4) if(png.data[i+1]>png.data[i]+15&&png.data[i+1]>png.data[i+2]) colored++;
      assert(colored>100,'Memory workspace was not painted');
      assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
      assert.equal(await frame.locator('body').evaluate(()=>window.__memoryLayoutMarker),marker);
      await viewportScreenshot(page,path.join(directory,`${name}.png`));
      layouts.push({name,viewport,...bounds,coloredCanvasPixels:colored});
    }
    await page.setViewportSize(viewports[0][1]);
    await page.waitForTimeout(600);
    await frame.getByRole('button',{name:'新建记忆',exact:true}).first().click({force:true});
    const cancel = frame.getByRole('button',{name:'取消',exact:true});
    await cancel.waitFor({timeout:10000});
    const dialogs = [];
    for(const [name,viewport] of viewports) {
      await page.setViewportSize(viewport);
      await page.waitForTimeout(600);
      const cancelBounds = await cancel.boundingBox();
      const frameBounds = await iframe.boundingBox();
      assert(cancelBounds.y>=frameBounds.y&&cancelBounds.y+cancelBounds.height<=frameBounds.y+frameBounds.height,'Dialog actions must remain inside the plugin');
      await viewportScreenshot(page,path.join(directory,`${name}-dialog.png`));
      dialogs.push({name,cancelBounds,frameBounds});
    }
    const canvas = frame.locator('canvas').first();
    const beforeScroll = PNG.sync.read(await canvas.screenshot());
    const area = await iframe.boundingBox();
    await page.mouse.move(area.x+area.width/2,area.y+area.height/2);
    await page.mouse.wheel(0,1000);
    await page.waitForTimeout(600);
    const afterScroll = PNG.sync.read(await canvas.screenshot());
    let changedPixels = 0;
    for(let i=0;i<beforeScroll.data.length;i+=4)if(beforeScroll.data.readUInt32BE(i)!==afterScroll.data.readUInt32BE(i))changedPixels++;
    assert(changedPixels>100,'Short dialog must scroll its form content');
    await viewportScreenshot(page,path.join(directory,'mobile-landscape-dialog-scrolled.png'));
    await cancel.click({force:true});
    await cancel.waitFor({state:'hidden',timeout:10000});
    await Promise.all(responses);
    assert.deepEqual(errors,[]);
    const report={base,layouts,dialogs,dialogCancelled:true,dialogScrollChangedPixels:changedPixels,blockedFallbackFonts,errors};
    writeFileSync(path.join(directory,'report.json'),JSON.stringify(report,null,2));
    console.log(JSON.stringify(report));
  } finally {await closeBrowser(browser);}
}
run().catch(error=>{console.error(error.message.split('Call log:')[0]);process.exitCode=1;});
