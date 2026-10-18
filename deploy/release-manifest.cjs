#!/usr/bin/env node
// 生成启动器信任的发布清单：只列出必须存在的相对路径及其 SHA-256。
// 启动器在切换与 boot 前逐项复核，任何差异都会阻止发布并触发回滚。
const {createHash} = require('node:crypto');
const {readFileSync, statSync, writeFileSync} = require('node:fs');
const {join, relative, resolve} = require('node:path');

const [releaseArgument, revision] = process.argv.slice(2);
if (!releaseArgument || !/^[0-9a-f]{40}$/.test(revision || '')) {
  console.error('用法: release-manifest.cjs <发布目录> <40 位 Git SHA>');
  process.exit(64);
}
const release = resolve(releaseArgument);
const binary = 'aio-idea';
if (!statSync(join(release, binary)).isFile()) {
  console.error('发布目录缺少 ' + binary);
  process.exit(66);
}

// 摘要以真实文件内容为准，不信任构建缓存或符号链接。
const digest = path => createHash('sha256').update(readFileSync(join(release, path))).digest('hex');
const manifest = {
  revision,
  host_binary: binary,
  host_sha256: digest(binary),
  web_index: 'web/index.html',
  extra_files: {
    'web/index.html': digest('web/index.html'),
    'aio.toml': digest('aio.toml'),
  },
};
writeFileSync(join(release, 'release-manifest.json'), JSON.stringify(manifest, null, 2) + '\n');
console.log('release manifest=' + relative(process.cwd(), join(release, 'release-manifest.json')) + ' revision=' + revision);
