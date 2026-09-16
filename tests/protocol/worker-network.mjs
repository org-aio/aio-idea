import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {randomUUID} from 'node:crypto';
import {resolve,dirname} from 'node:path';
import {setTimeout as pause} from 'node:timers/promises';

// 仅在新建的 Docker 网络命名空间内更改防火墙，不修改宿主规则。
const exec=promisify(execFile);
const origin='https://aio.addzero.site';
const host=process.env.AIO_NETWORK_TEST_HOST ?? 'root@192.168.31.252';
const cli=resolve(process.env.AIO_SPACE_TEST_CLI ?? '../aio-plugin-space/dist/cli.mjs');
const cookieSource=await readFile(process.env.AIO_COOKIE_FILE,'utf8');
const cookie=cookieSource.split(/\r?\n/).filter(line=>line && (!line.startsWith('#') || line.startsWith('#HttpOnly_'))).map(line=>line.split('\t')).map(parts=>`${parts[5]}=${parts[6]}`).join('; ');
const name='aio-channel-'+randomUUID().slice(0,8);const network=name+'-net';const alternate=name+'-new';const proxy=name+'-proxy';
const report={started:new Date().toISOString(),origin,scenarios:[],device:null};
const output=resolve(process.env.AIO_NETWORK_REPORT ?? 'target/worker-network/result.json');
await mkdir(dirname(output),{recursive:true});
const quote=value=>"'"+String(value).replaceAll("'","'\\''")+"'";
const remote=async args=>(await exec('ssh',[host,args.map(quote).join(' ')],{timeout:60_000,maxBuffer:1024*1024})).stdout.trim();
const docker=(...args)=>remote(['docker',...args]);
const inside=code=>docker('exec',name,'node','--input-type=module','-e',code);
async function api(path,method='GET',body){
  const response=await fetch(origin+'/api/runtime/workers'+path,{method,headers:{cookie,'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(30_000)});
  if(!response.ok){throw new Error(`管理请求失败 HTTP ${response.status}`);}
  return (await response.json()).data;
}
async function wait(condition,seconds=75){const end=Date.now()+seconds*1000;while(Date.now()<end){if(await condition()){return;}await pause(1500);}throw new Error('等待网络恢复超时');}
async function record(scenario,details){report.scenarios.push({scenario,...details});await writeFile(output,JSON.stringify(report,null,2));console.log(JSON.stringify({scenario,...details}));}
let device;let pid;
async function firewall(...args){
  assert.ok(/^\d+$/.test(pid));
  // PID 从带专用标签的新建容器读取；只进入该容器的网络命名空间。
  return remote(['nsenter','-t',pid,'-n','--','iptables',...args]);
}
async function scan(label){
  const id=randomUUID();const start=Date.now();
  await api('/tasks','POST',{id,worker_id:device,capability:'space.scan',input:{path:'/fixture',depth:0}});
  let task;await wait(async()=>{task=await api('/tasks/'+id);return !['queued','running'].includes(task.state);});
  assert.equal(task.state,'complete');assert.equal(task.result.root,'/fixture');
  return {label,task:id,milliseconds:Date.now()-start};
}
async function startWorker(proxyUrl){
  const args=['exec','-d','-e','AIO_SPACE_CONFIG_DIR=/state'];
  if(proxyUrl){args.push('-e','HTTPS_PROXY='+proxyUrl,'-e','NO_PROXY=');}
  await docker(...args,name,'sh','-c','echo $$ > /worker.pid; exec node /worker.mjs worker > /worker.log 2>&1');
}
async function stopped(){return inside("import {readFileSync} from 'node:fs';const pid=readFileSync('/worker.pid','utf8').trim();try{const stat=readFileSync('/proc/'+pid+'/stat','utf8');console.log(stat.includes(') Z '));}catch{console.log(true);}").then(s=>s==='true');}
try {
  await api('');
  await docker('network','create',network);await docker('network','create',alternate);
  await docker('run','-d','--name',name,'--label','aio.protocol-test=channel','--network',network,'node:22.23.1-bookworm-slim','sleep','infinity');
  assert.equal(await docker('inspect','--format','{{index .Config.Labels "aio.protocol-test"}}',name),'channel');
  pid=await docker('inspect','--format','{{.State.Pid}}',name);
  const temporary='/tmp/'+name+'.mjs';await exec('scp',[cli,host+':'+temporary]);await docker('cp',temporary,name+':/worker.mjs');await remote(['rm','-f',temporary]);
  await inside("import {mkdirSync,writeFileSync} from 'node:fs';mkdirSync('/fixture');writeFileSync('/fixture/record','channel fixture');mkdirSync('/state',{mode:448});");
  const paired=JSON.parse(await inside(`import {writeFileSync} from 'node:fs';const origin=${JSON.stringify(origin)};const response=await fetch(origin+'/api/runtime/workers/pairings',{method:'POST',headers:{'content-type':'application/json'},signal:AbortSignal.timeout(20000),body:JSON.stringify({label:${JSON.stringify(name)},platform:'linux',capabilities:['space.scan']})});if(!response.ok)throw new Error('pair '+response.status);const {data}=await response.json();writeFileSync('/state/worker.json',JSON.stringify({origin,token:data.token,deviceId:data.device_id,root:'/fixture'}),{mode:384});console.log(JSON.stringify({device:data.device_id,code:data.code}));`));
  device=paired.device;report.device=device;await api('/pairings/'+paired.code,'POST',{});
  // 额外探针证明新入站连接确实被阻断，worker 本身不监听端口。
  await docker('exec','-d',name,'node','-e',"require('node:http').createServer((q,r)=>r.end('probe')).listen(31337,'0.0.0.0')");
  const address=await docker('inspect','--format','{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}',name);
  await pause(500);await remote(['timeout','3','bash','-c',`exec 3<>/dev/tcp/${address}/31337`]);
  await firewall('-A','INPUT','-i','lo','-j','ACCEPT');await firewall('-A','INPUT','-m','conntrack','--ctstate','ESTABLISHED,RELATED','-j','ACCEPT');await firewall('-P','INPUT','DROP');
  await firewall('-A','OUTPUT','-o','lo','-j','ACCEPT');await firewall('-A','OUTPUT','-p','udp','--dport','53','-j','ACCEPT');await firewall('-A','OUTPUT','-p','tcp','--dport','53','-j','ACCEPT');await firewall('-A','OUTPUT','-p','tcp','--dport','443','-j','ACCEPT');await firewall('-P','OUTPUT','DROP');
  await assert.rejects(remote(['timeout','3','bash','-c',`exec 3<>/dev/tcp/${address}/31337`]));
  await startWorker();await record('NAT + 禁止新入站 + 仅 DNS/HTTPS 出站',{address,ports:await docker('port',name),...(await scan('inbound-blocked')),firewall:await firewall('-L','INPUT','-n','-v')});
  await firewall('-I','OUTPUT','1','-p','tcp','--dport','443','-j','REJECT','--reject-with','tcp-reset');await pause(5_000);await firewall('-D','OUTPUT','1');
  await record('连接重置后恢复',await scan('tcp-reset'));
  await firewall('-I','OUTPUT','1','-p','tcp','--dport','443','-j','DROP');
  const blocked=Date.now();await wait(async()=>{const workers=await api('');return workers.find(w=>w.id===device)?.status==='offline';},125);
  await record('全部 HTTPS 出站阻断时显示离线',{milliseconds:Date.now()-blocked,firewall:await firewall('-L','OUTPUT','-n','-v')});
  await firewall('-D','OUTPUT','1');await record('解除出站阻断后自动恢复',await scan('egress-restored'));
  await docker('network','connect',alternate,name);await docker('network','disconnect',network,name);
  const changed=await docker('inspect','--format','{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}',name);assert.notEqual(changed,address);
  await record('更换 NAT 子网地址后自动恢复',{from:address,to:changed,...(await scan('nat-rebinding'))});
  // 通过独立 HTTP CONNECT 代理访问真实 HTTPS，直接 443 出站仍保持封锁。
  await docker('run','-d','--name',proxy,'--label','aio.protocol-test=channel','--network',alternate,'node:22.23.1-bookworm-slim','node','-e',"const http=require('node:http'),net=require('node:net');http.createServer().on('connect',(req,client,head)=>{if(req.url!=='aio.addzero.site:443'){client.end('HTTP/1.1 403 Forbidden\\r\\n\\r\\n');return;}const upstream=net.connect(443,'aio.addzero.site',()=>{client.write('HTTP/1.1 200 Connection Established\\r\\n\\r\\n');if(head.length)upstream.write(head);upstream.pipe(client);client.pipe(upstream);});client.on('error',()=>upstream.destroy());upstream.on('error',()=>client.destroy());}).listen(3128,'0.0.0.0');");
  const proxyAddress=await docker('inspect','--format','{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}',proxy);
  await inside("import {readFileSync} from 'node:fs';process.kill(Number(readFileSync('/worker.pid','utf8')),'SIGTERM');");await wait(stopped,10);
  await firewall('-I','OUTPUT','1','-p','tcp','--dport','443','-j','REJECT','--reject-with','tcp-reset');
  await firewall('-I','OUTPUT','1','-d',proxyAddress,'-p','tcp','--dport','3128','-j','ACCEPT');
  await startWorker('http://'+proxyAddress+':3128');await record('直接 HTTPS 被阻断时经 CONNECT 代理工作',await scan('proxy-only'));
  await api('/'+device,'DELETE');await wait(stopped,45);
  const logs=await docker('exec',name,'cat','/worker.log');assert.match(logs,/授权已撤销/);
  await record('网页撤销设备后 worker 实际退出',{stopped:true});
  report.passed=true;
} catch(error) {report.passed=false;report.error=error.message;throw error;}
finally {
  if(device){await api('/'+device,'DELETE').catch(()=>{});}
  for(const container of [name,proxy]){await docker('rm','-f',container).catch(()=>{});}
  for(const net of [network,alternate]){await docker('network','rm',net).catch(()=>{});}
  report.finished=new Date().toISOString();await writeFile(output,JSON.stringify(report,null,2));
}
