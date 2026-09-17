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
let registered = null;
let registrations = [];
let documentation = null;
let removed = false;
let deviceFixtures = [];
let deviceInstalls = [];
let installTasks = [];
const readBody = async req => { let body=''; for await (const chunk of req) body+=chunk; return JSON.parse(body); };
const server = createServer(async(req,res)=>{
  const send=data=>res.writeHead(200,{'content-type':'application/json'}).end(JSON.stringify({data}));
  const path=new URL(req.url,'http://localhost').pathname;
  try {
    if(path==='/api/auth/session') return send(session);
    if(path==='/api/runtime/bootstrap') return send({catalog,permissions:session.permissions});
    if(path==='/api/runtime/catalog') return send(catalog);
    if(path==='/api/runtime/marketplace') return send([plugin,...(removed?[]:[{...cli,installed:deviceFixtures.some(d=>d.installed?.state==='installed')}]),...(registered?[registered]:[])]);
    if(path==='/api/runtime/workers/tasks') return send(installTasks);
    if(path.endsWith('/devices')) return send(deviceFixtures);
    if(path.endsWith('/install') && path.startsWith('/api/runtime/tools/')) {
      const body=await readBody(req);deviceInstalls.push(body);
      installTasks=[{worker_id:body.worker_id,capability:'tools.install',state:'queued',input:{id:manifest.id},error:null}];
      return send(installTasks[0]);
    }
    if(path===`/api/runtime/tools/${manifest.id}` && req.method==='DELETE') { removed=true;return send(null); }
    if(path==='/api/runtime/tools/access') return send(true);
    if(path==='/api/runtime/tools/register') {
      const body=await readBody(req);registrations.push(body);
      const value={id:'cli-test-'+registrations.length,version:'1.0.0',title:body.metadata.git?'my-cli':'my-command',summary:'自动生成的备注',homepage:body.metadata.git,license:'',tags:['cli'],platforms:Object.fromEntries(body.platforms.map(platform=>[platform,{requirements:[],install:[{program:'bash',args:['-o','pipefail','-c',body.command]}],uninstall:[],detect:null}]))};
      registered={git:'aio-tool:'+value.id,rev:value.version,title:value.title,summary:value.summary,license:'',tags:['cli'],installed:false,cli:value};
      documentation={metadata:{title:value.title,summary:value.summary,git:value.homepage},readme:'# 来自仓库的 README\n\n这是工具的使用说明。\n\n[查看源码](src/main.rs)',link_base:'https://github.com/example/my-cli/blob/abcdef/',image_base:'https://raw.githubusercontent.com/example/my-cli/abcdef/',error:null};
      return send(value);
    }
    if(path.startsWith('/api/runtime/tools/') && path.endsWith('/details')) {
      if(req.method==='PATCH') {
        const metadata=await readBody(req);Object.assign(registered,{title:metadata.title,summary:metadata.summary});Object.assign(registered.cli,{title:metadata.title,summary:metadata.summary,homepage:metadata.git});documentation.metadata=metadata;
      }
      return send(documentation || {metadata:{git:manifest.homepage,title:manifest.title,summary:manifest.summary},readme:'# Codex README',link_base:manifest.homepage+'/',image_base:manifest.homepage+'/',error:null});
    }
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
  const browser=await chromium.launch({channel:'chrome',headless:true});
  try {
    for(const mobile of [false,true]){
      registered=null;documentation=null;registrations=[];removed=false;deviceFixtures=[];deviceInstalls=[];installTasks=[];
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
      const link=page.getByRole('link',{name:'通过本机助手安装',exact:true});
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
      await page.screenshot({path:resolve(output,mobile?'mobile.png':'desktop.png'),fullPage:true,animations:"disabled"});
      deviceFixtures=[
        {id:'mac',label:'我的 Mac',platform:'darwin',status:'online',installed:{version:'0.1.4',state:'installed'},checked_at:Date.now(),fresh:true,can_install:true,supported:true,error:null},
        {id:'linux',label:'另一台 Linux',platform:'linux',status:'online',installed:null,checked_at:Date.now(),fresh:true,can_install:true,supported:true,error:null},
        {id:'offline',label:'离线电脑',platform:'linux',status:'offline',installed:null,checked_at:Date.now()-300000,fresh:false,can_install:false,supported:true,error:null},
        {id:'old',label:'旧版助手',platform:'darwin',status:'online',installed:null,checked_at:null,fresh:false,can_install:false,supported:true,error:null},
      ];
      await page.getByRole('button',{name:'刷新设备状态',exact:true}).click();
      await page.getByText('已安装 0.1.4',{exact:true}).waitFor();
      assert.equal(await page.getByRole('link',{name:'通过本机助手安装',exact:true}).count(),0);
      assert.equal(await page.getByRole('button',{name:'安装到此设备',exact:true}).count(),1);
      await page.getByRole('button',{name:'安装到此设备',exact:true}).click();
      await page.getByRole('dialog').getByText(/另一台 Linux/).waitFor();
      assert.equal(deviceInstalls.length,0);
      await page.getByRole('button',{name:'确认安装',exact:true}).click();
      await page.getByText('正在等待设备完成安装…',{exact:true}).waitFor();
      assert.deepEqual(deviceInstalls,[{worker_id:'linux',version:manifest.version}]);
      installTasks[0].state='complete';deviceFixtures[1].installed={version:manifest.version,state:'installed'};
      await page.getByRole('button',{name:'刷新设备状态',exact:true}).click();
      await page.getByText('设备已完成安装任务',{exact:true}).waitFor();
      assert.equal(await page.getByRole('button',{name:'安装到此设备',exact:true}).count(),0);
      await page.getByRole('heading',{name:'设备安装状态',exact:true}).scrollIntoViewIfNeeded();
      assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
      await page.screenshot({path:resolve(output,mobile?'mobile-devices.png':'desktop-devices.png'),animations:'disabled'});
      await page.getByRole('button',{name:'从市场删除',exact:true}).click();
      await page.getByRole('dialog').waitFor();assert.equal(removed,false);
      await page.getByRole('button',{name:'取消',exact:true}).click();assert.equal(removed,false);
      await page.getByRole('button',{name:'从市场删除',exact:true}).click();
      await page.getByRole('button',{name:'确认删除',exact:true}).click();
      await page.getByRole('dialog').waitFor({state:'hidden'});
      assert.equal(removed,true);
      deviceFixtures=[];installTasks=[];
      if(mobile) await page.getByRole('button',{name:'插件列表',exact:true}).click();
      await page.getByRole('button',{name:'添加 CLI',exact:true}).click();
      await page.getByRole('dialog').waitFor();
      await page.getByRole('textbox',{name:'安装命令',exact:true}).fill('npx -y my-cli@1.0.0 setup');
      await page.getByRole('textbox',{name:'Git 仓库（可选）',exact:true}).fill('https://github.com/example/my-cli');
      await page.getByRole('checkbox',{name:'保存后打开本机助手安装',exact:true}).uncheck();
      await page.screenshot({path:resolve(output,mobile?'mobile-register.png':'desktop-register.png'),fullPage:true,animations:"disabled"});
      await page.getByRole('button',{name:'保存到市场',exact:true}).click();
      await page.getByRole('heading',{name:'my-cli',exact:true}).waitFor();
      await page.getByRole('heading',{name:'来自仓库的 README',exact:true}).waitFor();
      assert.equal(registrations.length,1);
      assert.equal(registrations[0].metadata.title,'');
      assert.equal(registrations[0].detect,'');
      assert.equal(registrations[0].uninstall,'');
      assert.equal(await page.getByRole('link',{name:'查看源码',exact:true}).getAttribute('href'),'https://github.com/example/my-cli/blob/abcdef/src/main.rs');
      await page.getByRole('button',{name:'编辑标题和备注',exact:true}).click();
      await page.getByRole('textbox',{name:'标题',exact:true}).fill('我的 CLI');
      await page.getByRole('textbox',{name:'备注',exact:true}).fill('团队常用工具');
      await page.getByRole('button',{name:'保存',exact:true}).click();
      await page.getByRole('heading',{name:'我的 CLI',exact:true}).waitFor();
      await page.getByRole('button',{name:'刷新 README',exact:true}).click();
      await page.getByRole('heading',{name:'来自仓库的 README',exact:true}).waitFor();
      assert.equal(installs,0);
      assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
      await page.screenshot({path:resolve(output,mobile?'mobile-readme.png':'desktop-readme.png'),fullPage:true,animations:"disabled"});
      if(mobile) await page.getByRole('button',{name:'插件列表',exact:true}).click();
      await page.getByRole('button',{name:'添加 CLI',exact:true}).click();
      await page.getByRole('textbox',{name:'安装命令',exact:true}).fill('node --version');
      const cdp=await context.newCDPSession(page);await cdp.send('Page.enable');
      const navigation=new Promise(resolve=>cdp.on('Page.frameRequestedNavigation',event=>{if(event.url.startsWith('aio://')) resolve(event.url);}));
      await page.getByRole('button',{name:'保存并安装',exact:true}).click();
      const uri=await Promise.race([navigation,new Promise((_,reject)=>setTimeout(()=>reject(Error('No helper URI requested')),5000))]);
      assert.equal(uri,'aio://install/cli-test-2?version=1.0.0');
      await page.getByRole('heading',{name:'my-command',exact:true}).waitFor();
      assert.equal(registrations[1].metadata.git,'');
      await cdp.detach();
      assert.deepEqual(errors,[]);
      await context.close();
    }
    console.log('CLI marketplace: desktop/mobile, installed detection, target-device confirmation, offline/old workers, removal/cancel, registration and helper URI passed');
  }finally{await browser.close();server.close();}
})().catch(error=>{console.error(error);server.close();process.exitCode=1;});
