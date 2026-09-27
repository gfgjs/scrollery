#!/usr/bin/env node
// 将不含 FFmpeg 的 Windows 原生缩略图 worker 编译并暂存给 Tauri externalBin。
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
if (process.platform !== 'win32') {
  process.exit(0);
}

const isDev = process.argv.includes('--dev');
const profile = isDev ? 'debug' : 'release';
if (process.arch === 'x64') {
  // 不自动获取第三方源码；dispatcher 须由维护者显式准备，薄桥随本项目源码重编。
  const bridge = spawnSync(process.execPath, [path.join(repo, 'scripts/build-vpl-bridge.mjs')], {
    cwd: repo, stdio: 'inherit', shell: false, windowsHide: true,
  });
  if (bridge.status !== 0) process.exit(bridge.status ?? 1);
}
const args = [
  'build',
  '-p',
  'scrollery',
  '--bin',
  'native-thumbnail-worker',
  '--no-default-features',
  '--features',
  process.arch === 'x64' ? 'lite,native-vpl' : 'lite',
  ...(isDev ? [] : ['--release']),
];
// 开发入口先编译本 worker，raw-worker 等其它 sidecar 可能尚未暂存。
// 仅此 cargo 子进程不检查 externalBin；正式 Tauri 打包仍读取完整清单。
const cargoConfig = JSON.parse(process.env.TAURI_CONFIG ?? '{}');
cargoConfig.bundle ??= {};
cargoConfig.bundle.externalBin = [];
const build = spawnSync('cargo', args, {
  cwd: repo,
  stdio: 'inherit',
  shell: false,
  env: { ...process.env, TAURI_CONFIG: JSON.stringify(cargoConfig) },
});
if (build.status !== 0) {
  process.exit(build.status ?? 1);
}
const rustc = spawnSync('rustc', ['-vV'], { cwd: repo, encoding: 'utf8', shell: false });
if (rustc.status !== 0) {
  process.exit(rustc.status ?? 1);
}
const hostTriple = /^host:\s*(\S+)$/m.exec(rustc.stdout)?.[1];
if (!hostTriple) {
  throw new Error('无法读取 Rust host triple');
}
const source = path.join(repo, 'target', profile, 'native-thumbnail-worker.exe');
if (!existsSync(source)) {
  throw new Error(`原生缩略图 worker 产物缺失: ${source}`);
}
const binariesDir = path.join(repo, 'src-tauri', 'binaries');
mkdirSync(binariesDir, { recursive: true });
const destination = path.join(binariesDir, `native-thumbnail-worker-${hostTriple}.exe`);
copyFileSync(source, destination);
console.log(`[native-thumbnail-worker] 已暂存 ${path.relative(repo, destination)}`);
