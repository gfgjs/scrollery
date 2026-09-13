#!/usr/bin/env node
// 把 video-worker(MSVC workspace member)编译并暂存进 Tauri externalBin 目录。
//
// video-worker 与主程序使用同一 Rust 工具链，不需要 RAW worker 的 GNU 交叉编译旁路；
// Tauri 仍要求 sidecar 按「目标 triple」命名，故这里把 release 产物复制为
// binaries/video-worker-<host-triple>.exe。开发版使用 debug profile，既满足
// Tauri 编译期 externalBin 的资源检查，也让运行期 resolver 能在 target/debug
// 同目录找到同一份 worker。
//
// 非 Windows 与现有 AI/RAW sidecar 脚本保持一致：当前 Windows 发布链路之外的 sidecar
// 打包不在本批扩展，跳过不阻断跨平台前端构建。

import { spawnSync } from 'node:child_process';
import { mkdirSync, copyFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const binariesDir = path.join(repo, 'src-tauri', 'binaries');

if (process.platform !== 'win32') {
  console.log(
    `[build-video-worker] 非 Windows(${process.platform})——跳过 video-worker sidecar 编译;` +
      'mac/Linux sidecar 打包待后续落地。'
  );
  process.exit(0);
}

const isDev = process.argv.includes('--dev');
const profile = isDev ? 'debug' : 'release';
const cargoArgs = ['build', '-p', 'video-worker', ...(isDev ? [] : ['--release'])];
console.log(
  `[build-video-worker] cargo ${cargoArgs.join(' ')}(cwd=workspace 根; profile=${profile})`
);
const build = spawnSync('cargo', cargoArgs, {
  cwd: repo,
  stdio: 'inherit',
  shell: false,
});
if (build.status !== 0) {
  console.error(`[build-video-worker] cargo build 失败(exit=${build.status ?? build.signal})`);
  process.exit(build.status ?? 1);
}

const rustc = spawnSync('rustc', ['-vV'], { cwd: repo, encoding: 'utf8', shell: false });
if (rustc.status !== 0) {
  console.error(`[build-video-worker] rustc -vV 失败(exit=${rustc.status})`);
  process.exit(rustc.status ?? 1);
}
const match = /^host:\s*(\S+)$/m.exec(rustc.stdout);
if (!match) {
  console.error('[build-video-worker] 无法从 rustc -vV 输出解析 host triple');
  process.exit(1);
}

const hostTriple = match[1];
const source = path.join(repo, 'target', profile, 'video-worker.exe');
if (!existsSync(source)) {
  console.error(`[build-video-worker] ${profile} 产物缺失(不应发生):${path.relative(repo, source)}`);
  process.exit(1);
}

mkdirSync(binariesDir, { recursive: true });
const destination = path.join(binariesDir, `video-worker-${hostTriple}.exe`);
copyFileSync(source, destination);
console.log(
  `[build-video-worker] 已暂存 sidecar:${path.relative(repo, destination)}(源 ${path.relative(repo, source)})`
);
