const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {gzipSync, brotliCompressSync, constants: zlib} = require('node:zlib');
const {parse, serialize} = require('parse5');
const root = path.resolve(process.argv[2] || 'target/dx/aio-idea/release/web/public');
const assets = path.join(root, 'assets');
const file = path.join(root, 'index.html');
const document = parse(fs.readFileSync(file, 'utf8'));
const head = document.childNodes.find(node => node.tagName === 'html').childNodes.find(node => node.tagName === 'head');
const scripts=[];
function visit(node){
  if(node.tagName==='script'&&node.attrs.some(a=>a.name==='type'&&a.value==='module')){
    const src=node.attrs.find(a=>a.name==='src')?.value;
    if(src){const url=new URL(src,'https://build.invalid/');const name=path.posix.basename(url.pathname);
      if(url.origin==='https://build.invalid'&&/^aio-idea-dxh[a-f0-9]+\.js$/.test(name))scripts.push(fs.readFileSync(path.join(assets,name),'utf8'));
    }
  }
  for(const child of node.childNodes||[])visit(child);
}
visit(document);
assert.equal(scripts.length,1,'The current HTML must reference exactly one shell module');
const wasm=fs.readdirSync(assets).filter(name=>/^aio-idea_bg-dxh[a-f0-9]+\.wasm$/.test(name)&&scripts[0].includes(name));
assert.equal(wasm.length,1,'The current shell module must reference exactly one Wasm asset');
const href = `/assets/${wasm[0]}`;
const styles = fs.readdirSync(assets).filter(name => /-dxh[a-f0-9]+\.css$/.test(name));
for (const attributes of [{rel: 'preload', as: 'fetch', type: 'application/wasm', href, crossorigin: ''},
  ...styles.map(name => ({rel: 'preload', as: 'style', href: `/assets/${name}`}))]) {
  if (head.childNodes.some(node => node.tagName === 'link' && node.attrs.some(attr => attr.name === 'href' && attr.value === attributes.href))) continue;
  head.childNodes.push({nodeName: 'link', tagName: 'link', namespaceURI: 'http://www.w3.org/1999/xhtml',
    attrs: Object.entries(attributes).map(([name, value]) => ({name, value})),
    childNodes: [], parentNode: head});
}
fs.writeFileSync(file, serialize(document));
const report = [];
for (const name of fs.readdirSync(assets, {recursive: true})) {
  if (!/\.(wasm|js|css|svg|json)$/.test(name)) continue;
  const full = path.join(assets, name);
  if (!fs.statSync(full).isFile()) continue;
  const bytes = fs.readFileSync(full);
  // Brotli 与 gzip 同时预压缩：宿主按 Accept-Encoding 优先交付更小的 .br。
  const brotli = brotliCompressSync(bytes, {params: {[zlib.BROTLI_PARAM_QUALITY]: 11}});
  const compressed = gzipSync(bytes, {level: 9});
  const entry = {path: name, bytes: bytes.length};
  if (compressed.length < bytes.length) {
    fs.writeFileSync(`${full}.gz`, compressed);
    entry.gzipBytes = compressed.length;
  }
  if (brotli.length < bytes.length) {
    fs.writeFileSync(`${full}.br`, brotli);
    entry.brotliBytes = brotli.length;
  }
  if (entry.gzipBytes || entry.brotliBytes) report.push(entry);
}
console.log(JSON.stringify({preload: href, styles, assets: report}));
