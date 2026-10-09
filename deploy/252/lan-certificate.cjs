#!/usr/bin/env node
'use strict';
const {readFile,writeFile,mkdir,rename}=require('node:fs/promises');
const {join}=require('node:path');
const {createRequire}=require('node:module');
const {X509Certificate}=require('node:crypto');
const acme=createRequire(process.env.AIO_LAN_TOOLS || join(__dirname,'../package.json'))('acme-client');

// 只读取已有的 Cloudflare 域名凭据，不写入源码、命令参数或任务日志。
async function main() {
  const domain=process.env.AIO_LAN_DOMAIN;
  const address=process.env.AIO_LAN_ADDRESS;
  const directory=process.env.AIO_LAN_TLS_DIR || '/opt/aio-idea/lan';
  if(!domain||!address||!/^[a-z0-9.-]+$/.test(domain)){throw new Error('缺少局域网域名或地址配置');}
  const pem=await readFile(process.env.AIO_CLOUDFLARE_CERT || '/root/.cloudflared/cert.pem','utf8');
  const block=pem.match(/-----BEGIN ARGO TUNNEL TOKEN-----([\s\S]*?)-----END ARGO TUNNEL TOKEN-----/);
  if(!block){throw new Error('Cloudflare 凭据格式无效');}
  const {apiToken,zoneID}=JSON.parse(Buffer.from(block[1].replace(/\s/g,''),'base64'));
  const api=async(path,method='GET',body)=>{
    const response=await fetch(`https://api.cloudflare.com/client/v4/zones/${zoneID}/${path}`,{
      method,headers:{authorization:`Bearer ${apiToken}`,'content-type':'application/json'},
      body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(20000),
    });
    const value=await response.json();
    if(!response.ok||!value.success){throw new Error(`Cloudflare 域名操作失败（HTTP ${response.status}）`);}
    return value.result;
  };
  const records=await api('dns_records?name='+encodeURIComponent(domain));
  if(records.length) {
    if(records.length!==1||records[0].type!=='A'||records[0].content!==address||records[0].proxied){throw new Error('局域网域名已有不同配置，保留原记录');}
  }else{await api('dns_records','POST',{type:'A',name:domain,content:address,ttl:120,proxied:false});}
  await mkdir(directory,{recursive:true,mode:0o700});
  const certificate=join(directory,'certificate.pem');
  try {
    const current=new X509Certificate(await readFile(certificate));
    if(current.checkHost(domain)&&Date.parse(current.validTo)-Date.now()>30*86400_000){console.log('局域网证书仍有效，无需续期。');return;}
  }catch(error){if(error.code!=='ENOENT'){throw error;}}
  const keyFile=join(directory,'account.key');
  let accountKey;
  try{accountKey=await readFile(keyFile);}
  catch(error){if(error.code!=='ENOENT'){throw error;}accountKey=await acme.crypto.createPrivateKey();await writeFile(keyFile,accountKey,{mode:0o600});}
  const client=new acme.Client({directoryUrl:acme.directory.letsencrypt.production,accountKey});
  const [key,csr]=await acme.crypto.createCsr({commonName:domain});
  const challenges=new Map();
  const cert=await client.auto({csr,termsOfServiceAgreed:true,challengePriority:['dns-01'],
    challengeCreateFn:async(auth,challenge,keyAuthorization)=>{
      const name='_acme-challenge.'+auth.identifier.value;
      const record=await api('dns_records','POST',{type:'TXT',name,content:keyAuthorization,ttl:120});
      challenges.set(challenge.url,record.id);
    },
    challengeRemoveFn:async(_auth,challenge)=>{
      const id=challenges.get(challenge.url);
      if(id){await api('dns_records/'+id,'DELETE');challenges.delete(challenge.url);}
    },
  });
  const issued=new X509Certificate(cert);
  if(!issued.checkHost(domain)||Date.parse(issued.validTo)<=Date.now()){throw new Error('签发证书校验失败');}
  await writeFile(join(directory,'private.key.next'),key,{mode:0o600});
  await writeFile(certificate+'.next',cert,{mode:0o600});
  await rename(join(directory,'private.key.next'),join(directory,'private.key'));
  await rename(certificate+'.next',certificate);
  console.log('局域网 HTTPS 证书已签发并校验。');
}
main().catch(error=>{console.error('局域网证书准备失败：'+error.message);process.exitCode=1;});
