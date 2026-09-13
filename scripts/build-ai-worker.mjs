#!/usr/bin/env node
// 把 ai-worker(msvc 主工具链可编译的 ort 推理 worker)release 编译并暂存进
// tauri externalBin 目录,并把 ORT 运行时四件套 DLL 复制进同目录供 bundle.resources 分发。
//
// 背景(2026-08-11 发货闭包断链修复,见 docs/todo.md F-01):
// 安装版 AI 分析/人脸/OCR 依赖独立 ai-worker 子进程,主程序只在 current_exe 同目录找
// ai-worker.exe(worker_client/process.rs::ai_worker_exe);此前 externalBin 只列 raw-worker,
// ai-worker 仅 beforeDevCommand 编译 → 安装包内缺失,运行时报「ai-worker 可执行文件不存在」。
// 本脚本补齐 release 面:编译 + 以 host triple 落名进 src-tauri/binaries/(externalBin 按打包
// 目标 triple 匹配文件名,镜像 build-raw-worker.mjs 先例)。
//
// ORT DLL:ai-worker 走 ort load-dynamic(scrollery-ai-core engine.rs::resolve_ort_dylib 强制
// env ORT_DYLIB_PATH 或 exe 旁 onnxruntime.dll,拒绝回退 System32)。DirectML/dxcompiler/dxil
// 由 onnxruntime 从自身所在目录连带加载 → 四件套必须与 ai-worker.exe 同目录分发。
// 来源 = node_modules/onnxruntime-node(devDependencies,npm ci 即就位),与 .cargo/config.toml
// 的 dev ORT_DYLIB_PATH 指向同一份。
//
// 用法:node scripts/build-ai-worker.mjs(经 package.json 的 build:ai-worker 调用,挂在
//       tauri.conf.json 的 beforeBuildCommand 上——Tauri CLI 对缺失 sidecar 文件会硬失败,
//       故 build 路径必须先跑本脚本保证 externalBin/资源文件必在)。
// 非 Windows:跳过(mac/Linux sidecar 打包归 F-06 剩余工作,不阻断跨平台 dev/build)。

import { spawnSync } from 'node:child_process';
import { mkdirSync, copyFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const binariesDir = path.join(repo, 'src-tauri', 'binaries');
const ortDllSrcDir = path.join(
  repo,
  'node_modules',
  'onnxruntime-node',
  'bin',
  'napi-v6',
  'win32',
  'x64'
);
// onnxruntime 从自身所在目录连带加载这三件(见 .cargo/config.toml 注释),随主 DLL 一同分发。
const ORT_DLLS = ['onnxruntime.dll', 'DirectML.dll', 'dxcompiler.dll', 'dxil.dll'];

// 非 Windows:mac/Linux 的 sidecar 打包属剩余工作(F-06),此处跳过(不阻断跨平台 dev/build)。
if (process.platform !== 'win32') {
  console.log(
    `[build-ai-worker] 非 Windows(${process.platform})——跳过 ai-worker sidecar 编译;` +
      'mac/Linux sidecar 打包待后续落地。'
  );
  process.exit(0);
}

// 1. release 编译(cwd = workspace 根;ai-worker 是 workspace member,host 工具链 = msvc)。
console.log('[build-ai-worker] cargo build -p ai-worker --release(cwd=workspace 根)');
const build = spawnSync('cargo', ['build', '-p', 'ai-worker', '--release'], {
  cwd: repo,
  stdio: 'inherit',
  shell: false,
});
if (build.status !== 0) {
  console.error(`[build-ai-worker] cargo build 失败(exit=${build.status ?? build.signal})`);
  process.exit(build.status ?? 1);
}

// 2. 解析 host triple(勿硬编码:本机为 x86_64-pc-windows-msvc,但一律以 rustc -vV 为准)。
const rv = spawnSync('rustc', ['-vV'], { encoding: 'utf8', shell: false });
if (rv.status !== 0) {
  console.error(`[build-ai-worker] rustc -vV 失败(exit=${rv.status})`);
  process.exit(rv.status ?? 1);
}
const m = /^host:\s*(\S+)$/m.exec(rv.stdout);
if (!m) {
  console.error('[build-ai-worker] 无法从 rustc -vV 输出解析 host triple');
  process.exit(1);
}
const hostTriple = m[1];

// 3. 复制 release 产物 → src-tauri/binaries/ai-worker-<host-triple>.exe(externalBin 按打包目标 triple 命名)。
mkdirSync(binariesDir, { recursive: true });
const srcExe = path.join(repo, 'target', 'release', 'ai-worker.exe');
if (!existsSync(srcExe)) {
  console.error(`[build-ai-worker] release 产物缺失(不应发生):${path.relative(repo, srcExe)}`);
  process.exit(1);
}
const dstExe = path.join(binariesDir, `ai-worker-${hostTriple}.exe`);
copyFileSync(srcExe, dstExe);
console.log(
  `[build-ai-worker] 已暂存 sidecar:${path.relative(repo, dstExe)}(源 ${path.relative(repo, srcExe)})`
);

// 4. 复制 ORT 四件套 DLL(任一缺失即硬失败——安装版推理没有它们必然拉不起)。
for (const dll of ORT_DLLS) {
  const src = path.join(ortDllSrcDir, dll);
  if (!existsSync(src)) {
    console.error(
      `[build-ai-worker] ORT DLL 缺失:${path.relative(repo, src)}\n` +
        `  运行 npm ci 后重试(onnxruntime-node 是 devDependencies,安装即就位)。`
    );
    process.exit(1);
  }
  const dst = path.join(binariesDir, dll);
  copyFileSync(src, dst);
  console.log(`[build-ai-worker] 已暂存 ORT DLL:${path.relative(repo, dst)}`);
}
