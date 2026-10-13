#!/usr/bin/env node
// 把同构的 MSVC workspace member worker 编译并暂存进 Tauri externalBin 目录。
//
// enhance-worker 与 video-worker 与主程序使用同一 Rust 工具链，不需要 RAW worker 的 GNU
// 交叉编译旁路；两者的 profile 选择、host triple 命名与暂存流程一致，故共用本脚本。
// Tauri 仍要求 sidecar 按「目标 triple」命名，故这里把产物复制为
// binaries/<worker>-<host-triple>.exe。开发版使用 debug profile，既满足 Tauri 编译期
// externalBin 的资源检查，也让运行期 resolver 能在 target/debug 同目录找到同一份 worker。
//
// 非 Windows 平台跳过 sidecar 编译。
//
// 用法:node scripts/build-worker-sidecar.mjs <enhance-worker|video-worker> [--dev]

import { spawnSync } from 'node:child_process';
import { mkdirSync, copyFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// 只有这两个包名共用这一流程;其余 worker 的构建差异见各自脚本。
const WORKERS = ['enhance-worker', 'video-worker'];

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const binariesDir = path.join(repo, 'src-tauri', 'binaries');

const [worker, ...flags] = process.argv.slice(2);
if (!WORKERS.includes(worker) || flags.some((flag) => flag !== '--dev')) {
  console.error(
    `[build-worker-sidecar] 需要 worker 名 <${WORKERS.join('|')}>,收到 ${worker ?? '(缺省)'}`
  );
  process.exit(1);
}
const logTag = `build-${worker}`;

if (process.platform !== 'win32') {
  console.log(
    `[${logTag}] 非 Windows(${process.platform})——跳过 ${worker} sidecar 编译;` +
      'mac/Linux sidecar 打包待后续落地。'
  );
  process.exit(0);
}

const isDev = flags.includes('--dev');
const profile = isDev ? 'debug' : 'release';
const cargoArgs = ['build', '-p', worker, ...(isDev ? [] : ['--release'])];
console.log(
  `[${logTag}] cargo ${cargoArgs.join(' ')}(cwd=workspace 根; profile=${profile})`
);
const build = spawnSync('cargo', cargoArgs, {
  cwd: repo,
  stdio: 'inherit',
  shell: false,
});
if (build.status !== 0) {
  console.error(`[${logTag}] cargo build 失败(exit=${build.status ?? build.signal})`);
  process.exit(build.status ?? 1);
}

const rustc = spawnSync('rustc', ['-vV'], { cwd: repo, encoding: 'utf8', shell: false });
if (rustc.status !== 0) {
  console.error(`[${logTag}] rustc -vV 失败(exit=${rustc.status})`);
  process.exit(rustc.status ?? 1);
}
const match = /^host:\s*(\S+)$/m.exec(rustc.stdout);
if (!match) {
  console.error(`[${logTag}] 无法从 rustc -vV 输出解析 host triple`);
  process.exit(1);
}

const hostTriple = match[1];
const source = path.join(repo, 'target', profile, `${worker}.exe`);
if (!existsSync(source)) {
  console.error(`[${logTag}] ${profile} 产物缺失(不应发生):${path.relative(repo, source)}`);
  process.exit(1);
}

mkdirSync(binariesDir, { recursive: true });
const destination = path.join(binariesDir, `${worker}-${hostTriple}.exe`);
copyFileSync(source, destination);
console.log(
  `[${logTag}] 已暂存 sidecar:${path.relative(repo, destination)}(源 ${path.relative(repo, source)})`
);
