#!/usr/bin/env node
// 独立包复用图片处理源码，避免构建 worker 时编译完整应用及改写 Tauri 配置。

import { spawnSync } from 'node:child_process';
import { mkdirSync, copyFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const binariesDir = path.join(repo, 'src-tauri', 'binaries');

if (process.platform !== 'win32') {
  console.log(
    `[build-native-thumbnail-worker] 非 Windows(${process.platform})——跳过 native-thumbnail-worker sidecar 编译;` +
      'mac/Linux sidecar 打包待后续落地。'
  );
  process.exit(0);
}

const isDev = process.argv.includes('--dev');
const profile = isDev ? 'debug' : 'release';
const cargoArgs = ['build', '-p', 'native-thumbnail-worker', ...(isDev ? [] : ['--release'])];
console.log(
  `[build-native-thumbnail-worker] cargo ${cargoArgs.join(' ')}(cwd=workspace 根; profile=${profile})`
);
const build = spawnSync('cargo', cargoArgs, {
  cwd: repo,
  stdio: 'inherit',
  shell: false,
});
if (build.status !== 0) {
  console.error(`[build-native-thumbnail-worker] cargo build 失败(exit=${build.status ?? build.signal})`);
  process.exit(build.status ?? 1);
}

const rustc = spawnSync('rustc', ['-vV'], { cwd: repo, encoding: 'utf8', shell: false });
if (rustc.status !== 0) {
  console.error(`[build-native-thumbnail-worker] rustc -vV 失败(exit=${rustc.status})`);
  process.exit(rustc.status ?? 1);
}
const match = /^host:\s*(\S+)$/m.exec(rustc.stdout);
if (!match) {
  console.error('[build-native-thumbnail-worker] 无法从 rustc -vV 输出解析 host triple');
  process.exit(1);
}

const hostTriple = match[1];
const source = path.join(repo, 'target', profile, 'native-thumbnail-worker.exe');
if (!existsSync(source)) {
  console.error(`[build-native-thumbnail-worker] ${profile} 产物缺失(不应发生):${path.relative(repo, source)}`);
  process.exit(1);
}

mkdirSync(binariesDir, { recursive: true });
const destination = path.join(binariesDir, `native-thumbnail-worker-${hostTriple}.exe`);
copyFileSync(source, destination);
console.log(
  `[build-native-thumbnail-worker] 已暂存 sidecar:${path.relative(repo, destination)}(源 ${path.relative(repo, source)})`
);

