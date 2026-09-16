const assert=require('node:assert/strict');
const {writeFile}=require('node:fs/promises');
const {launchBrowser,contextFor,closeBrowser,viewportScreenshot}=require('./live-session.cjs');
const base=process.env.AIO_BASE_URL||'https://aio.addzero.site';
(async()=>{
 const browser=await launchBrowser();const results=[];
 try{
  for(const mobile of [false,true]){
   const context=await contextFor(browser,base,mobile);const page=await context.newPage();if(!mobile){await page.setViewportSize({width:1440,height:900});}const errors=[];
   page.on('pageerror',e=>errors.push(e.message));
   await page.goto(base,{waitUntil:'domcontentloaded'});
   if(mobile){await page.getByRole('button',{name:'打开菜单',exact:true}).click();}
   await page.locator('button[aria-label$="的账户菜单"]').filter({visible:true}).click();
   await page.getByRole('menuitem',{name:'个人配置',exact:true}).click();
   const panel=page.getByRole('dialog').filter({has:page.getByRole('heading',{name:'个人配置',exact:true})});
   await panel.getByRole('button',{name:'新增',exact:true}).waitFor();
   await panel.getByRole('tab',{name:'环境变量',exact:true}).click();
   await panel.getByRole('button',{name:'新增',exact:true}).click();
   const editor=page.getByRole('dialog').filter({has:page.getByRole('heading',{name:'新增个人配置',exact:true})});
   const target=`AIO_BROWSER_CHECK_${mobile?'MOBILE':'DESKTOP'}_${Date.now()}`;
   await editor.getByRole('textbox',{name:'环境变量名（PATH 表示附加目录）',exact:true}).fill(target);
   await editor.getByRole('textbox',{name:'配置内容',exact:true}).fill('first value');
   await editor.getByRole('button',{name:'保存并同步',exact:true}).click();
   await editor.waitFor({state:'hidden'});await panel.getByRole('textbox',{name:'搜索配置',exact:true}).fill(target);
   const row=panel.locator('section').filter({hasText:target});await row.getByRole('button',{name:'编辑',exact:true}).click();
   const edit=page.getByRole('dialog').filter({has:page.getByRole('heading',{name:'编辑个人配置',exact:true})});
   await edit.getByRole('textbox',{name:'配置内容',exact:true}).fill('second value');await edit.getByRole('button',{name:'保存并同步',exact:true}).click();await edit.waitFor({state:'hidden'});
   await row.getByRole('button',{name:'历史版本',exact:true}).click();
   const history=page.getByRole('dialog').filter({has:page.getByRole('heading',{name:'历史版本',exact:true})});
   await history.getByRole('button',{name:/^版本 /}).first().click();await history.getByText('first value',{exact:true}).waitFor();
   await history.getByRole('button',{name:'确认恢复此版本',exact:true}).click();await history.waitFor({state:'hidden'});
   const catalog=(await (await context.request.get(base+'/api/runtime/personal-config/catalog')).json()).data;
   const entry=catalog.entries.find(e=>e.target===target);assert.ok(entry);
   const content=(await (await context.request.get(base+'/api/runtime/personal-config/entries/'+entry.id)).json()).data;assert.equal(content.content,'first value');
   await viewportScreenshot(page,`/tmp/aio-personal-${mobile?'mobile':'desktop'}.png`);
   const layout=await page.evaluate(()=>({width:innerWidth,scroll:document.documentElement.scrollWidth,dialogs:[...document.querySelectorAll('[role="dialog"]')].map(e=>({x:e.getBoundingClientRect().x,width:e.getBoundingClientRect().width,bottom:e.getBoundingClientRect().bottom}))}));
   assert.ok(layout.scroll<=layout.width+1,JSON.stringify(layout));assert.equal(errors.length,0,errors.join('\n'));
   await row.getByRole('button',{name:'删除',exact:true}).click();await page.getByRole('button',{name:'确认删除',exact:true}).click();await row.getByText(/已删除/).waitFor();
   await panel.getByRole('tab',{name:'同步设备',exact:true}).click();await panel.getByText(/每台设备单独配对/).waitFor();
   await panel.getByRole('button',{name:'关闭',exact:true}).click();
   results.push({mobile,layout,errors,createEditRestoreDelete:true,fixtureId:entry.id});
  }
  await writeFile('/tmp/aio-personal-browser-result.json',JSON.stringify(results,null,2));console.log(JSON.stringify(results));
 }finally{await closeBrowser(browser);}
})().catch(error=>{console.error(error);process.exitCode=1;});
