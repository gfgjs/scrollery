#!/usr/bin/env node
// 把 raw-worker(gnu-only sidecar)编译并暂存进 tauri externalBin 目录(H2b-prod)。
//
// 背景:raw-worker(crates/exotic-workers/raw-worker)依赖 rsraw,其 build.rs 在 MSVC 下硬 panic,
// 故该 crate 脱离父 workspace(空 [workspace] 表),只能用 --target x86_64-pc-windows-gnu 编译。
// tauri externalBin 按「打包目标 triple」匹配 sidecar 文件名(binaries/<name>-<target-triple><exe>),
// 与二进制自身用哪条工具链编译无关——故这里把 gnu 编出的 raw-worker.exe 复制成 host(msvc)triple
// 命名,供 msvc 发布包按其目标 triple 取用(内容是 gnu 编的没关系)。
//
// 用法:node scripts/build-raw-worker.mjs(经 package.json 的 build:raw-worker 调用,并挂在
//       tauri.conf.json 的 beforeDevCommand/beforeBuildCommand 上——Tauri CLI 对缺失 sidecar 文件
//       会硬失败,故 dev/build 两条路径都必须先跑本脚本保证 externalBin 文件必在)。

import { spawnSync } from 'node:child_process';
import { mkdirSync, copyFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const GNU_TARGET = 'x86_64-pc-windows-gnu';
const workerCrate = path.join(repo, 'crates', 'exotic-workers', 'raw-worker');
const binariesDir = path.join(repo, 'src-tauri', 'binaries');

// 非 Windows:mac/Linux 的 sidecar 打包属剩余工作(#4),此处跳过(不阻断跨平台 dev/build)。
if (process.platform !== 'win32') {
  console.log(
    `[build-raw-worker] 非 Windows(${process.platform})——跳过 raw-worker(gnu-only sidecar)编译;` +
      'mac/Linux sidecar 打包待后续落地。'
  );
  process.exit(0);
}

// 1. gnu-only 编译(cwd = raw-worker crate;--release 出优化产物)。
console.log(`[build-raw-worker] cargo build --release --target ${GNU_TARGET}(cwd=${workerCrate})`);
const build = spawnSync('cargo', ['build', '--release', '--target', GNU_TARGET], {
  cwd: workerCrate,
  stdio: 'inherit',
  shell: false,
});
if (build.status !== 0) {
  console.error(`[build-raw-worker] cargo build 失败(exit=${build.status ?? build.signal})`);

  if (process.env.CI) {
    // CI 环境：硬失败（CI/release 绝不容忍陈旧 sidecar）
    process.exit(build.status ?? 1);
  } else {
    // 非 CI：检查是否已有旧产物（dev 可跳过重编，release 由 CI 硬验）
    const rv = spawnSync('rustc', ['-vV'], { encoding: 'utf8', shell: false });
    if (rv.status !== 0) {
      console.error(`[build-raw-worker] rustc -vV 失败(exit=${rv.status})`);
      process.exit(rv.status ?? 1);
    }
    const m = /^host:\s*(\S+)$/m.exec(rv.stdout);
    if (!m) {
      console.error('[build-raw-worker] 无法从 rustc -vV 输出解析 host triple');
      process.exit(1);
    }
    const hostTriple = m[1];
    const dst = path.join(binariesDir, `raw-worker-${hostTriple}.exe`);

    if (existsSync(dst)) {
      console.warn(
        `[build-raw-worker] gnu 编译失败但已有旧 sidecar,dev 跳过重编(release 由 CI 硬验)——` +
          path.relative(repo, dst)
      );
      process.exit(0);
    } else {
      console.error(
        `[build-raw-worker] gnu 编译失败且无旧产物。需配置:\n` +
          `  rustup target add x86_64-pc-windows-gnu\n` +
          `  mingw-w64 需在 PATH(含 dlltool/g++)`
      );
      process.exit(build.status ?? 1);
    }
  }
}

// 2. 解析 host triple(勿硬编码:本机为 x86_64-pc-windows-msvc,但一律以 rustc -vV 为准)。
const rv = spawnSync('rustc', ['-vV'], { encoding: 'utf8', shell: false });
if (rv.status !== 0) {
  console.error(`[build-raw-worker] rustc -vV 失败(exit=${rv.status})`);
  process.exit(rv.status ?? 1);
}
const m = /^host:\s*(\S+)$/m.exec(rv.stdout);
if (!m) {
  console.error('[build-raw-worker] 无法从 rustc -vV 输出解析 host triple');
  process.exit(1);
}
const hostTriple = m[1];

// 3. 复制 gnu 产物 → src-tauri/binaries/raw-worker-<host-triple>.exe(externalBin 按打包目标 triple 命名)。
const src = path.join(workerCrate, 'target', GNU_TARGET, 'release', 'raw-worker.exe');
const dst = path.join(binariesDir, `raw-worker-${hostTriple}.exe`);
mkdirSync(binariesDir, { recursive: true });
copyFileSync(src, dst);
console.log(
  `[build-raw-worker] 已暂存 sidecar:${path.relative(repo, dst)}(源 gnu 产物 ${path.relative(repo, src)})`
);
