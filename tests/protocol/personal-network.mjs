import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {randomUUID} from 'node:crypto';
import {setTimeout as pause} from 'node:timers/promises';
const exec=promisify(execFile),origin='https://aio.addzero.site';
const host=process.env.AIO_NETWORK_TEST_HOST??'root@192.168.31.252';
const cli=process.env.AIO_SPACE_TEST_CLI;
const cookie=(await readFile(process.env.AIO_COOKIE_FILE,'utf8')).split(/\r?\n/).filter(v=>v&&(!v.startsWith('#')||v.startsWith('#HttpOnly_'))).map(v=>v.split('\t')).map(v=>v[5]+'='+v[6]).join('; ');
const prefix='aio-config-'+randomUUID().slice(0,8),network=prefix+'-net',alternate=prefix+'-new';
const file='/fixture/.config/'+prefix+'.json';
const names=[prefix+'-a',prefix+'-b'],devices=[],pids=[];const report={scenarios:[],devices};
const record=label=>{report.scenarios.push(label);console.log(label);};
const output=process.env.AIO_NETWORK_REPORT??'/tmp/aio-personal-network-result.json';
const quote=v=>"'"+String(v).replaceAll("'","'\\''")+"'";
const remote=async args=>(await exec('ssh',[host,args.map(quote).join(' ')],{timeout:60_000,maxBuffer:1024*1024})).stdout.trim();
const docker=(...args)=>remote(['docker',...args]);
const inside=(index,code)=>docker('exec',names[index],'node','--input-type=module','-e',code);
async function api(path,method='GET',body){const response=await fetch(origin+'/api/runtime'+path,{method,headers:{cookie,'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(30000)});if(!response.ok)throw new Error(`HTTP ${response.status} ${path}`);return (await response.json()).data;}
const firewall=(index,...args)=>{assert.match(pids[index],/^\d+$/);return remote(['nsenter','-t',pids[index],'-n','--','iptables',...args]);};
async function wait(condition,seconds=75){const end=Date.now()+seconds*1000;while(Date.now()<end){if(await condition())return;await pause(1000);}throw new Error('等待配置同步超时');}
async function value(index){try{return JSON.parse(await inside(index,"import {readFileSync} from 'node:fs';console.log(readFileSync("+JSON.stringify(file)+",'utf8'));"));}catch{return null;}}
let entry;
try{
 await api('/personal-config/catalog');await docker('network','create',network);await docker('network','create',alternate);
 const temporary='/tmp/'+prefix+'.mjs';await exec('scp',[cli,host+':'+temporary]);
 for(let index=0;index<2;index++){
  await docker('run','-d','--name',names[index],'--label','aio.protocol-test=config','--network',network,'node:22.23.1-bookworm-slim','sleep','infinity');
  assert.equal(await docker('inspect','--format','{{index .Config.Labels "aio.protocol-test"}}',names[index]),'config');
  pids[index]=await docker('inspect','--format','{{.State.Pid}}',names[index]);await docker('cp',temporary,names[index]+':/worker.mjs');
  const pair=JSON.parse(await inside(index,`import {mkdirSync,writeFileSync} from 'node:fs';mkdirSync('/fixture');mkdirSync('/state',{mode:448});const origin=${JSON.stringify(origin)};const response=await fetch(origin+'/api/runtime/workers/pairings',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({label:${JSON.stringify(names[index])},platform:'linux',capabilities:['space.scan']})});if(!response.ok)throw new Error('pair');const {data}=await response.json();writeFileSync('/state/worker.json',JSON.stringify({origin,token:data.token,deviceId:data.device_id,root:'/fixture'}),{mode:384});console.log(JSON.stringify({device:data.device_id,code:data.code}));`));
  devices.push(pair.device);await api('/workers/pairings/'+pair.code,'POST',{});
  await docker('exec','-e','AIO_SPACE_CONFIG_DIR=/state',names[index],'node','/worker.mjs','config-enable','--root','/fixture','--foreground');
  await firewall(index,'-A','INPUT','-i','lo','-j','ACCEPT');await firewall(index,'-A','INPUT','-m','conntrack','--ctstate','ESTABLISHED,RELATED','-j','ACCEPT');await firewall(index,'-P','INPUT','DROP');
  await firewall(index,'-A','OUTPUT','-o','lo','-j','ACCEPT');for(const protocol of ['udp','tcp']){await firewall(index,'-A','OUTPUT','-p',protocol,'--dport','53','-j','ACCEPT');}await firewall(index,'-A','OUTPUT','-p','tcp','--dport','443','-j','ACCEPT');await firewall(index,'-P','OUTPUT','DROP');
  await docker('exec','-d','-e','AIO_SPACE_CONFIG_DIR=/state',names[index],'sh','-c','echo $$ > /worker.pid; exec node /worker.mjs worker > /worker.log 2>&1');
  assert.equal(await docker('port',names[index]),'');
 }
 await remote(['rm','-f',temporary]);
 const write=async content=>{entry=await api('/personal-config/entries','POST',{id:entry?.id??randomUUID(),expected:entry?.revision??null,kind:'file',target:file.slice('/fixture/'.length),layer:'shared',format:'jsonc',secret:true,executable:false,deleted:false,content:JSON.stringify(content)});};
 await write({online:0,offline:0});await wait(async()=>{const a=await value(0),b=await value(1);return a?.online===0&&b?.offline===0;});record('两台 NAT 设备仅 DNS/HTTPS 出站，无映射端口，接收同一账号配置');
 await firewall(1,'-I','OUTPUT','1','-p','tcp','--dport','443','-j','REJECT','--reject-with','tcp-reset');
 await inside(1,"import {writeFileSync} from 'node:fs';writeFileSync("+JSON.stringify(file)+",JSON.stringify({online:0,offline:2}));");
 await write({online:1,offline:0});await wait(async()=>(await value(0))?.online===1);assert.equal((await value(1)).online,0);await pause(2000);
 await firewall(1,'-D','OUTPUT','1');await wait(async()=>{const a=await value(0),b=await value(1);return a?.online===1&&a?.offline===2&&b?.online===1&&b?.offline===2;});record('防火墙阻断设备 B 时双方各改一个键，解除后合并并回传设备 A');
 const address=await docker('inspect','--format','{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}',names[1]);await docker('network','connect',alternate,names[1]);await docker('network','disconnect',network,names[1]);const changed=await docker('inspect','--format','{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}',names[1]);assert.notEqual(address,changed);
 await inside(1,"import {writeFileSync} from 'node:fs';writeFileSync("+JSON.stringify(file)+",JSON.stringify({online:1,offline:3}));");await wait(async()=>(await value(0))?.offline===3);record('NAT 地址变化后保持原设备身份继续双向同步');
 await api('/workers/'+devices[1],'DELETE');await wait(async()=>inside(1,"import {readFileSync} from 'node:fs';const pid=readFileSync('/worker.pid','utf8').trim();try{console.log(readFileSync('/proc/'+pid+'/stat','utf8').includes(') Z '));}catch{console.log(true);}").then(v=>v==='true'),45);record('撤销设备 B 后 worker 退出，设备 A 保持配对');report.passed=true;
}catch(error){report.passed=false;report.error=error.message;throw error;}
finally{
 if(entry){try{const current=(await api('/personal-config/entries/'+entry.id)).entry;await api('/personal-config/entries','POST',{id:current.id,expected:current.revision,kind:current.kind,target:current.target,layer:current.layer,format:current.format,secret:current.secret,executable:current.executable,deleted:true,content:''});}catch{}}
 for(const device of devices){await api('/workers/'+device,'DELETE').catch(()=>{});}for(const name of names){await docker('rm','-f',name).catch(()=>{});}for(const name of [network,alternate]){await docker('network','rm',name).catch(()=>{});}await remote(['rm','-f','/tmp/'+prefix+'.mjs']).catch(()=>{});
 await writeFile(output,JSON.stringify(report,null,2));console.log(JSON.stringify(report));
}
