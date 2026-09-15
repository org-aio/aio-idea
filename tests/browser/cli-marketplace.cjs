const assert = require('node:assert/strict');
const { createServer } = require('node:http');
const { readFile, mkdir } = require('node:fs/promises');
const { resolve, extname } = require('node:path');
const { chromium } = require('playwright');

const root = resolve(process.env.AIO_WEB_ROOT || 'target/dx/aio-idea/release/web/public');
const output = resolve('target/cli-marketplace-test');
const manifest = require('./fixtures/cli/manifest.json');
const cli = {git:'aio-tool:'+manifest.id,rev:manifest.version,title:manifest.title,summary:manifest.summary,license:manifest.license,tags:manifest.tags,installed:false,cli:manifest};
const plugin = {git:'https://example.com/normal.git',rev:'a'.repeat(64),title:'普通插件',summary:'宿主插件',license:'MIT',tags:['cli'],installed:false};
const session = {user_id:'test',account:'test',display_name:'Test',tenant_id:'test',tenant_label:'Test',permissions:['plugin:manage']};
const catalog = {session_context:'test',context:'test',tenant:{id:'test',label:'Test'},user:{label:'Test',handle:'test',initials:'T'},plugins:[],pages:[],page_versions:{},account_items:[]};
let installs = 0;
const server = createServer(async(req,res)=>{
  const send=data=>res.writeHead(200,{'content-type':'application/json'}).end(JSON.stringify({data}));
  const path=new URL(req.url,'http://localhost').pathname;
  try {
    if(path==='/api/auth/session') return send(session);
    if(path==='/api/runtime/bootstrap') return send({catalog,permissions:session.permissions});
    if(path==='/api/runtime/catalog') return send(catalog);
    if(path==='/api/runtime/marketplace') return send([plugin,cli]);
    if(path==='/api/runtime/plugins/install') { installs++; return send(catalog); }
    if(path.endsWith('/details')) return send({readme:'普通宿主插件',versions:[],builds:[]});
    if(path.startsWith('/api/')) return send([]);
    const file=resolve(root,'.'+(path==='/'?'/index.html':path));
    assert(file.startsWith(root+'/'));
    const mime={'.html':'text/html; charset=utf-8','.js':'text/javascript','.css':'text/css','.wasm':'application/wasm','.woff2':'font/woff2'};
    res.writeHead(200,{'content-type':mime[extname(file)]||'application/octet-stream'}).end(await readFile(file));
  }catch(error){res.writeHead(500).end(error.message);}
});
(async()=>{
  await mkdir(output,{recursive:true});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const browser=await chromium.launch({headless:true});
  try {
    for(const mobile of [false,true]){
      const context=await browser.newContext({viewport:mobile?{width:390,height:844}:{width:1440,height:1000}});
      const page=await context.newPage();const errors=[];
      page.on('pageerror',error=>errors.push(error.message));
      await page.goto('http://127.0.0.1:'+server.address().port);
      if(mobile) await page.getByRole('button',{name:'打开菜单',exact:true}).click();
      await page.locator('button[aria-label$="的账户菜单"]').last().click();
      await page.getByRole('menuitem',{name:'插件市场',exact:true}).click();
      const tree=page.getByRole('tree',{name:'插件',exact:true});
      await tree.getByRole('treeitem').first().waitFor();
      await page.getByRole('button',{name:'CLI',exact:true}).click();
      assert.equal(await tree.getByRole('treeitem').count(),1);
      await tree.getByRole('treeitem').click();
      await page.getByRole('heading',{name:manifest.title,exact:true}).waitFor();
      const link=page.getByRole('link',{name:'安装到本机',exact:true});
      assert.equal(await link.getAttribute('href'),'aio://install/codex-model-sync?version=0.1.4');
      // 拦截导航以验证点击目标，避免在浏览器测试中执行真实用户安装。
      await link.evaluate(element=>element.addEventListener('click',event=>{event.preventDefault();window.installLink=element.href;}));
      await link.click();
      assert.equal(await page.evaluate(()=>window.installLink),'aio://install/codex-model-sync?version=0.1.4');
      assert.equal(installs,0);
      for(const name of ['Windows','Linux','macOS']) await page.getByRole('tab',{name,exact:true}).click();
      await page.getByText('查看依赖和实际操作',{exact:true}).click();
      assert(await page.getByText('codex-model-sync ["uninstall"]',{exact:true}).count());
      assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
      await page.getByRole('heading',{name:manifest.title,exact:true}).scrollIntoViewIfNeeded();
      await page.screenshot({path:resolve(output,mobile?'mobile.png':'desktop.png'),fullPage:true});
      assert.deepEqual(errors,[]);
      await context.close();
    }
    console.log('CLI marketplace: desktop/mobile, typed filter, exact URI, no host install, uninstall steps passed');
  }finally{await browser.close();server.close();}
})().catch(error=>{console.error(error);server.close();process.exitCode=1;});
