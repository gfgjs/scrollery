#!/usr/bin/env node
// 隔离验收驱动器(P1-2 发行验收准备,2026-09-12)。
//
// why 需要它:Tauri 的 app_data_dir 由 bundle identifier 派生(PathResolver → dirs::data_dir()
// → Windows SHGetKnownFolderPath(FOLDERID_RoamingAppData) + identifier)。2026-09-12 实测:
// 覆盖 APPDATA/LOCALAPPDATA/USERPROFILE 环境变量**不改变**该解析结果,所以「干净测试库」不能靠
// 进程环境变量实现,只能靠身份覆盖配置 src-tauri/tauri.acceptance.conf.json 构建的验收包
// (identifier=com.scrollery.app.acceptance,productName=Scrollery Acceptance)。本脚本据此做三件事:
//   1) 库/缓存/日志/设置全部落在 Windows 解析出的 Roaming 目录下的 com.scrollery.app.acceptance
//      (硬守卫 + 内容核对,绝不碰用户真实库);
//   2) 媒体夹具在本脚本的临时根目录生成,不读用户照片;
//   3) 只操作自己拉起的进程 PID,不安装/卸载用户已装应用。
//
// 阶段(--stage):
//   boot   无头:PICASA_SMOKE_TEST=1 拉起验收包,断言 exit 0 且隔离 app-data 内出现启动日志(CI 用)。
//   ready  经 WebView2 CDP attach:断言 IPC 往返、前端挂载、后端 Ready 日志、零页面异常,再 exit_app 收干净。
//   chain  完整链路:首启 → 加根扫描 → 缩略图 → 查看 → 标记 → 导出 → 备份 → 重启核对。
//          --restore-drill 追加恢复演练(restore_stage → restore_arm → 重启后由 boot swap 收口)。
//          其中含目录移动取证:把 fixture 的 movable/ 子目录经真实 IPC move_directory 搬到
//          **另一个已扫描根**(目标根夹具),核对 item id / 收藏评分 / source-direct 路径可读
//         且内容 sha 一致,再搬回;移动前后 list_pending_directory_moves 必须为空清单。
// ⚠ 跨物理卷:**本机只有一块固定盘**(2026-09-12 实测 Win32_LogicalDisk 仅 C:),默认夹具两
//   个根同卷 → 走 rename 路径。跨卷「暂存→发布→删源」在本机**没有实测**,不得据此宣称已验证;
//   它的覆盖由 Rust 侧强制故障注入测试承担(ipc/dir_move/tests.rs:refused_payload_write_blocks_
//   publish_and_keeps_source / retry_without_payload_persistence_keeps_journal_and_source /
//   crash_before_publish_is_recovered_by_persisted_payload / crash_after_publish_before_delete_
//   source_converges / both_present_without_payload_stays_conflict / mismatched_payload_never_
//   claims_target)。要在真机上跑跨卷,给 --target-root=<另一块盘上的目录>。
//
// 用法:
//   node scripts/acceptance/isolated-app-smoke.mjs [--stage=boot|ready|chain] [--exe=<path>]
//        [--root=<dir>] [--port=9333] [--media=9] [--timeout=180] [--keep]
//        [--restore-drill] [--verify-user-library-untouched] [--proof=<path>]
//        [--target-root=<dir>]
//        [--install] [--install-only] [--uninstall-after]
//        [--report-tag=<tag>]  报告另存为 acceptance-report-<stage>-<tag>.json(负向对照用)
//        [--record-build] [--probe-build] [--selftest]
// 启动前强制三道门(任一不过即拒绝,且都发生在 spawn 之前——普通构建 setup 期就会写用户库):
//   ① exe 字节内烙有验收 identifier(编译期常量;普通构建烙的是 com.scrollery.app);
//   ② exe 的版本资源 ProductName 等于验收配置的 productName(独立第二条证据,经 FileVersionInfo 读);
//   ③ exe SHA256 与 --record-build 记下的构建证明一致,且验收配置自记证后未被改动。
// 前置流程:
//   npx tauri build -c src-tauri/tauri.acceptance.conf.json   # 必须带 bundle:安装验收要 NSIS/MSI 安装包
//   node scripts/acceptance/isolated-app-smoke.mjs --record-build   # 钉住产物哈希 + 配置哈希
//   node scripts/acceptance/isolated-app-smoke.mjs --stage=boot|ready|chain
//
// 安装后入口(GO 后首选):--install 会把验收包经 NSIS 静默安装(currentUser)到
// target/acceptance/install-scrollery-acceptance(自有 + ownership 校验),再从**安装目录**核对
// 嵌入 identifier / ProductName / 与构建产物 sha256 一致,随后用安装目录的主程序跑阶段。
// 安装包本身先过版本资源 ProductName 校验,普通 Scrollery 安装包会被拒。MSI 只构建+载荷解包
// 检查(scripts/verify-bundle-content.mjs),不安装。环境拒绝安装时会记为失败步骤并退回构建产物
// 继续跑可做的验收。
// 🔴 构建必须带 bundle(不要 --no-bundle):安装验收依赖 NSIS 安装包;构建后确认
// target/release/bundle/nsis/ 有 -setup.exe、bundle/msi/ 有 .msi,再跑 --record-build。
// attach 后的第四道门(ready/chain):CDP 端口先查占用,attach 后立刻用既有三个只读 IPC
// (get_log_dir / get_thumb_cache_dir / get_config_status)确认连上的实例自报路径确实在验收
// app-data 之下,确认通过才做任何写操作——连错实例时 exit_app 会关掉别人的应用。
// 退出码:全部步骤通过 = 0;任一断言失败/异常 = 1;selftest 自身损坏 = 2。

import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const ACCEPTANCE_IDENTIFIER = 'com.scrollery.app.acceptance';
const USER_IDENTIFIER = 'com.scrollery.app';
const EOL = '\n';
const STAGES = ['boot', 'ready', 'chain'];
// 用户已装应用默认落点(NSIS currentUser):验收脚本绝不触碰;落点经 Windows known folder 解析
// 得到(userInstallDir),不用环境变量拼路径。

// Windows known folder 解析:app_data_dir 的权威来源是 SHGetKnownFolderPath,不是环境变量
// (2026-09-12 实测:改写 APPDATA/LOCALAPPDATA/USERPROFILE 都不改变该 API 结果)。清理与守卫
// 一律以 API 结果为准,否则脚本可能删掉与「应用真正会写的位置」不同的目录——真正的库毫发无伤,
// 而验收却在别处空跑。
const PS_KNOWN_FOLDERS = [
  '$code = @"',
  'using System;',
  'using System.Runtime.InteropServices;',
  'public static class ScrolleryKnownFolder {',
  '  [DllImport("shell32.dll", CharSet=CharSet.Unicode)]',
  '  static extern int SHGetKnownFolderPath(ref Guid rfid, uint dwFlags, IntPtr hToken, out IntPtr ppszPath);',
  '  [DllImport("ole32.dll")] static extern void CoTaskMemFree(IntPtr p);',
  '  static readonly Guid RoamingId = new Guid("3EB685DB-65F9-4CF6-A03A-E3EF65729F3D");',
  '  static readonly Guid LocalId = new Guid("F1B32785-6FBA-4FCF-9D55-7B8E7F157091");',
  '  static string Read(Guid id) {',
  '    IntPtr p;',
  '    if (SHGetKnownFolderPath(ref id, 0, IntPtr.Zero, out p) != 0) return "";',
  '    string s = Marshal.PtrToStringUni(p);',
  '    CoTaskMemFree(p);',
  '    return s;',
  '  }',
  '  public static string Roaming() { return Read(RoamingId); }',
  '  public static string Local() { return Read(LocalId); }',
  '}',
  '"@',
  'Add-Type -TypeDefinition $code -Language CSharp',
  'Write-Output ("roaming=" + [ScrolleryKnownFolder]::Roaming())',
  'Write-Output ("local=" + [ScrolleryKnownFolder]::Local())',
].join(EOL);

let KNOWN_FOLDERS = null;

export function resolveKnownFolders() {
  if (KNOWN_FOLDERS) return KNOWN_FOLDERS;
  const r = spawnSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', PS_KNOWN_FOLDERS], {
    encoding: 'utf8',
    windowsHide: true,
    maxBuffer: 8 * 1024 * 1024,
  });
  if (r.status !== 0) throw new Error('known folder 解析失败(powershell):' + String(r.stderr || '').slice(0, 400));
  const out = { roaming: null, local: null };
  for (const line of String(r.stdout || '').split(/\r?\n/)) {
    const m = /^(roaming|local)=(.*)$/.exec(line.trim());
    if (m && m[2]) out[m[1]] = path.resolve(m[2]);
  }
  if (!out.roaming || !out.local) throw new Error('known folder 输出无法解析:' + JSON.stringify(String(r.stdout).slice(0, 300)));
  // env 与 API 不一致=shell 被改造过:此时「app 会写哪」与「脚本按 env 算哪」可能分叉,继续跑等于
  // 拿用户库冒险,直接拒绝(而不是挑一个继续)。
  for (const [envName, apiValue] of [['APPDATA', out.roaming], ['LOCALAPPDATA', out.local]]) {
    const envValue = process.env[envName];
    if (envValue && path.resolve(envValue).toLowerCase() !== apiValue.toLowerCase()) {
      throw new Error(
        envName + ' 环境变量(' + path.resolve(envValue) + ')与 Windows 实际解析(' + apiValue +
          ')不一致,拒绝在歧义环境下操作 app-data'
      );
    }
  }
  KNOWN_FOLDERS = out;
  return out;
}

function userInstallDir() {
  return path.join(resolveKnownFolders().local, 'Scrollery');
}

// ── 产物版本资源(交叉校验 exe 的产品身份)──────────────────────────────────────
// 交由 Windows/.NET 现成能力读取:[System.Diagnostics.FileVersionInfo]。本文件已有 known folder 的
// PowerShell 桥接,这里同法调用一个静态脚本 + 路径参数(输出 key=value),不再自己解析
// PE/VS_VERSIONINFO 二进制——那是易碎的重复实现,系统已有权威读法。
// 边界(2026-09-12 实测):有版本资源的产物读出 ProductName;非 PE/空文件各字段为空串(不报错),
// 由调用方判为「身份不可证」;文件缺失则脚本非零退出。
const PS_FILE_VERSION_INFO = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  'read-file-version-info.ps1'
);

// 读产物版本资源。失败抛错(读不到就必须当身份不可证处理,不能静默放行)。
export function readFileVersionInfo(exe) {
  const r = spawnSync(
    'powershell',
    ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', PS_FILE_VERSION_INFO, exe],
    { encoding: 'utf8', windowsHide: true, maxBuffer: 4 * 1024 * 1024 }
  );
  if (r.status !== 0) {
    throw new Error('读取产物版本资源失败(FileVersionInfo,exit=' + r.status + '):' + String(r.stderr || '').trim().slice(0, 300));
  }
  const info = {};
  for (const line of String(r.stdout || '').split(/\r?\n/)) {
    const i = line.indexOf('=');
    if (i <= 0) continue; // 值可为空或含 '=',按首个 '=' 切分
    info[line.slice(0, i)] = line.slice(i + 1).trim();
  }
  return info;
}

// ── 构建身份 + 构建证明 ────────────────────────────────────────────────────────
// 仅凭文件名/所在目录证明不了身份:target/release/scrollery.exe 完全可能是普通产品构建,而普通
// 构建一启动就写用户日常库(后台任务在 setup 期就落盘)。因此运行前必须同时满足:
//   ① exe 字节内烙的是验收 identifier(编译期常量,普通构建烙的是 com.scrollery.app);
//   ② 产物版本资源里的 ProductName 与验收配置一致(独立于①的第二条证据);
//   ③ exe SHA256 与 --record-build 记下的证明一致(产物没被换过、配置没被改过)。
// 三条都不满足即拒绝启动——不做「先跑起来再看 appDataDir」的检查,那时用户库已经被写过。

function relOrAbs(p) {
  const abs = path.resolve(p);
  const rel = path.relative(REPO, abs);
  return !rel.startsWith('..') && !path.isAbsolute(rel) ? rel.split(path.sep).join('/') : abs;
}

function sha256Buf(buf) {
  return createHash('sha256').update(buf).digest('hex');
}

function readAcceptanceConfig() {
  const p = path.join(REPO, 'src-tauri', 'tauri.acceptance.conf.json');
  const raw = fs.readFileSync(p, 'utf8');
  const cfg = JSON.parse(raw);
  if (cfg.identifier !== ACCEPTANCE_IDENTIFIER) {
    throw new Error('验收配置 identifier 应为 ' + ACCEPTANCE_IDENTIFIER + ',实际 ' + String(cfg.identifier));
  }
  if (!cfg.productName || !/acceptance/i.test(cfg.productName)) {
    throw new Error('验收配置 productName 需含 Acceptance,实际 ' + String(cfg.productName));
  }
  return { path: p, sha256: sha256Buf(Buffer.from(raw, 'utf8')), identifier: cfg.identifier, productName: cfg.productName };
}

export function analyzeExe(exe) {
  const buf = fs.readFileSync(exe);
  const version = readFileVersionInfo(exe);
  return {
    exe,
    exeBytes: buf.length,
    exeSha256: sha256Buf(buf),
    hasIdentifierMarker: buf.includes(Buffer.from(ACCEPTANCE_IDENTIFIER, 'utf8')),
    // 版本资源字段:读不到(非 PE/无资源)= 空串 → 归一为 null,由身份门判为不可证。
    productName: version.ProductName || null,
    fileDescription: version.FileDescription || null,
    fileVersion: version.FileVersion || null,
  };
}

// 🔴 必须走带 bundle 的 tauri build:--no-bundle 不产安装包,而安装后入口(默认验收路径)要
// 用 NSIS 安装包装到自有目录再从安装目录取证,没有安装包的构建没法完成验收。
const BUILD_HINT = 'npx tauri build -c src-tauri/tauri.acceptance.conf.json';
const RECORD_HINT = '先跑:' + BUILD_HINT + EOL + '      node scripts/acceptance/isolated-app-smoke.mjs --record-build';

export function assertAcceptanceIdentity(exe) {
  const cfg = readAcceptanceConfig();
  const info = analyzeExe(exe);
  const problems = [];
  if (!info.hasIdentifierMarker) {
    problems.push('exe 字节内未见验收 identifier ' + ACCEPTANCE_IDENTIFIER + '(普通构建烙的是 ' + USER_IDENTIFIER + ')');
  }
  if (!info.productName) problems.push('产物版本资源缺 ProductName(FileVersionInfo 读不到,身份不可证)');
  else if (info.productName !== cfg.productName) {
    problems.push('产物 ProductName "' + info.productName + '" 与验收配置 productName "' + cfg.productName + '" 不一致');
  }
  if (problems.length) {
    throw new Error('产物不是本轮验收构建,拒绝启动(普通构建启动即写用户真实库):' + EOL + '  - ' + problems.join(EOL + '  - ') + EOL + '  ' + RECORD_HINT);
  }
  return { cfg, info };
}

function proofPath() {
  return path.resolve(REPO, arg('proof', path.join('target', 'acceptance', 'acceptance-build-proof.json')));
}

export function recordBuildProof(exe, outPath) {
  const { cfg, info } = assertAcceptanceIdentity(exe);
  const proof = {
    format: 'scrollery-acceptance-build-proof/1',
    exe: relOrAbs(exe),
    exeSha256: info.exeSha256,
    exeBytes: info.exeBytes,
    identifier: cfg.identifier,
    productName: cfg.productName,
    configPath: relOrAbs(cfg.path),
    configSha256: cfg.sha256,
    pe: {
      productName: info.productName,
      fileDescription: info.fileDescription,
      fileVersion: info.fileVersion,
    },
    // 安装包与包内主程序的指纹。为什么必须一起绑定:target/release 的 exe 是占位符还原版(UNK),
    // 真正装给用户的是包内那份(带 msi/nsis 补丁),二者字节不同。只记 exe hash 回答不了
    // 「装出来的那份是不是这批产物」,故此处把安装包与包内主程序一并钉住。
    bundle: collectBundleFingerprints(info.exeBytes),
    recordedAtUtc: new Date().toISOString(),
  };
  const target = outPath || proofPath();
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, JSON.stringify(proof, null, 2) + EOL);
  return proof;
}

export function assertBuildProof(exe, atPath) {
  const p = atPath || proofPath();
  if (!fs.existsSync(p)) {
    throw new Error('缺验收构建证明:' + p + EOL + '  ' + RECORD_HINT);
  }
  const proof = JSON.parse(fs.readFileSync(p, 'utf8'));
  const cfg = readAcceptanceConfig();
  const info = analyzeExe(exe);
  const problems = [];
  if (proof.exeSha256 !== info.exeSha256) {
    problems.push(
      'exe 哈希与证明不符(证明 ' + String(proof.exeSha256).slice(0, 16) + '… 当前 ' +
        info.exeSha256.slice(0, 16) + '…):产物已被重建或替换,须重跑 --record-build'
    );
  }
  if (proof.configSha256 !== cfg.sha256) {
    problems.push('验收配置在记证之后被改动,须重新构建并 --record-build');
  }
  if (proof.identifier !== ACCEPTANCE_IDENTIFIER) problems.push('证明记录的 identifier 异常:' + String(proof.identifier));
  if (problems.length) throw new Error('验收构建证明校验失败:' + EOL + '  - ' + problems.join(EOL + '  - '));
  return { proof, info, proofPath: p };
}

// ── 受控目录守卫(删除前核对,而不是「删了再说」)────────────────────────────────
// app-data 必须同时满足:位于 Windows 解析出的 Roaming 下、直接子目录、名字等于验收 identifier,
// 且若已存在则内容得像是验收应用的数据(有 scrollery.db / logs / cache 等任一)。夹具目录要求
// 带本脚本写的标识文件——只凭 basename 相同就删,等于把任意 --root 当清理对象。
const APP_DATA_ARTIFACTS = ['scrollery.db', 'config.toml', 'logs', 'cache', 'documents', 'exotic', 'models'];
const FIXTURE_MARKER = '.acceptance-fixture.json';

export function assertDeletableAppData(dir) {
  const abs = path.resolve(dir);
  const roaming = resolveKnownFolders().roaming;
  if (path.basename(abs) !== ACCEPTANCE_IDENTIFIER) {
    throw new Error('拒绝删除:目录名不是验收 identifier:' + abs);
  }
  if (path.dirname(abs).toLowerCase() !== roaming.toLowerCase()) {
    throw new Error('拒绝删除:验收 app-data 不在 Windows 解析的 Roaming 下(' + abs + ')');
  }
  const userDir = path.resolve(path.join(roaming, USER_IDENTIFIER));
  if (abs.toLowerCase() === userDir.toLowerCase()) throw new Error('拒绝删除用户真实库:' + abs);
  if (!fs.existsSync(abs)) return abs;
  const entries = fs.readdirSync(abs);
  if (entries.length === 0) return abs;
  const looksOurs = entries.some((n) => APP_DATA_ARTIFACTS.includes(n.toLowerCase()));
  if (!looksOurs) {
    throw new Error(
      '拒绝删除:目录内容不像验收应用数据(无 ' + APP_DATA_ARTIFACTS.join('/') + ' 任一):' + abs +
        EOL + '  如确认可删请手动清理后再跑。'
    );
  }
  return abs;
}

export function assertDeletableFixtureRoot(dir, root) {
  const abs = path.resolve(dir);
  const absRoot = path.resolve(root);
  if (abs.toLowerCase() === absRoot.toLowerCase()) {
    throw new Error('拒绝删除:夹具目录不能等于验收根:' + abs);
  }
  if (!abs.toLowerCase().startsWith(absRoot.toLowerCase() + path.sep)) {
    throw new Error('拒绝删除:夹具目录不在验收根内(' + abs + ' ⊄ ' + absRoot + ')');
  }
  if (!fs.existsSync(abs)) return abs;
  if (!fs.existsSync(path.join(abs, FIXTURE_MARKER))) {
    throw new Error(
      '拒绝删除:目录没有本脚本写的标识文件 ' + FIXTURE_MARKER + ',可能属于他处:' + abs +
        EOL + '  如确认可删请手动清理后再跑。'
    );
  }
  return abs;
}

export function assertControlledRoot(root) {
  const abs = path.resolve(root);
  const roaming = resolveKnownFolders().roaming;
  const local = resolveKnownFolders().local;
  const protectedDirs = [
    path.parse(abs).root,
    os.homedir(),
    REPO,
    path.dirname(REPO),
    roaming,
    local,
    path.join(roaming, USER_IDENTIFIER),
    path.join(roaming, ACCEPTANCE_IDENTIFIER),
  ].filter(Boolean);
  for (const b of protectedDirs) {
    const nb = path.resolve(b).toLowerCase();
    if (abs.toLowerCase() === nb) throw new Error('拒绝使用受保护目录作为验收根:' + abs);
    if (nb.startsWith(abs.toLowerCase() + path.sep)) {
      throw new Error('拒绝:验收根是受保护目录的上级(' + abs + ' 包含 ' + b + ')');
    }
  }
  return abs;
}



function arg(name, fallback) {
  const hit = process.argv.slice(2).find((a) => a.startsWith('--' + name + '='));
  return hit ? hit.slice(name.length + 3) : fallback;
}
function hasFlag(name) {
  return process.argv.slice(2).includes('--' + name);
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ── 路径守卫(隔离红线)──────────────────────────────────────────────────────────

function appDataDirFor(identifier) {
  return path.join(resolveKnownFolders().roaming, identifier);
}

// 只允许在验收身份目录上做删除/建库:basename 必须等于验收 identifier,且不等于用户真实库。
function assertAcceptanceDir(dir) {
  const abs = path.resolve(dir);
  if (path.basename(abs).toLowerCase() !== ACCEPTANCE_IDENTIFIER) {
    throw new Error('拒绝操作非验收身份目录:' + abs);
  }
  const userDir = path.resolve(appDataDirFor(USER_IDENTIFIER));
  if (abs.toLowerCase() === userDir.toLowerCase()) throw new Error('拒绝操作用户真实库:' + abs);
  return abs;
}

// 只允许跑验收包:拒绝用户已装应用目录下的同名 exe(防误把用户产品当被测对象打标记/改库)。
function assertAcceptanceExe(exe) {
  const abs = path.resolve(exe);
  if (!fs.existsSync(abs)) throw new Error('验收包不存在:' + abs + '(先跑 ' + BUILD_HINT + ')');
  const installDir = path.resolve(userInstallDir());
  if (abs.toLowerCase().startsWith(installDir.toLowerCase() + path.sep)) {
    throw new Error('拒绝执行用户已装应用:' + abs);
  }
  return abs;
}

// ── PNG 夹具(确定性:同输入逐字节同产物,不依赖第三方图像库)──────────────────────

const CRC_TABLE = (() => {
  const table = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c;
  }
  return table;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function pngChunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body), 0);
  return Buffer.concat([len, body, crc]);
}

function makePng(width, height, seed) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 2; // 8bit/通道,truecolor RGB
  const raw = Buffer.alloc(height * (1 + width * 3));
  let o = 0;
  for (let y = 0; y < height; y++) {
    raw[o++] = 0; // 过滤器 None
    for (let x = 0; x < width; x++) {
      raw[o++] = (x * 3 + seed * 17) & 0xff;
      raw[o++] = (y * 3 + seed * 29) & 0xff;
      raw[o++] = (x + y + seed * 53) & 0xff;
    }
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    pngChunk('IHDR', ihdr),
    pngChunk('IDAT', zlib.deflateSync(raw, { level: 9 })),
    pngChunk('IEND', Buffer.alloc(0)),
  ]);
}

// 反向解析自检用:逐 chunk 校验 CRC + IHDR 尺寸,防夹具生成器退化成写坏文件。
function parsePng(buf) {
  const sig = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
  for (let i = 0; i < sig.length; i++) if (buf[i] !== sig[i]) throw new Error('PNG 签名不符');
  let off = 8;
  let ihdr = null;
  const types = [];
  while (off < buf.length) {
    const len = buf.readUInt32BE(off);
    const type = buf.toString('ascii', off + 4, off + 8);
    const data = buf.subarray(off + 8, off + 8 + len);
    const want = buf.readUInt32BE(off + 8 + len);
    const got = crc32(Buffer.concat([Buffer.from(type, 'ascii'), data]));
    if (want !== got) throw new Error('chunk ' + type + ' CRC 不符');
    if (type === 'IHDR') ihdr = { width: data.readUInt32BE(0), height: data.readUInt32BE(4) };
    types.push(type);
    off += 12 + len;
  }
  return { ihdr, types };
}

const FIXTURE_SIZES = [
  [640, 480],
  [1600, 1200],
  [2400, 1350],
  [900, 1600],
  [1280, 1280],
];

// 单个 PNG 条目:落盘并记下尺寸与内容 sha256(移动后要按 sha 证明「搬的是同一份内容」)。
function writePngItem(dir, name, w, h, seed, extra) {
  const file = path.join(dir, name);
  const bytes = makePng(w, h, seed);
  fs.writeFileSync(file, bytes);
  return Object.assign(
    { name, file, width: w, height: h, size: bytes.length, sha256: sha256Buf(bytes) },
    extra || {}
  );
}

// 造夹具:
//   ① 源根 mediaDir:count 张图(其中 1/3 落在 sub/ 以覆盖目录树分组)+ 1 个非媒体文件
//      (证明扫描器只收媒体)+ 一个 movable/ 子目录(目录移动验收的**源**,2 张图);
//   ② 目标根 targetDir:1 张图,作为「另一个已扫描目标目录」的宿主。
// 两个根都用本脚本自己的标识文件标记:删除前要靠它证明目录归本脚本所有。
function writeFixtures(mediaDir, targetDir, count) {
  const subDir = path.join(mediaDir, 'sub');
  fs.mkdirSync(subDir, { recursive: true });
  const items = [];
  for (let i = 0; i < count; i++) {
    const [w, h] = FIXTURE_SIZES[i % FIXTURE_SIZES.length];
    const inSub = i % 3 === 2;
    const name = 'fixture_' + String(i).padStart(2, '0') + '.png';
    items.push(writePngItem(inSub ? subDir : mediaDir, name, w, h, i + 1, { inSub }));
  }
  // 🔴 非媒体夹具必须用「确实未注册」的扩展名。曾用 .txt,结果扫描入库比预期多 1 项——
  // txt 在本产品里是**已注册的文档类媒体**(utils/format.rs 的 document("txt", Text)),
  // 会被正常入库,不是缺陷。这里改用 .xyz:项目自己的 classify_media_type("xyz") == None
  // 即以此为正典「未知格式」样例,故该文件应被扫描器丢弃。
  const noteFile = path.join(mediaDir, 'notes.xyz');
  fs.writeFileSync(noteFile, 'acceptance fixture — 未注册扩展名,不应入库\n');
  // 标识文件:删除夹具目录前必须见到它(只凭目录名相同就删,等于把任意 --root 当清理对象)。
  fs.writeFileSync(
    path.join(mediaDir, FIXTURE_MARKER),
    JSON.stringify({ createdBy: 'isolated-app-smoke.mjs', purpose: 'acceptance fixture root' }, null, 2) + EOL
  );

  // 目录移动的源:独立子目录,移动它不会动到上面那些断言用的条目。
  const movableDir = path.join(mediaDir, 'movable');
  fs.mkdirSync(movableDir, { recursive: true });
  const movableItems = [
    writePngItem(movableDir, 'movable_a.png', 1024, 768, 101, { inMovable: true }),
    writePngItem(movableDir, 'movable_b.png', 800, 600, 102, { inMovable: true }),
  ];

  // 另一个扫描根(目录移动的目标宿主)。
  fs.mkdirSync(targetDir, { recursive: true });
  fs.writeFileSync(
    path.join(targetDir, FIXTURE_MARKER),
    JSON.stringify({ createdBy: 'isolated-app-smoke.mjs', purpose: 'acceptance target root' }, null, 2) + EOL
  );
  const targetItems = [writePngItem(targetDir, 'target_a.png', 1200, 900, 201)];

  return {
    items,
    noteFile,
    movable: { dirName: 'movable', dir: movableDir, items: movableItems },
    target: { items: targetItems },
    // 源根扫描应看到的全部媒体条目(普通条目 + movable 子树)。
    allMediaItems: [...items, ...movableItems],
  };
}

// ── 进程 / CDP ────────────────────────────────────────────────────────────────

class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.nextId = 0;
    this.pending = new Map();
    this.exceptions = [];
    ws.onmessage = (ev) => {
      const m = JSON.parse(ev.data);
      if (m.id !== undefined && this.pending.has(m.id)) {
        const p = this.pending.get(m.id);
        this.pending.delete(m.id);
        if (m.error) p.reject(new Error(p.method + ': ' + m.error.message));
        else p.resolve(m.result);
      } else if (m.method === 'Runtime.exceptionThrown') {
        this.exceptions.push(m.params.exceptionDetails);
      }
    };
    // 🔴 断开时必须清空在途请求:应用退出会销毁 WebView,socket 随之关闭,那一刻在途的 IPC
    // (典型就是 exit_app 自己)永远不会回包。不清的话对应 Promise 永久 pending,调用方就挂死。
    // 这里统一 reject,让等待方立刻拿到「连接已断」而不是静默等待。
    ws.onclose = () => {
      for (const [, p] of this.pending) {
        p.reject(new Error(p.method + ': CDP 连接已关闭(应用退出/窗口销毁)'));
      }
      this.pending.clear();
      this.closed = true;
    };
  }
  send(method, params = {}) {
    return new Promise((resolve, reject) => {
      const id = ++this.nextId;
      this.pending.set(id, { resolve, reject, method });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }
}

function launchApp(exe, logDir, extraEnv, tag) {
  const outFd = fs.openSync(path.join(logDir, tag + '-stdout.log'), 'a');
  const errFd = fs.openSync(path.join(logDir, tag + '-stderr.log'), 'a');
  const child = spawn(exe, [], {
    cwd: path.dirname(exe),
    env: Object.assign({}, process.env, extraEnv),
    stdio: ['ignore', outFd, errFd],
    // 只隐藏控制台窗口:应用自己的 WebView 窗口仍照常显示(ready/chain 阶段要靠它挂 CDP)。
    windowsHide: true,
  });
  OWN_CHILDREN.push(child);
  return child;
}

async function waitExit(child, timeoutMs) {
  if (child.exitCode !== null) return child.exitCode;
  return Promise.race([
    new Promise((resolve) => child.on('exit', (code, signal) => resolve(code === null ? (signal ? -1 : -1) : code))),
    sleep(timeoutMs).then(() => Promise.reject(new Error('等待进程退出超时'))),
  ]);
}

// 只杀自己拉起的 PID(含其子进程);按进程名批量终止会波及用户实例。
function killOurProcess(child) {
  if (child.exitCode !== null) return;
  try {
    child.kill();
  } catch (_) {}
  const pid = child.pid;
  if (pid) spawnSync('taskkill', ['/PID', String(pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' });
}

async function attachCdp(port, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let lastErr = '未开始';
  for (;;) {
    try {
      const res = await fetch('http://127.0.0.1:' + port + '/json/list');
      if (res.ok) {
        const targets = await res.json();
        const page = targets.find((t) => t.type === 'page');
        if (page) {
          const ws = new WebSocket(page.webSocketDebuggerUrl);
          await new Promise((resolve, reject) => {
            ws.onopen = resolve;
            ws.onerror = () => reject(new Error('ws 连接失败'));
          });
          return { cdp: new Cdp(ws), pageUrl: page.url, close: () => ws.close() };
        }
        lastErr = '无 page 目标(现有:' + targets.map((t) => t.type).join(',') + ')';
      } else {
        lastErr = 'HTTP ' + res.status;
      }
    } catch (e) {
      lastErr = e.message;
    }
    if (Date.now() > deadline) throw new Error('CDP 目标未就绪(端口 ' + port + '):' + lastErr);
    await sleep(500);
  }
}

// ── 端口占用守卫 ───────────────────────────────────────────────────────────────
// why 要在 spawn 之前查:CDP 端点按端口 attach,端口若被**别的进程**监听(上次验收残留、或别的
// 调试会话),attachCdp 会连到别人的页面——后续 IPC 就打在了无关进程身上,连 exit_app 都可能把
// 别人的应用关掉。先确认端口可用,再谈 attach。
//
// 判定分两种 EADDRINUSE,但**都不 attach**、都只等:
//   - 有活跃监听者(connect 成功):多半是刚退出会话的 WebView2 浏览器子进程还没退完
//     (宿主 exit 0 之后它可能多活一瞬)——等它有界释放;
//   - TIME_WAIT(connect 被拒):同样等一会儿即自然释放。
// 超时仍未释放 → 明确失败,不动占用者、也不改端口。
//
// 🔴 为什么不改用「每次会话换新端口」(2026-09-12 复查后否掉的方案):WebView2 的浏览器进程按
// user-data-dir 复用——旧浏览器进程若还在,新示例的 `--remote-debugging-port` 参数**不会再生效**,
// 于是新端口上根本没人监听,attach 只会超时。换端口既没解决释放竞态,又新增了 profile 复杂度。
// 有界等待才是对症的做法:等它退完,同一个端口自然可用。
export function assertPortFree(port, graceMs) {
  const grace = graceMs === undefined ? 10000 : graceMs;
  return new Promise((resolve, reject) => {
    const started = Date.now();
    let lastWasListener = false;
    const attempt = () => {
      const probe = net.createServer();
      probe.once('error', (err) => {
        const code = err && err.code;
        if (code !== 'EADDRINUSE') {
          reject(new Error('端口 ' + port + ' 可用性检查失败(' + code + '):' + (err && err.message ? err.message : String(err))));
          return;
        }
        const c = net.connect({ port, host: '127.0.0.1' });
        const retryOrFail = (listening) => {
          c.destroy();
          lastWasListener = listening;
          if (Date.now() - started >= grace) {
            reject(
              new Error(
                '端口 ' + port + ' 在 ' + grace + 'ms 内未释放(' + (listening ? '仍有监听者' : '疑似 TIME_WAIT') + '):' + EOL +
                  '  若确认是别处的调试会话占用,请先结束它;本脚本只等待,不会 attach 忙端口,也不结束任何占用者。'
              )
            );
            return;
          }
          setTimeout(attempt, 500);
        };
        c.once('connect', () => retryOrFail(true));
        c.once('error', () => retryOrFail(false));
      });
      probe.once('listening', () => probe.close(() => resolve(port)));
      probe.listen(port, '127.0.0.1');
    };
    attempt();
    void lastWasListener;
  });
}

// ── 实例身份守卫(attach 之后、任何写操作之前)───────────────────────────────────
// why:端口查空只保证「此刻没人监听」,不保证接下来 attach 到的就是我们 spawn 的那个实例。真正
// 决定性的是**实例自报的路径**:app_data_dir 是后端启动时算出来的,只读 IPC 取回来比对最直接。
// 只用既有只读命令(get_log_dir / get_thumb_cache_dir,返回值都由 AppState 里的 app_data_dir 派生),
// 不新增产品 IPC;也刻意不做任何写操作——连错实例时 exit_app 会关掉别人的应用。
export function attachedIdentityViolations(observed, expectedAppDataDir) {
  const violations = [];
  const expected = path.resolve(expectedAppDataDir);
  const prefix = expected.toLowerCase() + path.sep;
  for (const [label, value] of Object.entries(observed)) {
    if (typeof value !== 'string' || !value) {
      violations.push(label + ' 未返回可用路径:' + JSON.stringify(value));
      continue;
    }
    const abs = path.resolve(value);
    const lower = abs.toLowerCase();
    if (lower !== expected.toLowerCase() && !lower.startsWith(prefix)) {
      violations.push(label + ' 不在预期验收 app-data 之下:' + abs);
    }
  }
  return violations;
}

export async function assertAttachedIdentity(cdp, ctx) {
  const observed = {
    logDir: await ipc(cdp, 'get_log_dir', null),
    thumbCacheDir: await ipc(cdp, 'get_thumb_cache_dir', null),
    // config.toml 就在 app_data_dir 根下(后端用同一个 app_data_dir 初始化),第三条独立证据。
    configPath: (await ipc(cdp, 'get_config_status', null)).path,
  };
  const violations = attachedIdentityViolations(observed, ctx.appDataDir);
  if (violations.length) {
    throw new Error(
      'CDP 连到的实例不是本次验收实例(已在任何写操作之前拦下):' + EOL + '  - ' +
        violations.join(EOL + '  - ') + EOL +
        '  所连端口可能被别的进程占用;只关闭本进程 socket 与本次 spawn 的 PID。'
    );
  }
  return observed;
}

function evaluate(cdp, expression, timeoutMs) {
  return cdp
    .send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
      timeout: timeoutMs === undefined ? 120000 : timeoutMs,
    })
    .then((r) => {
      if (r.exceptionDetails) {
        const ex = r.exceptionDetails.exception || {};
        throw new Error('页面异常:' + (ex.description || r.exceptionDetails.text));
      }
      return r.result ? r.result.value : undefined;
    });
}

// 直调 IPC 命令(与前端同一入口):参数名按 Rust 形参 camelCase 传。
function ipc(cdp, cmd, args, timeoutMs) {
  const expr =
    'window.__TAURI_INTERNALS__.invoke(' +
    JSON.stringify(cmd) +
    ', ' +
    JSON.stringify(args === undefined ? null : args) +
    ')';
  return evaluate(cdp, expr, timeoutMs);
}

// Channel 复刻:tauri 的 Channel 命令参数线上形态 = "__CHANNEL__:<callbackId>"
// (tauri::ipc::Channel 的 CommandArg 实现读该字符串,见 tauri/src/ipc/channel.rs)。
async function openChannel(cdp) {
  const expr =
    '(function () {' +
    ' var events = [];' +
    ' var id = window.__TAURI_INTERNALS__.transformCallback(function (raw) {' +
    // 通道协议在流尾会发一个 {end:true,index:N} 收尾标记(Channel 内部用它做顺序清理),
    // 那不是业务载荷——收进 events 会被下游当成「缺 itemId 的响应」。这里直接丢掉。
    '   if (raw && typeof raw === "object" && raw.end === true) return;' +
    '   events.push(raw && typeof raw === "object" && "message" in raw ? raw.message : raw);' +
    ' });' +
    ' window.__acceptance = { id: id, events: events };' +
    ' return "__CHANNEL__:" + id;' +
    '})()';
  return evaluate(cdp, expr);
}

async function drainChannel(cdp) {
  return evaluate(cdp, 'window.__acceptance ? window.__acceptance.events.splice(0) : []');
}

async function waitForChannel(cdp, predicate, timeoutMs, label) {
  const deadline = Date.now() + timeoutMs;
  const seen = [];
  for (;;) {
    const batch = await drainChannel(cdp);
    for (const ev of batch) {
      seen.push(ev);
      if (predicate(ev)) return { hit: ev, seen };
    }
    if (Date.now() > deadline) throw new Error('等待通道事件超时(' + label + ');已见 ' + JSON.stringify(seen.slice(-5)));
    await sleep(300);
  }
}

// ── app-data 侧观测(日志 / 缓存文件计数)────────────────────────────────────────

function readLogs(logDir) {
  if (!fs.existsSync(logDir)) return '';
  const files = fs
    .readdirSync(logDir)
    .filter((n) => n.endsWith('.log') || n.endsWith('.jsonl'))
    .map((n) => path.join(logDir, n))
    .sort();
  let text = '';
  for (const f of files.slice(-4)) text += fs.readFileSync(f, 'utf8');
  return text;
}

function countFiles(dir, ext) {
  if (!fs.existsSync(dir)) return 0;
  let n = 0;
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p);
      else if (!ext || p.toLowerCase().endsWith(ext)) n++;
    }
  };
  walk(dir);
  return n;
}

function listPayload(exeDir) {
  const want = ['raw-worker.exe', 'ai-worker.exe', 'video-worker.exe', 'onnxruntime.dll', 'DirectML.dll', 'dxcompiler.dll', 'dxil.dll'];
  const present = [];
  const missing = [];
  for (const n of want) {
    if (fs.existsSync(path.join(exeDir, n))) present.push(n);
    else missing.push(n);
  }
  return { present, missing };
}

// 用户真实库的「只读元数据指纹」:仅取顶层条目名 + mtime,证明验收全程零副作用。
// 卷别判定(目录移动走 rename 还是暂存,取决于源/目标是否同卷)。
// 注意:本机只有一块固定盘(2026-09-12 实测 Win32_LogicalDisk 仅 C:),所以默认夹具同卷——
// 跨卷「暂存→发布→删源」在这台机器上**没有被实测**,不要当成已验证;其覆盖由 Rust 侧强制
// 故障注入测试承担(见 chain 步骤里的说明)。要真跑跨卷,用 --target-root 指向另一块盘。
function sameVolume(a, b) {
  return path.parse(path.resolve(a)).root.toLowerCase() === path.parse(path.resolve(b)).root.toLowerCase();
}

// 用户真实库的「顶层元数据」指纹:只取顶层条目名 + mtimeMs。
// ⚠ 覆盖边界:这**不是**全库逐字节零改动的证明——库内文件内容变化(例如日志追加、DB 页重写)
// 不会改变顶层 mtime,本指纹也看不出来。它的用途是「验收前后我的顶层目录结构没被动过」这一条
// 弱但可机械核对的证据;报告一律按「顶层元数据未变化」措辞,不写成字节级不变。
function userLibraryTopLevelFingerprint() {
  const dir = path.join(resolveKnownFolders().roaming, USER_IDENTIFIER);
  if (!fs.existsSync(dir)) return [];
  return fs
    .readdirSync(dir, { withFileTypes: true })
    .map((e) => e.name + ':' + fs.statSync(path.join(dir, e.name)).mtimeMs)
    .sort();
}

// ── 步骤/轮询工具 ───────────────────────────────────────────────────────────────
// ── 安装后入口(NSIS currentUser 静默安装到自有目录)─────────────────────────────
// why 要装一次:target/release 的裸 exe 只证明「二进制能跑」,证明不了「用户装出来的东西能跑」
// ——sidecar 布局、legal 资源、快捷方式与注册表写入都在安装器里。所以验收要在**装出来的目录**里
// 再跑一遍。
//
// 隔离边界(不碰用户既有安装):
//   ① 安装目录固定落在验收根下并带本脚本写的标识文件,安装前后都校验 ownership;
//   ② 目录恰好是用户已装位置(%LOCALAPPDATA%\Scrollery)、在其内、或是其上级 → 拒绝;
//   ③ 安装器自身先过版本资源校验(NSIS 模板把 ProductName 写进安装器),普通 Scrollery 安装包
//      会因 ProductName 不符被拒;
//   ④ 装完从**安装目录**复核:嵌入 identifier + ProductName + 与构建产物 sha256 一致,三条都过
//      才用它跑后续阶段。
// 环境拒绝(权限/组策略/NSIS 自身失败)不算验收通过:记录成失败步骤、退回构建产物继续跑可做的
// 部分,报告里明确标出 installedEntry.ok=false。
const INSTALL_DIR_NAME = 'install-scrollery-acceptance';
const INSTALL_MARKER = '.acceptance-install.json';

function installDirFor(root) {
  return path.join(root, INSTALL_DIR_NAME);
}

// 安装目录守卫:必须在受控验收根内、不与用户已装位置重叠。
export function assertInstallableDir(dir, root) {
  const abs = path.resolve(dir);
  const absRoot = path.resolve(root);
  const userDir = path.resolve(userInstallDir());
  if (!abs.toLowerCase().startsWith(absRoot.toLowerCase() + path.sep)) {
    throw new Error('安装目录必须在验收根内(' + abs + ' ⊄ ' + absRoot + ')');
  }
  const l = abs.toLowerCase();
  const u = userDir.toLowerCase();
  if (l === u || l.startsWith(u + path.sep)) {
    throw new Error('拒绝安装进用户已装目录:' + abs);
  }
  if (u.startsWith(l + path.sep)) {
    throw new Error('拒绝:安装目录是用户已装目录的上级(' + abs + ' 包含 ' + userDir + ')');
  }
  // NSIS 的 `/D=` 参数不能加引号,含空格的路径会被截断 → 直接拒绝,不冒装歪的险。
  if (/\s/.test(abs)) throw new Error('安装目录路径不能含空白字符(NSIS /D= 参数限制):' + abs);
  return abs;
}

// 安装目录 ownership:我们建的目录才有标识文件;已有目录无标识 → 不碰。
function claimInstallDir(dir) {
  const marker = path.join(dir, INSTALL_MARKER);
  if (fs.existsSync(dir)) {
    const entries = fs.readdirSync(dir);
    if (entries.length > 0 && !fs.existsSync(marker)) {
      throw new Error('安装目录已存在且非本次验收所有(无标识文件):' + dir);
    }
  } else {
    fs.mkdirSync(dir, { recursive: true });
  }
  fs.writeFileSync(
    marker,
    JSON.stringify({ createdBy: 'isolated-app-smoke.mjs', purpose: 'acceptance install target' }, null, 2) + EOL
  );
  return dir;
}

// 找 7z(载荷比对要用它从安装包里解出主程序)。找不到就返回 null,由调用方降级并标注。
function find7z() {
  const candidates = [
    process.env.SEVENZIP,
    'C:\\Program Files\\7-Zip\\7z.exe',
    'C:\\Program Files (x86)\\7-Zip\\7z.exe',
  ].filter(Boolean);
  for (const c of candidates) if (fs.existsSync(c)) return c;
  const rv = spawnSync('where.exe', ['7z'], { encoding: 'utf8', windowsHide: true });
  if (rv.status === 0 && rv.stdout.trim()) return rv.stdout.trim().split(/\r?\n/)[0];
  return null;
}

// 解出安装包内的主程序并算 sha256——这是「安装目录里的 exe 应等于什么」的期望值。
//
// 🔴 为什么不能拿 target/release/scrollery.exe 当期望值(2026-09-12 逐字节实测):tauri-bundler
// 为每个 bundle 各打一次补丁——把 exe 内的 `__TAURI_BUNDLE_TYPE_VAR_UNK__` 占位符改写成该 bundle
// 的类型串(msi / nsis),建完包再把工作副本**还原**回 UNK。于是同一次构建产出三份不同字节:
//   target/release = bb1c39dc…(UNK) / MSI 载荷 = 52008131…(MSI) / NSIS 载荷 = 323b89f1…(NSIS)
// 而**安装到磁盘的那份 == NSIS 载荷**(实测逐字节相同)。故期望值必须取自包内。
//
// MSI 的载荷被匿名化成 `PathFile_<hash>` 之类的无扩展名条目,名字无从比对,只能按**字节尺寸**定位
// (与 scripts/verify-bundle-content.mjs 同一套办法)。
function extractPayloadExe(installer, exeName, expectedSize) {
  const sevenZip = find7z();
  if (!sevenZip) return null;
  const list = spawnSync(sevenZip, ['l', '-slt', installer], {
    encoding: 'utf8',
    windowsHide: true,
    maxBuffer: 64 * 1024 * 1024,
  });
  if (list.status !== 0) return null;
  const blocks = String(list.stdout || '').split(/\r?\n\r?\n/);
  let entry = null;
  for (const b of blocks) {
    const pm = /Path = (.+)/.exec(b);
    const sm = /Size = (\d+)/.exec(b);
    if (!pm || !sm) continue;
    const name = pm[1].trim();
    const size = Number(sm[1]);
    const base = path.basename(name).toLowerCase();
    if (base === exeName.toLowerCase()) {
      entry = name;
      break;
    }
    if (expectedSize && size === expectedSize && !entry && !/\.dll$/i.test(name)) {
      entry = name; // MSI 匿名条目:按尺寸认
    }
  }
  if (!entry) return null;
  const outDir = path.join(REPO, 'target', 'acceptance', '_payload-extract');
  fs.rmSync(outDir, { recursive: true, force: true });
  fs.mkdirSync(outDir, { recursive: true });
  const rv = spawnSync(sevenZip, ['x', '-y', '-o' + outDir, installer, entry], {
    encoding: 'utf8',
    windowsHide: true,
    maxBuffer: 64 * 1024 * 1024,
  });
  const extracted = path.join(outDir, entry);
  if (rv.status !== 0 || !fs.existsSync(extracted)) return null;
  return {
    entry,
    sha256: sha256Buf(fs.readFileSync(extracted)),
    bytes: fs.statSync(extracted).size,
    via: path.basename(sevenZip),
  };
}

// 收集 bundle 产物指纹(安装包本身 + 各自包内主程序),供构建证明绑定与安装取证比对。
function collectBundleFingerprints(expectedPayloadSize) {
  const out = { installers: [], note: null, sevenZip: Boolean(find7z()) };
  const bundleRoot = path.join(REPO, 'target', 'release', 'bundle');
  if (!fs.existsSync(bundleRoot)) {
    out.note = 'bundle 未构建(target/release/bundle 不存在)';
    return out;
  }
  const found = [];
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p);
      else if (/\.(msi|exe)$/i.test(e.name)) found.push(p);
    }
  };
  walk(bundleRoot);
  for (const installer of found.sort()) {
    const rec = {
      path: relOrAbs(installer),
      kind: installer.toLowerCase().endsWith('.msi') ? 'msi' : 'nsis',
      bytes: fs.statSync(installer).size,
      sha256: sha256Buf(fs.readFileSync(installer)),
    };
    const payload = extractPayloadExe(installer, 'scrollery.exe', expectedPayloadSize);
    if (payload) rec.payloadExe = { sha256: payload.sha256, bytes: payload.bytes, entry: payload.entry };
    out.installers.push(rec);
  }
  if (!out.sevenZip) out.note = '未找到 7z,安装包内载荷指纹未记录';
  return out;
}


function findNsisInstaller() {
  const dir = path.join(REPO, 'target', 'release', 'bundle', 'nsis');
  if (!fs.existsSync(dir)) return [];
  return fs
    .readdirSync(dir)
    .filter((n) => /-setup\.exe$/i.test(n) || /\.exe$/i.test(n))
    .map((n) => path.join(dir, n))
    .sort();
}

// 从安装目录找出主程序:排除卸载器,按嵌入 identifier 认(不靠文件名猜)。
function findInstalledExe(dir, identifier) {
  const marker = Buffer.from(identifier, 'utf8');
  const found = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isFile() || !/\.exe$/i.test(entry.name)) continue;
    if (/^uninstall/i.test(entry.name)) continue;
    const full = path.join(dir, entry.name);
    if (fs.readFileSync(full).includes(marker)) found.push(full);
  }
  if (found.length === 0) throw new Error('安装目录里找不到含验收 identifier 的主程序:' + dir);
  if (found.length > 1) throw new Error('安装目录里有多个候选主程序:' + found.join(', '));
  return found[0];
}

// 执行安装 + 从安装目录取证。返回 {exe, installDir, evidence}。失败由调用方记录并决定是否继续。
async function installAcceptanceEntry(ctx) {
  const installDir = claimInstallDir(assertInstallableDir(installDirFor(ctx.root), ctx.root));
  const installers = findNsisInstaller();
  if (installers.length === 0) {
    throw new Error('未找到 NSIS 安装包(target/release/bundle/nsis)——A' +
      ' 需先跑 ' + BUILD_HINT + '(带 bundle,才会产出 NSIS 安装包)');
  }
  const expectedProduct = readAcceptanceConfig().productName;
  // 安装器身份:NSIS 模板把 ProductName 写进安装器版本资源;不符即拒绝(防误用普通 Scrollery 安装包)。
  const accepted = installers.filter((p) => {
    try {
      return readFileVersionInfo(p).ProductName === expectedProduct;
    } catch (_) {
      return false;
    }
  });
  if (accepted.length === 0) {
    throw new Error(
      'NSIS 安装包 ProductName 均不等于 "' + expectedProduct + '",拒绝安装(普通 Scrollery 安装包会装到用户目录):' +
        EOL + '  ' + installers.map((p) => path.basename(p)).join(EOL + '  ')
    );
  }
  const installer = accepted[0];
  const built = ctx.exe;
  const builtHash = sha256Buf(fs.readFileSync(built));
  // 期望值取自**安装包内**的主程序,而不是 target/release 下的 exe。原因(2026-09-12 逐字节实测):
  // tauri-bundler 为每个 bundle 各打一次补丁——把 exe 里的 `__TAURI_BUNDLE_TYPE_VAR_UNK__` 占位符
  // 改写成该 bundle 的类型串(msi / nsis),建完包再把工作副本**还原**回 UNK。于是同一次构建里出现
  // 三份不同字节:target/release(UNK)、MSI 包内(MSI)、NSIS 包内(NSIS)。实测装配结果:
  //   target/release = bb1c39dc…(UNK) / MSI 载荷 = 52008131…(MSI) / NSIS 载荷 = 323b89f1…(NSIS)
  // 而**安装到磁盘的那份 == NSIS 载荷**(逐字节相同)。这正是要证明的不变量。
  const payloadName = path.basename(built);
  const payload = extractPayloadExe(installer, payloadName, fs.statSync(built).size);

  // NSIS 静默安装:/S + /D=<目录>(/D 必须是最后一个参数且不加引号,故上面禁了空白路径)。
  const r = spawnSync(installer, ['/S', '/D=' + installDir], { encoding: 'utf8', windowsHide: true, maxBuffer: 8 * 1024 * 1024 });
  const out = ((r.stdout || '') + (r.stderr || '')).trim();
  if (r.status !== 0) {
    throw new Error('NSIS 静默安装失败(exit=' + r.status + ')' + (out ? EOL + out.slice(-800) : ''));
  }

  // 从安装目录取证:嵌入 identifier、ProductName、与构建产物逐字节一致。
  const installedExe = findInstalledExe(installDir, ACCEPTANCE_IDENTIFIER);
  const installedHash = sha256Buf(fs.readFileSync(installedExe));
  const vi = readFileVersionInfo(installedExe);
  const problems = [];
  if (vi.ProductName !== expectedProduct) {
    problems.push('安装目录产物 ProductName "' + vi.ProductName + '" ≠ "' + expectedProduct + '"');
  }
  // 载荷级不变量:装到磁盘的 exe 必须等于**本轮安装包内**那份。
  if (!payload) {
    problems.push('无法从安装包取出主程序做载荷比对(缺 7z 或解包失败):' + installer);
  } else if (installedHash !== payload.sha256) {
    problems.push(
      '安装目录产物 ≠ 安装包内该二进制(装的不是本轮包):安装 ' + installedHash.slice(0, 16) +
        '… vs 包内 ' + payload.sha256.slice(0, 16) + '…'
    );
  }
  if (problems.length) throw new Error('安装目录取证失败:' + EOL + '  - ' + problems.join(EOL + '  - '));

  return {
    exe: installedExe,
    installDir,
    installer,
    evidence: {
      installer: path.basename(installer),
      installDir,
      installedExe: path.basename(installedExe),
      sha256: installedHash,
      productName: vi.ProductName,
      fileVersion: vi.FileVersion,
      // 载荷级证明:安装产物 == 安装包内该二进制(见函数内注释:与 target/release 不同是 bundler
      // 的 bundle-type 补丁行为,不是缺陷,故不再拿它当期望值)。
      payloadHashVerified: Boolean(payload),
      payloadExeSha256: payload ? payload.sha256 : null,
      payloadExeBytes: payload ? payload.bytes : null,
      buildProductExeSha256: builtHash,
      buildProductDiffersByBundleTypePatch: Boolean(payload) && builtHash !== installedHash,
    },
  };
}

// 卸载安装器产物(仅显式 --uninstall-after 时;只卸载我们自有目录里的那一份)。
function uninstallAcceptanceEntry(installDir) {
  const uninstaller = path.join(installDir, 'uninstall.exe');
  if (!fs.existsSync(uninstaller)) throw new Error('安装目录里没有卸载器:' + uninstaller);
  const r = spawnSync(uninstaller, ['/S'], { encoding: 'utf8', windowsHide: true, maxBuffer: 8 * 1024 * 1024 });
  const out = ((r.stdout || '') + (r.stderr || '')).trim();
  if (r.status !== 0) throw new Error('静默卸载失败(exit=' + r.status + ')' + (out ? EOL + out.slice(-400) : ''));
  const leftover = fs.existsSync(path.join(installDir, INSTALL_MARKER));
  return { uninstaller, markerLeftover: leftover };
}


// ── Worker 真实运行验证(exotic 帧协议握手)──────────────────────────────────────
// why 要有这一步:此前只做「文件在不在」的存在性/哈希校验,那只证明**分发**到位,不证明 worker
// **跑得起来**。这里按生产同一套机制实跑:spawn worker → 写 Hello 帧 → 等 Ready 帧 → 校验
// worker_id 与协议版本 → 发 Shutdown(在 finish 里)收干净退出。
//
// 覆盖边界(如实):握手路径**不承载模型加载**(ai-worker 模块头原文:「进程握手快而恒定
// (Hello→Ready,5s 档):**不承载模型加载**」),故本轮**不覆盖**模型依赖路径——AI 推理 / OCR /
// 增强等需要模型文件的分支未跑,也不在本轮供给范围内;raw/video 的能力声明亦只到 probe 通过范围。
const EXOT_MAGIC = Buffer.from('EXOT', 'ascii');
const EXOT_VERSION = 3;
const FRAME_HELLO = 1;
const FRAME_READY = 2;
const FRAME_SHUTDOWN = 6;

// 组一帧:24 字节定长头(magic/version/type/requestId(u64)/jsonLen/blobLen)+ JSON 段 + blob 段。
function exotFrame(type, requestId, body) {
  const json = body === undefined ? Buffer.alloc(0) : Buffer.from(JSON.stringify(body), 'utf8');
  const header = Buffer.alloc(24);
  EXOT_MAGIC.copy(header, 0);
  header.writeUInt16LE(EXOT_VERSION, 4);
  header.writeUInt16LE(type, 6);
  header.writeBigUInt64LE(BigInt(requestId), 8);
  header.writeUInt32LE(json.length, 16);
  header.writeUInt32LE(0, 20);
  return Buffer.concat([header, json]);
}

// 从累积字节取一帧;不足一帧返回 null(继续读)。
function exotTakeFrame(buf) {
  if (buf.length < 24) return null;
  if (!buf.subarray(0, 4).equals(EXOT_MAGIC)) {
    throw new Error('帧头 magic 不是 EXOT(收到 ' + JSON.stringify(buf.subarray(0, 4).toString('latin1')) + ')');
  }
  const version = buf.readUInt16LE(4);
  const type = buf.readUInt16LE(6);
  const requestId = Number(buf.readBigUInt64LE(8));
  const jsonLen = buf.readUInt32LE(16);
  const blobLen = buf.readUInt32LE(20);
  const total = 24 + jsonLen + blobLen;
  if (buf.length < total) return null;
  const jsonRaw = buf.subarray(24, 24 + jsonLen);
  return {
    frame: { version, type, requestId, json: jsonRaw.length ? JSON.parse(jsonRaw.toString('utf8')) : null, blobLen },
    rest: buf.subarray(total),
  };
}

// 实跑一个 worker 并完成握手;失败抛错(附 stderr 尾部便于定位)。
function handshakeWorker(exe, expectedWorkerId, timeoutMs) {
  return new Promise((resolve, reject) => {
    const child = spawn(exe, [], { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
    let acc = Buffer.alloc(0);
    let stderr = '';
    let settled = false;
    const finish = (err, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try {
        child.stdin.end(exotFrame(FRAME_SHUTDOWN, 0, undefined));
      } catch (_) {}
      killOurProcess(child);
      if (err) reject(new Error(err + (stderr ? EOL + '  stderr: ' + stderr.trim().slice(-400) : '')));
      else resolve(value);
    };
    const timer = setTimeout(() => finish('worker 握手超时(' + timeoutMs + 'ms):' + path.basename(exe)), timeoutMs);
    child.on('error', (e) => finish('worker 启动失败:' + e.message));
    child.on('exit', (code) => {
      if (!settled) finish('worker 提前退出(exit=' + code + '):' + path.basename(exe));
    });
    child.stderr.on('data', (d) => { stderr += d.toString('utf8'); });
    child.stdout.on('data', (d) => {
      acc = Buffer.concat([acc, d]);
      try {
        let taken;
        while ((taken = exotTakeFrame(acc)) !== null) {
          acc = taken.rest;
          const f = taken.frame;
          if (f.type !== FRAME_READY) continue; // 非 Ready 帧(如 Progress)不参与握手判定
          const ready = f.json || {};
          if (ready.worker_id !== expectedWorkerId) {
            finish('worker_id 不符:' + JSON.stringify(ready.worker_id) + ' ≠ ' + expectedWorkerId);
            return;
          }
          if (Number(ready.protocol_version) !== EXOT_VERSION) {
            finish('协议版本不符:' + ready.protocol_version + ' ≠ ' + EXOT_VERSION);
            return;
          }
          finish(null, {
            workerId: ready.worker_id,
            workerVersion: ready.worker_version,
            protocolVersion: ready.protocol_version,
            capabilities: ready.capabilities,
            frameVersion: f.version,
          });
          return;
        }
      } catch (e) {
        finish('解析 worker 输出失败:' + e.message);
      }
    });
    child.stdin.write(
      exotFrame(FRAME_HELLO, 0, {
        host_version: 'acceptance',
        protocol_version: EXOT_VERSION,
        max_blob_len: 64 << 20,
      })
    );
  });
}

// 打包内应存在的 worker 与其期望 worker_id(与各 worker 源码的 WORKER_ID 常量一致)。
const EXPECTED_WORKERS = [
  { file: 'raw-worker.exe', id: 'raw-worker' },
  { file: 'ai-worker.exe', id: 'ai-worker' },
  { file: 'video-worker.exe', id: 'video-worker' },
];

// 统一目录树遍历(服务 list-items / move-inputs / target-root 三处,避免各写一份假设)。
// Windows 路径归一化,只用于**比较**:后端对用户路径做 canonicalize 后会带 verbatim 前缀
// (`\\?\C:\...`,UNC 则是 `\\?\UNC\server\share`),直接拿它和普通盘符路径做 startsWith 必然失配。
// 去掉前缀后再 resolve,两侧口径才一致。不改磁盘上的真实路径,只影响比较。
function toComparablePath(p) {
  let s = String(p || '');
  if (s.startsWith('\\\\?\\UNC\\')) s = '\\\\' + s.slice(8);
  else if (s.startsWith('\\\\?\\')) s = s.slice(4);
  return path.resolve(s).toLowerCase();
}

// 目标是否位于某受控根之下(用归一化口径比较,见上)。
function isUnder(candidate, root) {
  const c = toComparablePath(candidate);
  const r = toComparablePath(root);
  return c === r || c.startsWith(r + path.sep);
}

//
// 实测契约(db/queries/scan/directories.rs + composables/useFolderTree.ts):
//   - `get_directory_tree(rootId)` **只返回根层**(query_directory_level 的 seed 子句是
//     `parent_id IS NULL`),不是整棵树;
//   - `DirNode` 没有 children 字段,只有 `hasChildren: boolean`(children 由前端另调
//     `get_directory_children(parentId)` 懒加载——useFolderTree.ts 就是这么做的)。
// 为免日后 DTO 真加了 children 就静默少走一层,这里两种形状都吃:有 children 就地下探,
// 否则按 hasChildren 逐层拉取。返回扁平节点数组(含根)。
async function walkDirectoryTree(cdp, rootId, timeoutMs) {
  const roots = await ipc(cdp, 'get_directory_tree', { rootId, categories: null });
  if (!Array.isArray(roots)) throw new Error('get_directory_tree 返回异常:' + JSON.stringify(roots).slice(0, 160));
  const flat = [];
  const visit = async (node) => {
    flat.push(node);
    if (Array.isArray(node.children)) {
      for (const c of node.children) await visit(c);
      return;
    }
    if (node.hasChildren === false) return;
    const children = await ipc(cdp, 'get_directory_children', { parentId: node.id, categories: null }, timeoutMs);
    for (const c of children || []) await visit(c);
  };
  for (const r of roots) await visit(r);
  return flat;
}

const results = [];
// 请求应用干净退出,并以「自有进程真的退出」为判据。
//
// 🔴 为什么不 await exit_app 的响应(2026-09-12 实测):exit_app 会销毁 WebView,CDP socket 随即断开,
// 那个调用本身永远不会回包——先 await 它会让流程卡死(或至少白等一整个 deadline)。正确顺序:
//   ① 先注册退出监听(waitExit),② 再 fire-and-forget 发退出请求并吞掉失败,
//   ③ 以「进程退出事件 / deadline」判定成功。产品退出已发生就该判成功,不因 IPC 无回包而误判。
async function requestCleanExit(cdp, child, timeoutMs) {
  const deadline = timeoutMs || 30000;
  const codePromise = waitExit(child, deadline); // 必须先注册,再发请求
  ipc(cdp, 'exit_app', null).catch(() => {}); // 不 await:见上
  return codePromise;
}

// 本进程自己拉起的子进程(失败路径统一回收时用)。launchApp 里登记。
const OWN_CHILDREN = [];
// 事件循环保活句柄。理由:AsyncLocal 的 pending promise 不持有 handle——当唯一 handle(WebSocket 等)
// 关闭时 Node 会当作无事可做而静默退出,退出码 0。2026-09-12 实测踩过:chain 跑到 chain:backup 就
// 「成功」结束了,而报告/日志停在那一瞬。此句柄让流程只能显式收尾,不能悄悄消失。
let KEEPALIVE = null;

async function runStep(name, fn) {
  const t0 = Date.now();
  try {
    const evidence = await fn();
    const ms = Date.now() - t0;
    results.push({ name, ok: true, ms, evidence: evidence === undefined ? null : evidence });
    console.log('  [ok] ' + name + ' (' + ms + 'ms)' + (evidence === undefined ? '' : ' ' + JSON.stringify(evidence)));
    return evidence;
  } catch (err) {
    const message = err && err.message ? err.message : String(err);
    results.push({ name, ok: false, ms: Date.now() - t0, error: message });
    console.error('  [FAIL] ' + name + ': ' + message);
    throw err;
  }
}

async function pollIpc(cdp, cmd, args, predicate, timeoutMs, label) {
  const deadline = Date.now() + timeoutMs;
  let last;
  for (;;) {
    last = await ipc(cdp, cmd, args);
    if (predicate(last)) return last;
    if (Date.now() > deadline) throw new Error('轮询超时(' + label + '),最后值:' + JSON.stringify(last).slice(0, 300));
    await sleep(500);
  }
}

async function waitChannelCount(cdp, count, timeoutMs, label) {
  const deadline = Date.now() + timeoutMs;
  const seen = [];
  while (seen.length < count) {
    for (const ev of await drainChannel(cdp)) seen.push(ev);
    if (seen.length >= count) break;
    if (Date.now() > deadline) throw new Error('通道事件不足(' + label + '):' + seen.length + '/' + count);
    await sleep(200);
  }
  return seen;
}

// 拉起带 CDP 端口的实例。先查端口空闲:端口被占时 attach 会连到别人身上,那种错误一旦发生就不是
// 「验收失败」而是「验收打到无关进程」,不能留这个口子。
async function launchWithCdp(ctx, tag) {
  // 单端口(launch 与 attach 共用),先等它释放——不换端口、不 attach 忙端口(理由见 assertPortFree)。
  await assertPortFree(ctx.port);
  const child = launchApp(ctx.exe, ctx.logDir, { WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=' + ctx.port }, tag);
  return { child, port: ctx.port };
}

function countThumbs(dir) {
  return countFiles(dir, '.webp');
}

// ── 阶段:boot(无头隔离冒烟,CI 用)─────────────────────────────────────────────

async function stageBoot(ctx) {
  const child = launchApp(ctx.exe, ctx.logDir, { PICASA_SMOKE_TEST: '1' }, 'boot');
  let code = -1;
  try {
    code = await waitExit(child, ctx.timeoutMs);
  } finally {
    killOurProcess(child);
  }
  await runStep('boot:exit-zero', () => {
    // 101 = panic;1 = fatal_startup_error;0 = RunEvent::Ready 就绪即退。
    if (code !== 0) throw new Error('退出码 ' + code + '(期望 0;101=panic,1=启动致命错误)');
    return { exitCode: code };
  });
  await runStep('boot:isolated-app-data', () => {
    if (!fs.existsSync(ctx.appDataDir)) {
      throw new Error('隔离 app-data 未创建:' + ctx.appDataDir + '(说明构建未走验收身份覆盖配置)');
    }
    const db = path.join(ctx.appDataDir, 'scrollery.db');
    if (!fs.existsSync(db)) throw new Error('隔离库缺 scrollery.db:' + db);
    const logs = readLogs(path.join(ctx.appDataDir, 'logs'));
    if (logs.includes('[FATAL]')) throw new Error('启动日志含致命错误');
    return { appDataDir: ctx.appDataDir, dbBytes: fs.statSync(db).size, logChars: logs.length };
  });
}

// ── 阶段:ready(CDP attach + IPC 往返 + Ready 日志 + 干净退出)──────────────────

async function stageReady(ctx) {
  const { child, port } = await launchWithCdp(ctx, 'ready');
  // session 在 try 之外先置空:attach 失败也必须走 finally 回收**自己 spawn 的**进程
  // (attach 抛错时若只有 try 内的 finally,进程会留在后台继续跑)。
  let session = null;
  try {
    session = await attachCdp(port, ctx.timeoutMs);
    const cdp = session.cdp;
    await cdp.send('Runtime.enable');
    // 任何写操作之前先确认连上的是本次验收实例(见守卫注释)。
    await runStep('ready:attached-identity', () => assertAttachedIdentity(cdp, ctx));
    await runStep('ready:ipc-roundtrip', async () => {
      const cfg = await ipc(cdp, 'get_startup_config', null);
      if (!cfg || typeof cfg !== 'object') throw new Error('get_startup_config 返回异常:' + JSON.stringify(cfg));
      return { keys: Object.keys(cfg).slice(0, 8), pageUrl: session.pageUrl };
    });
    await runStep('ready:frontend-mounted', async () => {
      const raw = await evaluate(
        cdp,
        'JSON.stringify({ state: document.readyState, title: document.title,' +
          ' appChildren: document.getElementById("app") ? document.getElementById("app").childElementCount : -1 })'
      );
      const probe = JSON.parse(raw);
      if (probe.state !== 'complete') throw new Error('document.readyState=' + probe.state);
      if (!(probe.appChildren > 0)) throw new Error('#app 无子节点,前端未挂载');
      return probe;
    });
    await runStep('ready:no-page-exception', () => {
      if (cdp.exceptions.length) {
        throw new Error('页面异常 ' + cdp.exceptions.length + ' 条:' + JSON.stringify(cdp.exceptions[0]).slice(0, 240));
      }
      return { exceptions: 0 };
    });
    await runStep('ready:clean-exit', async () => {
      const code = await requestCleanExit(cdp, child);
      if (code !== 0) throw new Error('exit_app 后退出码 ' + code + '(期望 0:退出路径含 WAL checkpoint)');
      return { exitCode: code };
    });
    // 🔴 日志断言必须放在**退出之后**读(2026-09-12 实测踩过):进程存活期间日志走缓冲,
    // info 级还在内存里没落盘——此时只读得到 WARN 一类已 flush 的行,于是把「后端已就绪」
    // 误判成「日志未见 Ready」。退出会 flush,退完再读才是完整日志。
    // 另外:就绪的**行为证据**在退出前已经取到(IPC 往返 / 前端挂载 / 零页面异常),
    // 这里读日志是补充证据,不是唯一判据。
    await runStep('ready:backend-ready-log', () => {
      const logs = readLogs(path.join(ctx.appDataDir, 'logs'));
      if (logs.includes('[FATAL]')) throw new Error('启动日志含致命错误');
      const m = logs.match(/Rust boot[^\n]*Ready[^\n]*/);
      // 🔴 为什么**不**拿「日志里必须有 Ready 行」判失败(2026-09-12 单变量实测查明):
      // logging.rs:546 用 `EnvFilter::try_from_default_env()`,**优先读 `RUST_LOG` 环境变量**,
      // 只有它缺省时才回落到 config.toml 的 log_level(默认 info)。本机父进程环境里 `RUST_LOG=warn`,
      // 于是 info 级的 Ready 行被过滤掉——与产品退出实现、与缓冲丢失都无关。
      // 实测(只改 RUST_LOG 一个变量,其余环境照旧):RUST_LOG=info 的子进程立刻多出 13 行、
      // 其中包含 `"level":"INFO" ... "Rust boot → Ready 230ms"`;warn 档则只有那条 WARN。
      // 就绪的正式判据是**行为证据**(IPC 往返 / 前端挂载 / 零页面异常),本步之前已全部通过;
      // 本步只保留:① FATAL 硬失败;② 有 Ready 行就记为补充证据,没有则如实标注环境过滤原因。
      return {
        sawReadyLine: Boolean(m),
        logChars: logs.length,
        rustLogEnv: process.env.RUST_LOG || null,
        note: m
          ? '日志含 Ready 行'
          : '日志未含 Ready 行:EnvFilter 优先取 RUST_LOG=' + (process.env.RUST_LOG || '(未设)') +
            ',info 级被过滤(logging.rs:546);就绪由前面的行为证据判定。要复现该行:以 RUST_LOG=info 启动子进程',
      };
    });
  } finally {
    if (session) session.close();
    killOurProcess(child);
  }
}

// ── 阶段:chain(首启→扫描→缩略图→查看→标记→导出→备份→重启恢复)────────────────

async function stageChain(ctx) {
  const children = [];
  const sessions = [];
  const launch = async (tag) => {
    const { child, port } = await launchWithCdp(ctx, tag);
    children.push(child);
    const session = await attachCdp(port, ctx.timeoutMs);
    sessions.push(session);
    await session.cdp.send('Runtime.enable');
    // 任何写操作之前先确认实例身份:连错进程时后续 IPC(尤其 exit_app)会打在别人身上。
    await assertAttachedIdentity(session.cdp, ctx);
    return { child, cdp: session.cdp, session };
  };
  const exitClean = async (cdp, child, label) => {
    // 顺序要紧:先注册退出监听,再 fire 退出请求(见 requestCleanExit 注释:
    // exit_app 的 IPC 响应会随 WebView 销毁而永不回包,不能先 await 它)。
    const code = await requestCleanExit(cdp, child, 30000);
    if (code !== 0) throw new Error(label + ' 退出码 ' + code + '(期望 0)');
    return { exitCode: code };
  };
  try {
    const first = await launch('chain');
    const cdp = first.cdp;
    const expected = ctx.fixtures.items.length;
    // 源根扫描看到的条目 = 根目录文件 + movable/ 子树(移动验收的源);根目录列表仍是 expected。
    const expectedAll = ctx.fixtures.allMediaItems.length;

    await runStep('chain:first-launch-empty', async () => {
      const roots = await ipc(cdp, 'list_scan_roots', null);
      if (!Array.isArray(roots) || roots.length !== 0) throw new Error('首启扫描根应 0 个,实际 ' + JSON.stringify(roots));
      return { roots: 0 };
    });

    const root = await runStep('chain:add-root', async () => {
      const r = await ipc(cdp, 'add_scan_root', { path: ctx.mediaDir, alias: 'Acceptance Fixture' });
      if (!r || typeof r.id !== 'number') throw new Error('add_scan_root 返回异常:' + JSON.stringify(r));
      return { id: r.id, path: r.path };
    });

    await runStep('chain:full-scan', async () => {
      const channel = await openChannel(cdp);
      const started = ipc(
        cdp,
        'start_scan',
        {
          rootId: root.id,
          runId: 'acceptance-' + Date.now(),
          onProgress: channel,
          groupBy: null,
          sortWithinGroup: null,
          sortOrder: null,
          quick: false,
        },
        ctx.timeoutMs
      );
      const awaited = waitForChannel(
        cdp,
        (ev) => ev && (ev.type === 'completed' || ev.type === 'error'),
        ctx.timeoutMs,
        'scan'
      );
      await started;
      const hit = await awaited;
      if (hit.hit.type === 'error') throw new Error('扫描报错:' + JSON.stringify(hit.hit));
      if (Number(hit.hit.totalItems) !== expectedAll) {
        throw new Error('扫描入库 ' + hit.hit.totalItems + ' 项,期望 ' + expectedAll + '(夹具含 1 个非媒体文件,不应入库)');
      }
      return { totalItems: hit.hit.totalItems, elapsedMs: hit.hit.elapsedMs, events: hit.seen.length };
    });

    const items = await runStep('chain:list-items', async () => {
      const dirs = await walkDirectoryTree(cdp, root.id, ctx.timeoutMs);
      const rootDir = dirs[0];
      if (!rootDir) throw new Error('目录树为空:' + JSON.stringify(dirs).slice(0, 200));
      // 🔴 两条既有 API 契约都别想当然(见 db/queries/scan/directories.rs):
      //   ① `get_directory_tree` 只返回**根层**(query_directory_level 用 `parent_id IS NULL`),
      //      子目录要走 `get_directory_children(parent_id)` 逐层下探——它不是整棵树;
      //   ② `list_directory_files` 只列**该目录直属**的文件,不递归——夹具把 1/3 的图放在 sub/,
      //      所以根目录只该列出 6 项,基础 9 图必须逐目录取数再合并。
      // 这里按这两个契约逐层遍历(不改产品语义);movable/ 子树另算(目录移动用)。
      const baseNames = new Set(ctx.fixtures.items.map((x) => x.name));
      const all = [];
      for (const d of dirs) {
        const files = await ipc(cdp, 'list_directory_files', {
          directoryId: d.id,
          limit: 500,
          offset: 0,
          categories: null,
        });
        if (!Array.isArray(files)) throw new Error('list_directory_files 返回异常:' + JSON.stringify(files).slice(0, 160));
        for (const f of files) {
          if (typeof f.id !== 'number') throw new Error('文件行缺 id:' + JSON.stringify(f).slice(0, 160));
          all.push(f);
        }
      }
      const base = all.filter((f) => baseNames.has(f.fileName));
      if (base.length !== expected) {
        throw new Error(
            '基础夹具条目 ' + base.length + ' 项,期望 ' + expected +
            '(跨全部目录共列出 ' + all.length + ' 项;目录:' + JSON.stringify(dirs.map((n) => n.name)) + ')'
        );
      }
      // 根目录直属项数也要与夹具布局一致(6 = 9 张里 1/3 落 sub/),否则说明扫描的目录归属错了。
      const inSub = ctx.fixtures.items.filter((x) => x.inSub).length;
      const rootDirect = all.filter((f) => baseNames.has(f.fileName) && !ctx.fixtures.items.find((x) => x.name === f.fileName).inSub);
      if (rootDirect.length !== expected - inSub) {
        throw new Error('根目录直属 ' + rootDirect.length + ' 项,期望 ' + (expected - inSub) + '(其余 ' + inSub + ' 项在 sub/)');
      }
      return {
        dirId: rootDir.id,
        ids: base.map((f) => f.id),
        names: base.map((f) => f.fileName),
        layout: { rootDirect: rootDirect.length, inSub },
      };
    });

    const cache = await runStep('chain:cache-dir', async () => {
      const dir = await ipc(cdp, 'get_thumb_cache_dir', null);
      if (typeof dir !== 'string' || !dir) throw new Error('get_thumb_cache_dir 返回异常:' + JSON.stringify(dir));
      if (!isUnder(dir, ctx.appDataDir)) {
        throw new Error('缩略图缓存不在隔离 app-data 内:' + dir);
      }
      return { dir, webpBefore: countThumbs(dir) };
    });

    await runStep('chain:thumbnails', async () => {
      const channel = await openChannel(cdp);
      const done = ipc(
        cdp,
        'batch_request_thumbnails',
        { itemIds: items.ids, targetSize: null, onResult: channel },
        ctx.timeoutMs
      );
      const seen = await waitChannelCount(cdp, items.ids.length, ctx.timeoutMs, 'thumbnails');
      await done;
      // 逐项核对,而不是只数事件条数(凑够 N 条不代表覆盖了请求的 N 个 item):
      //   ① 响应 itemId 必须在请求集内,且请求集被完整覆盖;
      //   ② thumbPath 必须真实存在,且落在受控的隔离 app-data 内(缓存根就在其下)。
      const wantIds = new Set(items.ids);
      const gotIds = new Set();
      for (const r of seen) {
        if (!r || typeof r.itemId !== 'number') {
          throw new Error('缩略图响应缺 itemId:' + JSON.stringify(r).slice(0, 200));
        }
        if (!wantIds.has(r.itemId)) throw new Error('缩略图响应 itemId 不在请求集内:' + r.itemId);
        if (typeof r.thumbPath !== 'string' || !r.thumbPath) {
          throw new Error('缩略图未产出路径(item ' + r.itemId + '):' + JSON.stringify(r).slice(0, 200));
        }
        // thumbPath 是**相对缓存目录**的路径(status 1 由前端拼 `${cacheDir}/thumbnails/${thumbPath}`,
        // 见 composables/useThumbLoader.ts buildThumbUrl);不是绝对路径。status 3 才是原文件绝对路径。
        const absThumb = path.isAbsolute(r.thumbPath)
          ? path.resolve(r.thumbPath)
          : path.resolve(cache.dir, 'thumbnails', r.thumbPath);
        if (!fs.existsSync(absThumb)) throw new Error('缩略图路径在磁盘上不存在:' + absThumb + '(响应 ' + r.thumbPath + ')');
        if (!isUnder(absThumb, ctx.appDataDir)) {
          throw new Error('缩略图路径不在隔离 app-data 内:' + absThumb);
        }
        gotIds.add(r.itemId);
      }
      if (gotIds.size !== wantIds.size) {
        throw new Error('缩略图覆盖 ' + gotIds.size + '/' + wantIds.size + ' 项(响应条数 ' + seen.length + ')');
      }
      // 🔴 不要求 before→after 严格增加:扫描后的后台派生很可能在 cache-dir 取快照之前就已把 webp 全部
      // 产出,此时批量请求直接命中缓存,after==before 是**正常闭环**,不是失败。
      // 生成证据由「缓存内至少有 1 个衍生 webp」承担(首启 app-data 为空,这些只可能是本次扫描产出的)。
      const webpAfter = countThumbs(cache.dir);
      if (webpAfter < 1) throw new Error('缓存内没有任何 webp,衍生未发生:' + cache.dir);
      return {
        results: seen.length,
        covered: gotIds.size,
        webpBefore: cache.webpBefore,
        webpAfter,
        // 相等是允许的(见上);此处记录事实供报告解读,不作判据。
        increasedDuringBatch: webpAfter > cache.webpBefore,
      };
    });

    const detail = await runStep('chain:view-detail', async () => {
      const d = await ipc(cdp, 'get_media_detail', { id: items.ids[0] });
      if (!d) throw new Error('get_media_detail 返回空');
      const f = ctx.fixtures.items.find((x) => x.name === d.fileName);
      if (!f) throw new Error('详情文件名不在夹具内:' + d.fileName);
      if (Number(d.width) !== f.width || Number(d.height) !== f.height) {
        throw new Error('详情尺寸 ' + d.width + 'x' + d.height + ' 与夹具 ' + f.width + 'x' + f.height + ' 不符');
      }
      if (d.availability !== 'online') throw new Error('availability=' + d.availability + '(期望 online)');
      // 🔴 元数据不算「查看」:还要在**真实 WebView** 里把这张图经 asset 协议真正解码出来。
      // 这里走的就是生产入口 —— 页内 `__TAURI_INTERNALS__.convertFileSrc`(前端 resolveAssetUrl
      // 的实现底座),不是 Node 侧 fs.readFile:后者绕过了媒体协议与 CSP,证明不了「用户能看见」。
      const absForLoad = String(d.absPath || '');
      if (!absForLoad) throw new Error('详情缺 absPath,无法做真实加载');
      const loaded = JSON.parse(
        await evaluate(
          cdp,
          'new Promise(function (resolve) {' +
            ' const url = window.__TAURI_INTERNALS__.convertFileSrc(' + JSON.stringify(absForLoad) + ');' +
            ' const img = new Image();' +
            ' const done = function (ok, extra) {' +
            '   resolve(JSON.stringify(Object.assign({ ok: ok, url: url, src: img.src },' +
            '     { naturalWidth: img.naturalWidth, naturalHeight: img.naturalHeight, complete: img.complete }, extra || {})));' +
            ' };' +
            ' img.onload = function () { done(true); };' +
            ' img.onerror = function () { done(false, { error: "onerror" }); };' +
            ' img.src = url;' +
            ' setTimeout(function () { if (!img.complete) done(false, { error: "timeout" }); }, 30000);' +
          ' })'
        )
      );
      if (!loaded.ok) {
        throw new Error('WebView 未能加载图片(asset 协议):' + JSON.stringify(loaded).slice(0, 300));
      }
      const f2 = ctx.fixtures.allMediaItems.find((x) => x.name === d.fileName);
      if (Number(loaded.naturalWidth) !== f2.width || Number(loaded.naturalHeight) !== f2.height) {
        throw new Error(
          'WebView 解码尺寸 ' + loaded.naturalWidth + 'x' + loaded.naturalHeight +
            ' 与夹具 ' + f2.width + 'x' + f2.height + ' 不符'
        );
      }
      return {
        fileName: d.fileName,
        width: d.width,
        height: d.height,
        availability: d.availability,
        webviewLoaded: { naturalWidth: loaded.naturalWidth, naturalHeight: loaded.naturalHeight, scheme: String(loaded.src).split(':')[0] },
      };
    });

    await runStep('chain:mark', async () => {
      const fav = await ipc(cdp, 'toggle_favorite', { itemId: items.ids[0] });
      if (fav !== true) throw new Error('toggle_favorite 返回 ' + JSON.stringify(fav));
      await ipc(cdp, 'set_rating', { itemId: items.ids[0], rating: 4 });
      const d = await ipc(cdp, 'get_media_detail', { id: items.ids[0] });
      if (d.isFavorited !== true || Number(d.rating) !== 4) {
        throw new Error('标记未落库:isFavorited=' + d.isFavorited + ' rating=' + d.rating);
      }
      const stats = await ipc(cdp, 'get_stats', null);
      return { isFavorited: d.isFavorited, rating: d.rating, totalFavorited: stats.totalFavorited };
    });

    // ── 目录移动(真实 IPC;源与目标都只碰 fixture 自有目录)────────────────────────
    // 目标必须是「另一个已扫描目录」,故先加根并扫描目标夹具根。
    const targetRoot = await runStep('chain:add-target-root', async () => {
      const r = await ipc(cdp, 'add_scan_root', { path: ctx.targetMediaDir, alias: 'Acceptance Target' });
      if (!r || typeof r.id !== 'number') throw new Error('add_scan_root(目标)返回异常:' + JSON.stringify(r));
      return { id: r.id };
    });

    await runStep('chain:target-scan', async () => {
      const channel = await openChannel(cdp);
      const started = ipc(
        cdp,
        'start_scan',
        {
          rootId: targetRoot.id,
          runId: 'acceptance-target-' + Date.now(),
          onProgress: channel,
          groupBy: null,
          sortWithinGroup: null,
          sortOrder: null,
          quick: false,
        },
        ctx.timeoutMs
      );
      const awaited = waitForChannel(
        cdp,
        (ev) => ev && (ev.type === 'completed' || ev.type === 'error'),
        ctx.timeoutMs,
        'target-scan'
      );
      await started;
      const hit = await awaited;
      if (hit.hit.type === 'error') throw new Error('目标根扫描报错:' + JSON.stringify(hit.hit));
      if (Number(hit.hit.totalItems) !== ctx.fixtures.target.items.length) {
        throw new Error('目标根入库 ' + hit.hit.totalItems + ' 项,期望 ' + ctx.fixtures.target.items.length);
      }
      return { totalItems: hit.hit.totalItems };
    });

    const movePlan = await runStep('chain:move-inputs', async () => {
      // 与 list-items 同一 helper:先前这里也是「tree 即全树」的假设,sub/ 一层就找不到了。
      const dirs = await walkDirectoryTree(cdp, root.id, ctx.timeoutMs);
      const rootDir = dirs[0];
      const movableDir = dirs.find((n) => n.name === ctx.fixtures.movable.dirName);
      if (!rootDir || !movableDir) {
        throw new Error('目录树缺根行或 movable/:' + JSON.stringify(dirs.map((n) => n.name)));
      }
      const targetDirs = await walkDirectoryTree(cdp, targetRoot.id, ctx.timeoutMs);
      const targetRootDir = targetDirs[0];
      if (!targetRootDir) throw new Error('目标根目录树为空');
      // 源目录里的条目 id(移动后要按同一批 id 复核身份、标记与路径)。
      const files = await ipc(cdp, 'list_directory_files', {
        directoryId: movableDir.id,
        limit: 100,
        offset: 0,
        categories: null,
      });
      if (!Array.isArray(files) || files.length !== ctx.fixtures.movable.items.length) {
        throw new Error('movable/ 列出 ' + (files ? files.length : 'null') + ' 项,期望 ' + ctx.fixtures.movable.items.length);
      }
      return {
        sourceDirId: movableDir.id,
        sourceRootDirId: rootDir.id,
        targetDirId: targetRootDir.id,
        itemIds: files.map((f) => f.id),
      };
    });

    // 移动前给源目录里的条目打上用户资产(收藏 + 评分),移动后要逐项核对没被动过。
    await runStep('chain:move-mark', async () => {
      const itemId = movePlan.itemIds[0];
      const fav = await ipc(cdp, 'toggle_favorite', { itemId });
      if (fav !== true) throw new Error('toggle_favorite 返回 ' + JSON.stringify(fav));
      await ipc(cdp, 'set_rating', { itemId, rating: 3 });
      const d = await ipc(cdp, 'get_media_detail', { id: itemId });
      if (d.isFavorited !== true || Number(d.rating) !== 3) {
        throw new Error('移动前标记未落库:' + JSON.stringify({ id: d.id, fav: d.isFavorited, rating: d.rating }));
      }
      return { itemId, isFavorited: d.isFavorited, rating: d.rating };
    });

    // 未完成移动清单:移动前后都必须空(表里留行 = 有未收尾的移动)。
    const pendingMoves = async (label) => {
      const pending = await ipc(cdp, 'list_pending_directory_moves', null);
      if (!Array.isArray(pending) || pending.length !== 0) {
        throw new Error(label + ':list_pending_directory_moves 应空,实际 ' + JSON.stringify(pending).slice(0, 300));
      }
      return { pending: 0 };
    };
    await runStep('chain:moves-idle-before', () => pendingMoves('移动前'));

    const moved = await runStep('chain:move-to-target', async () => {
      const res = await ipc(
        cdp,
        'move_directory',
        { sourceDirId: movePlan.sourceDirId, targetDirId: movePlan.targetDirId },
        ctx.timeoutMs
      );
      if (!res || typeof res.dirId !== 'number') throw new Error('move_directory 返回异常:' + JSON.stringify(res).slice(0, 300));
      if (res.dirId !== movePlan.sourceDirId) throw new Error('移动后目录 id 变了:' + res.dirId);
      if (Number(res.affectedMedia) !== ctx.fixtures.movable.items.length) {
        throw new Error('受影响条目 ' + res.affectedMedia + ',期望 ' + ctx.fixtures.movable.items.length);
      }
      if (!isUnder(res.targetAbsPath, ctx.targetMediaDir)) {
        throw new Error('移动落点不在目标根内:' + res.targetAbsPath);
      }
      if (res.recoveryId !== null && res.recoveryId !== undefined) throw new Error('移动未收尾,recoveryId=' + res.recoveryId);
      if (res.sourceLeftover !== null && res.sourceLeftover !== undefined) throw new Error('源残留未清理:' + res.sourceLeftover);
      return res;
    });

    await runStep('chain:move-verify', async () => {
      const itemId = movePlan.itemIds[0];
      const d = await ipc(cdp, 'get_media_detail', { id: itemId });
      if (d.id !== itemId) throw new Error('item id 漂移:' + d.id + ' ≠ ' + itemId);
      if (d.isFavorited !== true || Number(d.rating) !== 3) {
        throw new Error('移动后用户资产丢失:' + JSON.stringify({ fav: d.isFavorited, rating: d.rating }));
      }
      if (d.availability !== 'online') throw new Error('移动后条目不可用:' + d.availability);
      const abs = String(d.absPath || '');
      if (!isUnder(abs, ctx.targetMediaDir)) {
        throw new Error('移动后 absPath 不在目标根内:' + abs);
      }
      // source-direct 路径可读:IPC 给出的绝对路径直接读文件,内容 sha 必须与夹具逐字节一致。
      if (!fs.existsSync(abs)) throw new Error('移动后源直连路径不存在:' + abs);
      const onDisk = sha256Buf(fs.readFileSync(abs));
      const fixture = ctx.fixtures.movable.items.find((x) => x.name === d.fileName);
      if (!fixture) throw new Error('移动后文件名不在夹具内:' + d.fileName);
      if (onDisk !== fixture.sha256) throw new Error('移动后内容 sha 不符:' + d.fileName);
      const oldPath = path.join(ctx.fixtures.movable.dir, d.fileName);
      if (fs.existsSync(oldPath)) throw new Error('源位置仍留有文件(移动未真正搬走):' + oldPath);
      return { itemId, absPath: abs, sha256: onDisk.slice(0, 16), availability: d.availability };
    });

    await runStep('chain:moves-idle-after', () => pendingMoves('移动后'));

    // 搬回原处,让后续导出/备份/重启按原来的布局对照(移动本身已在上一步取到证据)。
    await runStep('chain:move-back', async () => {
      const res = await ipc(
        cdp,
        'move_directory',
        { sourceDirId: moved.dirId, targetDirId: movePlan.sourceRootDirId },
        ctx.timeoutMs
      );
      if (res.recoveryId !== null && res.recoveryId !== undefined) throw new Error('搬回未收尾,recoveryId=' + res.recoveryId);
      const d = await ipc(cdp, 'get_media_detail', { id: movePlan.itemIds[0] });
      if (d.isFavorited !== true || Number(d.rating) !== 3) throw new Error('搬回后用户资产丢失');
      const abs = String(d.absPath || '');
      if (!isUnder(abs, ctx.mediaDir)) {
        throw new Error('搬回后 absPath 不在源根内:' + abs);
      }
      const fixture = ctx.fixtures.movable.items.find((x) => x.name === d.fileName);
      if (sha256Buf(fs.readFileSync(abs)) !== fixture.sha256) throw new Error('搬回后内容 sha 不符');
      return { absPath: abs, affectedMedia: res.affectedMedia };
    });

    await runStep('chain:moves-idle-final', () => pendingMoves('搬回后'));

    // 新增命令的 ACL 取证(命令在 registry 里注册、capabilities 由构建期生成覆盖)。
    await runStep('chain:new-command-acl', async () => {
      // clear_semantic_search:新增搜索命令。能通即 ACL 已覆盖;不下载模型(空库上是幂等清空)。
      await ipc(cdp, 'clear_semantic_search', null);
      // retry_directory_move:用一个不可能存在的日志 id,应返回 null(这条命令已收尾)。
      const retried = await ipc(cdp, 'retry_directory_move', { recoveryId: 999999 });
      if (retried !== null && retried !== undefined) {
        throw new Error('retry_directory_move(不存在 id)应返回 null,实际 ' + JSON.stringify(retried).slice(0, 200));
      }
      return { clearSemanticSearch: 'ok', retryDirectoryMove: null };
    });

    // 打包内 worker 实际运行:逐个 spawn 并完成一次真实 Hello→Ready 握手(见函数头注明的模型边界)。
    await runStep('workers:handshake', async () => {
      const dir = path.dirname(ctx.exe);
      const results = [];
      const absent = [];
      for (const w of EXPECTED_WORKERS) {
        const exe = path.join(dir, w.file);
        if (!fs.existsSync(exe)) {
          absent.push(w.file);
          continue;
        }
        results.push(await handshakeWorker(exe, w.id, 30000));
      }
      if (absent.length === EXPECTED_WORKERS.length) {
        throw new Error('目录内没有任何预期 worker:' + dir + '(打包闭包不完整)');
      }
      if (absent.length) throw new Error('缺少 worker(无法验证运行):' + absent.join(', '));
      return {
        dir: path.relative(REPO, dir) || '.',
        workers: results,
        modelDependentPathsRun: false,
        note: '握手路径不承载模型加载;AI 推理/OCR/增强等模型分支未跑',
      };
    });

    const exportDir = path.join(ctx.root, 'export');
    fs.mkdirSync(exportDir, { recursive: true });
    await runStep('chain:export', async () => {
      const req = {
        selection: { kind: 'explicit', ids: items.ids },
        targetParent: exportDir,
        naming: 'sequence',
        conflict: 'rename',
        includeManifest: true,
        source: { kind: 'selection' },
        allowInsideLibrary: true,
      };
      const pre = await ipc(cdp, 'preflight_export', { req });
      if (!pre || Number(pre.count) !== expected) throw new Error('导出预检异常:' + JSON.stringify(pre));
      if (pre.targetWritable !== true) throw new Error('导出目标不可写:' + JSON.stringify(pre));
      const jobId = await ipc(cdp, 'start_export', { req }, ctx.timeoutMs);
      const done = await pollIpc(
        cdp,
        'export_status',
        null,
        (v) => v && v.jobId === jobId && v.status !== 'running' && v.status !== 'idle',
        ctx.timeoutMs,
        'export'
      );
      if (done.status !== 'completed') throw new Error('导出终态 ' + done.status + ':' + JSON.stringify(done));
      // 🔴 产物不在 targetParent 根下,而在它下面的**终局子目录**里:export/core.rs 用
      // `target_parent.join(final_dir_name(时间戳))` 创建,再由 finalDir 回报(同名时追加 -2/-3 去重)。
      // 在根下找 manifest 必然落空。
      const finalDir = String(done.finalDir || '');
      if (!finalDir) throw new Error('导出完成但未回报 finalDir:' + JSON.stringify(done).slice(0, 200));
      const absFinal = path.resolve(finalDir);
      const absExportRoot = path.resolve(exportDir);
      if (!isUnder(absFinal, absExportRoot)) {
        throw new Error('导出终局目录不在自有 exportDir 内:' + absFinal);
      }
      const png = countFiles(absFinal, '.png');
      if (png !== expected) throw new Error('终局目录 ' + png + ' 个 png,期望 ' + expected + '(' + absFinal + ')');
      const manifest = path.join(absFinal, 'manifest.scrollery.json');
      if (!fs.existsSync(manifest)) throw new Error('导出缺 manifest:' + manifest);
      // 计数也要卡死:只数 PNG 会漏「有跳过/失败但恰好也导出了 expected 个」;
      // succeeded 必须等于预期,itemsTotal(未封顶的失败/跳过总数)必须为 0——否则是父级累计计数误绿。
      if (Number(done.succeeded) !== expected) {
        throw new Error('succeeded=' + done.succeeded + ' 期望 ' + expected + ':' + JSON.stringify(done).slice(0, 200));
      }
      if (Number(done.itemsTotal || 0) !== 0) {
        throw new Error('存在 ' + done.itemsTotal + ' 项失败/跳过(期望 0):' + JSON.stringify(done.items || []).slice(0, 200));
      }
      return { jobId, finalDir: absFinal, png, succeeded: done.succeeded, itemsTotal: 0, manifestBytes: fs.statSync(manifest).size };
    });

    const backupDir = path.join(ctx.root, 'backup');
    fs.mkdirSync(backupDir, { recursive: true });
    const backup = await runStep('chain:backup', async () => {
      const pre = await ipc(cdp, 'preflight_backup', { dest: backupDir });
      if (!pre || pre.writable !== true) throw new Error('备份目标不可写:' + JSON.stringify(pre));
      if (pre.documentsConsistent !== true) throw new Error('文档行与文件不一致,备份会硬失败:' + JSON.stringify(pre));
      // 🔴 先记下执行前已有的包 —— 目标目录里可能留着上一轮的产物(2026-09-12 实测踩过:
      // list_backups 含前次包,`length >= 1` 立刻命中,于是把**旧包**当成本轮结果写进证据,
      // 而本轮真实状态还是 running)。判据必须锚定「本轮新增的那一个」。
      const before = await ipc(cdp, 'list_backups', { dest: backupDir });
      const beforePaths = new Set((Array.isArray(before) ? before : []).map((b) => b.path));
      await ipc(cdp, 'start_backup', { dest: backupDir }, ctx.timeoutMs);
      const listed = await pollIpc(
        cdp,
        'list_backups',
        { dest: backupDir },
        (v) => Array.isArray(v) && v.some((b) => !beforePaths.has(b.path)),
        ctx.timeoutMs,
        'backup-package'
      );
      const status = await ipc(cdp, 'backup_status', null);
      // 终态必须是 completed:failed / cancelled 都不算成功(之前只查了 failed)。
      if (!status || status.status !== 'completed') {
        throw new Error('备份终态应为 completed,实际 ' + JSON.stringify(status).slice(0, 200));
      }
      // 终态回报的 path 才是本轮包;它必须出现在列表里,且不属于执行前的旧集合。
      const finalPath = String(status.path || '');
      if (!finalPath) throw new Error('备份 completed 但未回报 path:' + JSON.stringify(status).slice(0, 200));
      const added = listed.filter((b) => !beforePaths.has(b.path));
      if (!added.some((b) => b.path === finalPath)) {
        throw new Error('终态 path 不在本轮新增包里:' + finalPath + ' (新增 ' + JSON.stringify(added.map((b) => b.path)) + ')');
      }
      return {
        beforeCount: beforePaths.size,
        afterCount: listed.length,
        added: added.length,
        packagePath: finalPath,
        backupId: (added.find((b) => b.path === finalPath) || {}).backupId || null,
        status: status.status,
      };
    });

    await runStep('chain:clean-exit', () => exitClean(cdp, first.child, 'chain 首启'));
    first.session.close();

    const second = await launch('chain-restart');
    await runStep('chain:restart-state', async () => {
      const roots = await ipc(second.cdp, 'list_scan_roots', null);
      // 两个扫描根:源夹具根 + 目录移动用的目标根。
      if (!Array.isArray(roots) || roots.length !== 2) throw new Error('重启后扫描根异常:' + JSON.stringify(roots).slice(0, 200));
      const d = await ipc(second.cdp, 'get_media_detail', { id: items.ids[0] });
      if (d.isFavorited !== true || Number(d.rating) !== 4) {
        throw new Error('重启后标记丢失:isFavorited=' + d.isFavorited + ' rating=' + d.rating);
      }
      const webp = countThumbs(cache.dir);
      if (webp < 1) throw new Error('重启后缩略图缓存为空');
      return { roots: roots.length, isFavorited: d.isFavorited, rating: d.rating, thumbWebp: webp, viewed: detail.fileName };
    });

    if (ctx.restoreDrill) {
      // 🔴 判据必须先「破坏」再验:备份后把标记改成与备份内**不同**的值,恢复后才要求旧值回来。
      // 若只断言「恢复后 isFavorited=true / rating=4」,那么恢复根本没执行时这些值本来也还是原样,
      // 测试照样通过——那是一条不证明任何事的断言。这里改成:备份(收藏+4) → 改成(未收藏+2)
      // → 恢复 → 必须回到(收藏+4),只有真发生了恢复才可能满足。
      await runStep('chain:restore-mutate-after-backup', async () => {
        const itemId = items.ids[0];
        const unfav = await ipc(second.cdp, 'toggle_favorite', { itemId });
        if (unfav !== false) throw new Error('取消收藏失败:' + JSON.stringify(unfav));
        await ipc(second.cdp, 'set_rating', { itemId, rating: 2 });
        const now = await ipc(second.cdp, 'get_media_detail', { id: itemId });
        if (now.isFavorited !== false || Number(now.rating) !== 2) {
          throw new Error('备份后改写未生效:' + JSON.stringify({ fav: now.isFavorited, rating: now.rating }));
        }
        return { isFavorited: now.isFavorited, rating: now.rating, note: '与备份内容不同,恢复后应被旧值覆盖' };
      });

      const staged = await runStep('chain:restore-stage', async () => {
        const r = await ipc(second.cdp, 'restore_stage', { packagePath: backup.packagePath }, ctx.timeoutMs);
        if (!r || !r.backupId || !r.stagingDir) throw new Error('restore_stage 返回异常:' + JSON.stringify(r).slice(0, 240));
        // 🔴 必须把 stagingDir 带出来:Rust 侧 `staging_dir: String` 是**必填**字段(非 Option),
        // 漏传会让 IPC 以「缺字段」失败;而且它并非无用参数——后端拿它做一致性核对(非空且与
        // backupId 派生路径不符即拒)。
        return {
          backupId: r.backupId,
          stagingDir: r.stagingDir,
          schemaVersion: r.schemaVersion,
          needsMigration: r.needsMigration,
          counts: r.counts,
        };
      });
      await runStep('chain:restore-arm', async () => {
        await ipc(
          second.cdp,
          'restore_arm',
          { backupId: staged.backupId, stagingDir: staged.stagingDir },
          ctx.timeoutMs
        );
        const marker = path.join(ctx.appDataDir, 'pending-restore.json');
        if (!fs.existsSync(marker)) throw new Error('arm 后未见恢复标记:' + marker);
        return { marker: path.basename(marker) };
      });
      await runStep('chain:restore-exit', () => exitClean(second.cdp, second.child, 'chain 恢复前'));
      second.session.close();
      const third = await launch('chain-restore');
      await runStep('chain:restore-swap', async () => {
        const marker = path.join(ctx.appDataDir, 'pending-restore.json');
        if (fs.existsSync(marker)) throw new Error('重启后恢复标记未清理(boot swap 未收口)');
        const d = await ipc(third.cdp, 'get_media_detail', { id: items.ids[0] });
        // 断言的是**备份时的旧值**(收藏 + 4),而不是恢复前刚写下的新值(未收藏 + 2)。
        // 结合上面的 mutate 步骤:只有真发生了恢复,才可能看到旧值回来。
        if (!d || d.isFavorited !== true || Number(d.rating) !== 4) {
          throw new Error(
            '恢复后未回到备份时状态(期望 收藏+4,实际 ' +
              JSON.stringify({ fav: d && d.isFavorited, rating: d && d.rating }) +
              ') → 恢复可能未真正执行'
          );
        }
        return {
          markerCleared: true,
          restoredToBackup: { isFavorited: d.isFavorited, rating: d.rating },
          overwrittenThenDiscarded: { isFavorited: false, rating: 2 },
        };
      });
      await runStep('chain:restore-clean-exit', () => exitClean(third.cdp, third.child, 'chain 恢复后'));
      third.session.close();
    }
    // 终态哨兵:只有真的走完 chain 才会记下这一步。配合上面的结算检查,
    // 「跑一半静默退出」就再也不会被当成通过。
    await runStep('chain:done', () => ({ note: 'chain 全流程已执行到末尾' }));
  } finally {
    for (const s of sessions) {
      try {
        s.close();
      } catch (_) {}
    }
    for (const c of children) killOurProcess(c);
  }
}

// ── 入口 ──────────────────────────────────────────────────────────────────────

function makeContext() {
  const stage = arg('stage', 'boot');
  if (!STAGES.includes(stage)) throw new Error('未知 --stage=' + stage + '(可选 ' + STAGES.join('|') + ')');
  const exe = assertAcceptanceExe(arg('exe', path.join(REPO, 'target', 'release', 'scrollery.exe')));
  // 启动前必须拿到的三份证据(缺任一即拒绝):字节内身份、版本资源身份、记录下来的 exe 哈希。
  // 全部在 spawn 之前完成——普通构建 setup 期就会写用户日常库,「先跑起来再查 appDataDir」那时库已脏。
  const identity = assertAcceptanceIdentity(exe);
  const proofed = assertBuildProof(exe);
  const appDataDir = assertAcceptanceDir(appDataDirFor(ACCEPTANCE_IDENTIFIER));
  const userLibrary = userLibraryTopLevelFingerprint();
  const root = assertControlledRoot(arg('root', path.join(REPO, 'target', 'acceptance')));
  // 目标根:默认放在验收根内(同卷);给了 --target-root 就用它——真给另一块盘时
  // 本次运行会走跨卷「暂存→发布→删源」路径,见 chain 的卷别判定与说明。
  const targetMediaDir = assertControlledRoot(arg('target-root', path.join(root, 'media-target')));
  const mediaCount = Number(arg('media', '9'));
  if (!Number.isInteger(mediaCount) || mediaCount < 3) throw new Error('--media 至少 3(N 张图,含子目录与非媒体文件)');
  const ctx = {
    stage,
    exe,
    appDataDir,
    root,
    mediaDir: path.join(root, 'media'),
    targetMediaDir,
    exportDir: path.join(root, 'export'),
    backupDir: path.join(root, 'backup'),
    logDir: path.join(root, 'logs'),
    port: Number(arg('port', '9333')),
    timeoutMs: Number(arg('timeout', '180')) * 1000,
    restoreDrill: hasFlag('restore-drill'),
    // 安装后入口:--install 让后续阶段跑在「NSIS 装出来的目录」里(而不是裸 exe);
    // --uninstall-after 在阶段跑完后静默卸载我们自有目录里的那一份。
    install: hasFlag('install'),
    installOnly: hasFlag('install-only'),
    uninstallAfter: hasFlag('uninstall-after'),
    verifyUserLibrary: hasFlag('verify-user-library-untouched'),
    keep: hasFlag('keep'),
    reportTag: arg('report-tag', null),
    userLibrary,
    sameVolume: sameVolume(path.join(root, 'media'), targetMediaDir),
    identity: {
      exeSha256: identity.info.exeSha256,
      productName: identity.info.productName,
      proofPath: proofed.proofPath,
    },
  };
  return ctx;
}

function prepare(ctx) {
  fs.mkdirSync(ctx.logDir, { recursive: true });
  // 默认全新开始:清掉上一次验收的隔离库与夹具。删除前逐项核对(见两个守卫的实现):app-data 必须
  // 落在 Windows 解析出的 Roaming 下、名字等于验收 identifier、内容像验收应用数据;夹具目录必须带
  // 本脚本写的标识文件。核对不过就拒绝删除——不做「删了再说」。
  // --keep 用于「同一隔离库跨阶段续测」,此时保留库与夹具。
    if (!ctx.keep) {
      fs.rmSync(assertDeletableAppData(ctx.appDataDir), { recursive: true, force: true });
      fs.rmSync(assertDeletableFixtureRoot(ctx.mediaDir, ctx.root), { recursive: true, force: true });
      fs.rmSync(assertDeletableFixtureRoot(ctx.targetMediaDir, ctx.root), { recursive: true, force: true });
    }
  ctx.fixtures = writeFixtures(ctx.mediaDir, ctx.targetMediaDir, Number(arg('media', '9')));
  fs.mkdirSync(ctx.exportDir, { recursive: true });
  fs.mkdirSync(ctx.backupDir, { recursive: true });
  const payload = listPayload(path.dirname(ctx.exe));
  console.log('== 隔离验收(' + ctx.stage + ') ==');
  console.log('  验收包     : ' + ctx.exe);
  console.log('  产物身份   : sha256 ' + ctx.identity.exeSha256.slice(0, 16) + '… / ProductName "' + ctx.identity.productName + '"');
  console.log('  构建证明   : ' + ctx.identity.proofPath);
  console.log('  隔离 app-data: ' + ctx.appDataDir);
  console.log('  媒体夹具   : ' + ctx.mediaDir + '(' + ctx.fixtures.items.length + ' 张 PNG + 1 非媒体)');
  console.log('  目标根夹具 : ' + ctx.targetMediaDir + '(' + ctx.fixtures.target.items.length + ' 张 PNG;目录移动的目标宿主)');
  console.log('  源/目标卷  : ' + (ctx.sameVolume ? '同卷(rename 路径)' : '跨卷(暂存→发布→删源路径)'));
  console.log('  旁置载荷   : ' + (payload.present.length ? payload.present.join(', ') : '(无)'));
  if (payload.missing.length) console.log('  载荷缺项   : ' + payload.missing.join(', ') + '(不阻断本脚本;安装包载荷断言归 verify-bundle-content)');
  return payload;
}

async function selftest() {
  const fails = [];
  const expect = (cond, msg) => {
    if (!cond) fails.push(msg);
  };
  // PNG 夹具必须真能被解析回正确尺寸(生成器退化 = 验收基线不可信)。
  for (const [w, h] of FIXTURE_SIZES.concat([[7, 5]])) {
    const parsed = parsePng(makePng(w, h, 3));
    expect(parsed.ihdr && parsed.ihdr.width === w && parsed.ihdr.height === h, 'PNG 往返尺寸不符:' + w + 'x' + h);
    expect(parsed.types.join(',') === 'IHDR,IDAT,IEND', 'PNG chunk 序列异常:' + parsed.types.join(','));
  }
  expect(makePng(16, 16, 1).equals(makePng(16, 16, 1)), 'PNG 生成不确定(同输入两次不一致)');
  expect(!makePng(16, 16, 1).equals(makePng(16, 16, 2)), 'PNG 生成未随 seed 变化');
  // 路径守卫:验收身份目录放行,用户真实库必须拦下。
  const acceptance = path.join(resolveKnownFolders().roaming, ACCEPTANCE_IDENTIFIER);
  let blocked = false;
  try {
    assertAcceptanceDir(path.join(resolveKnownFolders().roaming, USER_IDENTIFIER));
  } catch (_) {
    blocked = true;
  }
  expect(blocked, '路径守卫未拦下用户真实库');
  blocked = false;
  try {
    assertAcceptanceDir(path.join(resolveKnownFolders().roaming, USER_IDENTIFIER, 'sub'));
  } catch (_) {
    blocked = true;
  }
  expect(blocked, '路径守卫未拦下用户真实库子目录');
  expect(assertAcceptanceDir(acceptance).endsWith(ACCEPTANCE_IDENTIFIER), '路径守卫误拦验收目录');
  // 版本资源桥接(FileVersionInfo)双向取证:非 PE 文件必须读出空 ProductName(不是抛错、更不是
  // 「有值」),而真实产物在盘时必须读出真 ProductName——恒空或恒有值都会让身份门失效。
  const verTmp = fs.mkdtempSync(path.join(process.env.TEMP || '.', 'acceptance-ver-'));
  try {
    const notPe = path.join(verTmp, 'not-a-pe.exe');
    fs.writeFileSync(notPe, Buffer.from('not-a-pe'));
    expect(readFileVersionInfo(notPe).ProductName === '', '非 PE 文件竟读出 ProductName');
    const realExe = [
      path.join(resolveKnownFolders().local, 'Scrollery', 'scrollery.exe'),
      path.join(REPO, 'src-tauri', 'binaries', 'ai-worker-x86_64-pc-windows-msvc.exe'),
    ].find((p) => fs.existsSync(p));
    if (realExe) {
      const name = readFileVersionInfo(realExe).ProductName;
      if (name) expect(name.length > 0, '真实产物读不出 ProductName:' + realExe);
      else console.log('  (提示:' + path.basename(realExe) + ' 无版本资源,正样本待真实 Tauri 产物验证)');
    }
  } finally {
    fs.rmSync(verTmp, { recursive: true, force: true });
  }
  // 端口守卫(有界等待语义):
  //   ① 有监听者时,宽限期内拿不到就**失败**(不 attach、不杀占用者);
  //   ② 期间释放,则等待成功(这正是「刚退出会话的 WebView2 子进程晚一步放端口」那条路径);
  //   ③ 空闲端口立即通过。①用短 grace 免得自检等满 10s。
  const portHolder = net.createServer(() => {});
  await new Promise((resolve, reject) => {
    portHolder.once('error', reject);
    portHolder.listen(0, '127.0.0.1', resolve);
  });
  const busyPort = portHolder.address().port;
  let portBlocked = false;
  try {
    await assertPortFree(busyPort, 800);
  } catch (_) {
    portBlocked = true;
  }
  expect(portBlocked, '端口守卫未在有监听者时按宽限期失败');
  // 期间释放 → 等待应成功(而不是直接判失败)。
  const releaseLater = assertPortFree(busyPort, 8000);
  setTimeout(() => portHolder.close(() => {}), 1200);
  await releaseLater;
  // 空闲端口立即通过。
  const freeProbe = net.createServer();
  await new Promise((resolve) => freeProbe.listen(0, '127.0.0.1', resolve));
  const freePort = freeProbe.address().port;
  await new Promise((resolve) => freeProbe.close(resolve));
  await assertPortFree(freePort, 3000);
  // 实例身份守卫:只读 IPC 返回的路径必须落在预期验收 app-data 之下。反样本取用户真实库——
  // 这正是「attach 到别人实例」时会观测到的值(别人的 logDir 会指到用户库)。
  const userDir = path.join(resolveKnownFolders().roaming, USER_IDENTIFIER);
  expect(
    attachedIdentityViolations({ logDir: path.join(userDir, 'logs') }, acceptance).length === 1,
    '身份守卫未拦下用户真实库下的 logDir'
  );
  expect(
    attachedIdentityViolations({ thumbCacheDir: path.join(userDir, 'cache') }, acceptance).length === 1,
    '身份守卫未拦下用户真实库下的 thumbCacheDir'
  );
  expect(
    attachedIdentityViolations({ logDir: null, thumbCacheDir: '' }, acceptance).length === 2,
    '身份守卫未拦下读不到路径(不可证)的返回值'
  );
  expect(
    attachedIdentityViolations({ configPath: path.join(userDir, 'config.toml') }, acceptance).length === 1,
    '身份守卫未拦下用户真实库下的 config.toml'
  );
  expect(
    attachedIdentityViolations(
      { logDir: path.join(acceptance, 'logs'), thumbCacheDir: path.join(acceptance, 'cache') },
      acceptance
    ).length === 0,
    '身份守卫对验收实例误报'
  );
  // 同前缀的兄弟目录不是子目录,不能靠字符串侥幸放行。
  expect(
    attachedIdentityViolations({ logDir: acceptance + '-backup' + path.sep + 'logs' }, acceptance).length === 1,
    '身份守卫未拦下同前缀兄弟目录'
  );
  // 身份门:只带 identifier 字节、没有版本资源的文件必须被拒(哈希或 marker 单独都证明不了身份)。
  const gateTmp = fs.mkdtempSync(path.join(process.env.TEMP || '.', 'acceptance-gate-'));
  try {
    const markerOnly = path.join(gateTmp, 'marker-only.exe');
    fs.writeFileSync(markerOnly, Buffer.concat([Buffer.from('MZ'), Buffer.from(ACCEPTANCE_IDENTIFIER, 'utf8')]));
    expect(analyzeExe(markerOnly).hasIdentifierMarker === true, '身份分析未识别 identifier 字节');
    expect(analyzeExe(markerOnly).productName === null, '身份分析对非 PE 谎报读出 ProductName');
    let caught = false;
    try {
      assertAcceptanceIdentity(markerOnly);
    } catch (_) {
      caught = true;
    }
    expect(caught, '身份门未拦下「只有 marker 字节、无版本资源」的产物');
    // 构建证明门:缺证明 / exe 哈希不符 / 配置漂移三种都必须拦下,匹配的证明必须放行。
    const proofFile = path.join(gateTmp, 'proof.json');
    const cfg = readAcceptanceConfig();
    const tryProof = () => {
      try {
        assertBuildProof(markerOnly, proofFile);
        return false;
      } catch (_) {
        return true;
      }
    };
    let proofBlocked = 0;
    if (tryProof()) proofBlocked++;
    fs.writeFileSync(proofFile, JSON.stringify({ exeSha256: 'deadbeef', configSha256: cfg.sha256, identifier: ACCEPTANCE_IDENTIFIER }, null, 2));
    if (tryProof()) proofBlocked++;
    fs.writeFileSync(proofFile, JSON.stringify({ exeSha256: analyzeExe(markerOnly).exeSha256, configSha256: 'stale', identifier: ACCEPTANCE_IDENTIFIER }, null, 2));
    if (tryProof()) proofBlocked++;
    expect(proofBlocked === 3, '构建证明门漏放(应拦 3 种,实拦 ' + proofBlocked + ' 种)');
    fs.writeFileSync(proofFile, JSON.stringify({ exeSha256: analyzeExe(markerOnly).exeSha256, configSha256: cfg.sha256, identifier: ACCEPTANCE_IDENTIFIER }, null, 2));
    expect(tryProof() === false, '构建证明门对匹配的证明误拦');
  } finally {
    fs.rmSync(gateTmp, { recursive: true, force: true });
  }
  // 受控删除守卫:名字对但不在 Windows 解析位置、或内容不像验收数据的目录必须拒删;
  // 夹具目录没有本脚本标识文件必须拒删;受保护目录不能被当作验收根。
  const guardTmp = fs.mkdtempSync(path.join(process.env.TEMP || '.', 'acceptance-guard-'));
  try {
    const fakeAppData = path.join(guardTmp, ACCEPTANCE_IDENTIFIER);
    fs.mkdirSync(fakeAppData, { recursive: true });
    fs.writeFileSync(path.join(fakeAppData, 'someone-elses-data.bin'), 'x');
    let blocked2 = false;
    try {
      assertDeletableAppData(fakeAppData);
    } catch (_) {
      blocked2 = true;
    }
    expect(blocked2, '受控删除守卫未拦下「不在 Roaming 下」的同名目录');
    const noMarker = path.join(guardTmp, 'fixture-without-marker');
    fs.mkdirSync(noMarker, { recursive: true });
    fs.writeFileSync(path.join(noMarker, 'photo.png'), 'x');
    blocked2 = false;
    try {
      assertDeletableFixtureRoot(noMarker, guardTmp);
    } catch (_) {
      blocked2 = true;
    }
    expect(blocked2, '受控删除守卫未拦下没有标识文件的夹具目录');
    const marked = path.join(guardTmp, 'fixture-with-marker');
    fs.mkdirSync(marked, { recursive: true });
    fs.writeFileSync(path.join(marked, FIXTURE_MARKER), '{}');
    expect(assertDeletableFixtureRoot(marked, guardTmp) === marked, '受控删除守卫误拦带标识的夹具目录');
    blocked2 = false;
    try {
      assertDeletableFixtureRoot(path.join(os.tmpdir(), 'acceptance-outside-root'), guardTmp);
    } catch (_) {
      blocked2 = true;
    }
    expect(blocked2, '受控删除守卫未拦下验收根之外的路径');
    for (const badRoot of [path.parse(guardTmp).root, os.homedir(), REPO]) {
      let caughtRoot = false;
      try {
        assertControlledRoot(badRoot);
      } catch (_) {
        caughtRoot = true;
      }
      expect(caughtRoot, '受控根守卫未拦下受保护目录:' + badRoot);
    }
    expect(assertControlledRoot(guardTmp) === guardTmp, '受控根守卫误拦普通临时目录');
  } finally {
    fs.rmSync(guardTmp, { recursive: true, force: true });
  }
  // 夹具落盘:数量 + 子目录 + 非媒体文件。
  // 安装目录守卫:验收根外、用户已装目录、含空白路径都必须拒;合规目录放行;ownership 只认自有。
  const installTmp = fs.mkdtempSync(path.join(process.env.TEMP || '.', 'acceptance-install-'));
  try {
    const okDir = installDirFor(installTmp);
    expect(assertInstallableDir(okDir, installTmp) === path.resolve(okDir), '安装目录守卫误拦合规目录');
    let caughtInstall = false;
    try {
      assertInstallableDir(path.join(process.env.TEMP || '.', 'outside-root-install'), installTmp);
    } catch (_) {
      caughtInstall = true;
    }
    expect(caughtInstall, '安装目录守卫未拦下验收根之外的目录');
    caughtInstall = false;
    try {
      assertInstallableDir(path.join(userInstallDir(), 'nested'), installTmp);
    } catch (_) {
      caughtInstall = true;
    }
    expect(caughtInstall, '安装目录守卫未拦下用户已装目录');
    caughtInstall = false;
    try {
      assertInstallableDir(path.join(userInstallDir(), '..', 'ScrollerySibling'), installTmp);
    } catch (_) {
      caughtInstall = true;
    }
    expect(caughtInstall, '安装目录守卫未拦下用户已装目录的兄弟路径(仍需在验收根内)');
    caughtInstall = false;
    try {
      assertInstallableDir(path.join(installTmp, 'with space'), installTmp);
    } catch (_) {
      caughtInstall = true;
    }
    expect(caughtInstall, '安装目录守卫未拦下含空白的路径(NSIS /D= 会截断)');
    // ownership:空目录/自有标识放行;有内容且无标识 → 拒。
    expect(claimInstallDir(okDir) === okDir, '安装目录认领失败');
    expect(fs.existsSync(path.join(okDir, INSTALL_MARKER)), '安装目录缺标识文件');
    const foreignDir = path.join(installTmp, 'foreign-install');
    fs.mkdirSync(foreignDir, { recursive: true });
    fs.writeFileSync(path.join(foreignDir, 'someone-else.bin'), 'x');
    let caughtClaim = false;
    try {
      claimInstallDir(foreignDir);
    } catch (_) {
      caughtClaim = true;
    }
    expect(caughtClaim, '安装目录认领未拦下非自有目录');
  } finally {
    fs.rmSync(installTmp, { recursive: true, force: true });
  }

  // 从安装目录认主程序:按嵌入 identifier 认,不靠文件名;无候选/多候选都要报错。
  const instProbe = fs.mkdtempSync(path.join(process.env.TEMP || '.', 'acceptance-instexe-'));
  try {
    fs.writeFileSync(path.join(instProbe, 'uninstall.exe'), Buffer.from('MZ-no-marker'));
    fs.writeFileSync(path.join(instProbe, 'scrollery.exe'), Buffer.concat([Buffer.from('MZ'), Buffer.from(ACCEPTANCE_IDENTIFIER, 'utf8')]));
    const foundExe = findInstalledExe(instProbe, ACCEPTANCE_IDENTIFIER);
    expect(path.basename(foundExe) === 'scrollery.exe', '安装目录认主程序找错文件:' + path.basename(foundExe));
    fs.writeFileSync(path.join(instProbe, 'other.exe'), Buffer.concat([Buffer.from('MZ'), Buffer.from(ACCEPTANCE_IDENTIFIER, 'utf8')]));
    let multiCaught = false;
    try {
      findInstalledExe(instProbe, ACCEPTANCE_IDENTIFIER);
    } catch (_) {
      multiCaught = true;
    }
    expect(multiCaught, '安装目录认主程序未拦下多候选');
    fs.rmSync(path.join(instProbe, 'other.exe'));
    fs.rmSync(path.join(instProbe, 'scrollery.exe'));
    let noneCaught = false;
    try {
      findInstalledExe(instProbe, ACCEPTANCE_IDENTIFIER);
    } catch (_) {
      noneCaught = true;
    }
    expect(noneCaught, '安装目录认主程序未拦下无候选');
  } finally {
    fs.rmSync(instProbe, { recursive: true, force: true });
  }

  const tmp = fs.mkdtempSync(path.join(process.env.TEMP || '.', 'acceptance-selftest-'));
  try {
    const fx = writeFixtures(path.join(tmp, 'media'), path.join(tmp, 'media-target'), 9);
    expect(fx.items.length === 9, '夹具数量不符:' + fx.items.length);
    expect(fx.items.filter((i) => i.inSub).length === 3, '子目录夹具数量不符');
    expect(fs.existsSync(fx.noteFile), '非媒体夹具缺失');
    expect(fs.existsSync(path.join(path.dirname(fx.noteFile), FIXTURE_MARKER)), '夹具标识文件缺失');
    for (const it of fx.items) {
      const parsed = parsePng(fs.readFileSync(it.file));
      expect(parsed.ihdr.width === it.width, '落盘夹具尺寸漂移:' + it.name);
    }
    // 目录移动夹具:movable 子树 + 目标根,且每条都带内容 sha(移动后按 sha 证明搬的是同一份)。
    expect(fx.movable.items.length === 2, 'movable 条目数不符:' + fx.movable.items.length);
    expect(fx.allMediaItems.length === fx.items.length + 2, '源根媒体总数不符');
    expect(fx.target.items.length === 1, '目标根条目数不符:' + fx.target.items.length);
    expect(fs.existsSync(path.join(tmp, 'media-target', FIXTURE_MARKER)), '目标根标识文件缺失');
    for (const it of [...fx.items, ...fx.movable.items, ...fx.target.items]) {
      expect(/^[0-9a-f]{64}$/.test(it.sha256), '夹具条目缺内容 sha:' + it.name);
      expect(sha256Buf(fs.readFileSync(it.file)) === it.sha256, '夹具内容 sha 与落盘不符:' + it.name);
    }
    // 卷别判定:同一棵临时树的两个根必然同卷;不同盘符必须判为跨卷。
    expect(sameVolume(path.join(tmp, 'media'), path.join(tmp, 'media-target')), '同卷判定误报');
    expect(!sameVolume('C:/x', 'D:/x'), '不同盘符应判为跨卷');
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  if (fails.length) {
    console.error('selftest 失败:');
    for (const f of fails) console.error('  - ' + f);
    process.exit(2);
  }
  console.log('✓ selftest 通过(PNG + 身份门/构建证明 + 版本资源桥接 + 端口占用/实例身份守卫 + 受控删除 + 夹具/卷别)');
}

async function main() {
  await selftest();
  if (hasFlag('selftest')) {
    process.exit(0);
  }
  // 记证/探针模式:只读产物与配置,不 spawn、不删任何目录。
  //   --record-build  验收构建完成后跑一次,把「这批产物 = 这份配置」钉成证明;此后每次运行都要
  //                   拿它比对,产物被重建或配置被改动都会在启动前被拒。
  //   --probe-build   只打印身份分析结果(排障用),任一门失败返回非零。
  if (hasFlag('record-build') || hasFlag('probe-build')) {
    const exePath = assertAcceptanceExe(arg('exe', path.join(REPO, 'target', 'release', 'scrollery.exe')));
    const info = analyzeExe(exePath);
    console.log('== 产物身份分析 ==');
    console.log('  exe            : ' + exePath);
    console.log('  sha256         : ' + info.exeSha256);
    console.log('  bytes          : ' + info.exeBytes);
    console.log('  identifier     : ' + (info.hasIdentifierMarker ? ACCEPTANCE_IDENTIFIER : '(未含验收 identifier)'));
    console.log('  ProductName    : ' + (info.productName || '(无)'));
    console.log('  FileDesc       : ' + (info.fileDescription || '(无)'));
    console.log('  FileVersion    : ' + (info.fileVersion || '(无)'));
    assertAcceptanceIdentity(exePath);
    if (hasFlag('record-build')) {
      const proof = recordBuildProof(exePath);
      console.log('✓ 身份校验通过;构建证明已写入:' + proofPath());
      console.log('  ' + proof.exeSha256.slice(0, 16) + '… / config ' + proof.configSha256.slice(0, 16) + '… / ' + proof.productName);
    } else {
      const proofed = assertBuildProof(exePath);
      console.log('✓ 身份校验与构建证明均通过(' + proofed.proofPath + ')');
    }
    return;
  }
  const ctx = makeContext();
  prepare(ctx);
  // 🔴 保活定时器:防止「事件循环空转 → Node 静默 exit 0」把一次未跑完的验收伪装成成功。
  // 2026-09-12 实测踩过:chain 跑到 chain:backup 后进程直接退出、退出码 0、报告与日志都停在那一刻,
  // 交付目录里留下的还是上一轮的失败报告。AsyncLocal 的 pending promise 不持有 handle,一旦当前
  // await 所依赖的唯一 handle(WebSocket 等)关闭,Node 就会当作无事可做而退出——退出码还是 0。
  KEEPALIVE = setInterval(() => {}, 1000);

  // 安装后入口(GO 后的首选路径):装到自有目录,取证通过后把 ctx.exe 换成安装目录里的主程序。
  // 环境拒绝(权限/组策略/NSIS)不当作验收通过:记为失败步骤,退回构建产物继续跑可做的部分。
  if (ctx.install || ctx.installOnly) {
    try {
      const installed = await runStep('install:nsis-silent', () => installAcceptanceEntry(ctx));
      // 成功也要带 ok:true,否则报告里只有失败路径显式写 ok:false,读的人分不清「成功了」与「没跑」。
      ctx.installedEvidence = Object.assign({ ok: true }, installed.evidence);
      if (!ctx.installOnly) ctx.exe = installed.exe;
      await runStep('install:installed-identity', () => {
        assertAcceptanceIdentity(installed.exe);
        const proof = JSON.parse(fs.readFileSync(proofPath(), 'utf8'));
        const hash = sha256Buf(fs.readFileSync(installed.exe));
        // 比对目标是**证明里记录的 NSIS 载荷**(而非 exeSha256——那是占位符还原版的 hash,见证明内注释)。
        const nsis = ((proof.bundle && proof.bundle.installers) || []).find((i) => i.kind === 'nsis');
        if (!nsis) throw new Error('构建证明里没有 nsis 条目:先跑 --record-build(构建需带 bundle)');
        if (!nsis.payloadExe) throw new Error('构建证明里没有包内主程序指纹:需 7z 重新 --record-build');
        if (hash !== nsis.payloadExe.sha256) {
          throw new Error(
            '安装产物 ≠ 证明记录的包内主程序:' + hash.slice(0, 16) + '… vs ' + nsis.payloadExe.sha256.slice(0, 16) + '…'
          );
        }
        return {
          installedExe: installed.exe,
          sha16: hash.slice(0, 16),
          matchesProvenPayload: true,
          installerSha16: nsis.sha256.slice(0, 16),
        };
      });
    } catch (err) {
      const message = err && err.message ? err.message : String(err);
      // 不再重复 push 一条同名失败:runStep 已经记录过(ok:false + error + 已打印 [FAIL]),
      // 再记一条会让报告出现同一失败两次,也让人分不清实际失败了几步。这里只补上下文。
      ctx.installedEvidence = {
        ok: false,
        error: message,
        note: '安装入口不可用;后续阶段退回构建产物继续(报告中 installedEntry.ok=false,且整体判失败)',
      };
    }
  }
  const t0 = Date.now();
  if (ctx.stage === 'boot') await stageBoot(ctx);
  else if (ctx.stage === 'ready') await stageReady(ctx);
  else await stageChain(ctx);
  await runStep('isolation:user-library-top-level-unchanged', () => {
    if (!ctx.verifyUserLibrary) return { skipped: '未加 --verify-user-library-untouched' };
    const after = userLibraryTopLevelFingerprint();
    const before = JSON.stringify(ctx.userLibrary);
    if (JSON.stringify(after) !== before) {
      throw new Error('用户真实库**顶层元数据**发生变化(验收越界!)');
    }
    // 覆盖边界如实标注:这不是全库逐字节零改动的证明(见 fingerprint 函数注释)。
    return { entries: after.length, scope: '顶层条目名 + mtimeMs', byteLevelProof: false };
  });
  // MSI:只验构建+载荷,不安装(不是本次安装入口;载荷解包断言由 verify-bundle-content 覆盖)。
  await runStep('install:msi-not-installed', () => {
    const msiDir = path.join(REPO, 'target', 'release', 'bundle', 'msi');
    const msis = fs.existsSync(msiDir) ? fs.readdirSync(msiDir).filter((n) => /\.msi$/i.test(n)) : [];
    if (msis.length === 0) return { msi: '未构建', installed: false };
    return { msi: msis.join(', '), installed: false, note: 'MSI 仅构建;载荷解包检查见 scripts/verify-bundle-content.mjs' };
  });
  if (ctx.uninstallAfter && ctx.installedEvidence && ctx.installedEvidence.installDir) {
    await runStep('install:uninstall', () => {
      const r = uninstallAcceptanceEntry(ctx.installedEvidence.installDir);
      return {
        uninstaller: path.basename(r.uninstaller),
        // markerLeftover 指的是**本脚本**写在安装目录里的 .acceptance-install.json(NSIS 卸载器不认识它),
        // 不是产品残留;下次认领时见到它即允许清空重装。
        ownMarkerLeftover: r.markerLeftover,
        note: 'ownMarkerLeftover = 脚本自有标识文件,非产品残留',
      };
    });
  }
  const report = {
    stage: ctx.stage,
    exe: ctx.exe,
    // 安装入口证据(装了才跑得出真实结论;失败则如实记 false 并说明后续退回构建产物)。
    installedEntry: ctx.installedEvidence || { ok: false, error: '未尝试(--install 未给出)' },
    appDataDir: ctx.appDataDir,
    mediaDir: ctx.mediaDir,
    restoreDrill: ctx.restoreDrill,
    reportTag: ctx.reportTag,
    // 卷别写进报告:同卷只覆盖 rename 路径,跨卷路径在本机未实测(见文件头说明与 Rust 测试)。
    directoryMove: {
      sourceRoot: ctx.mediaDir,
      targetRoot: ctx.targetMediaDir,
      sameVolume: ctx.sameVolume,
      crossVolumeMeasured: !ctx.sameVolume,
      note: ctx.sameVolume
        ? '本机两个根同卷,只覆盖同卷 rename 路径;跨卷暂存/发布/删源由 Rust 故障注入测试覆盖,未在真机实测。'
        : '两个根跨卷,本次运行覆盖暂存→发布→删源路径。',
    },
    // 已观察到的产品侧边界(不掩盖):本轮只握手了 3 个 externalBin(raw/ai/video),**不覆盖**
    // exotic 插件类 worker。实测隔离库日志里有 `exotic-image-psd 启动前复核未过(hash_mismatch):
    // 拒绝拉起 worker` —— 那是插件的启动前 hash 复核按设计拒绝(既不拉起、也不降级),属
    // 「可选组件供给/签名边界」,不是本轮 3 个随包 worker 的问题,也不在本轮验证范围内。
    // 本轮**未**改动注册表/校验/签名,故按 P2 供给边界记录,不声称全部特殊格式 worker 可用。
    knownBoundaries: {
      exoticPluginWorkers: {
        verified: false,
        observed: 'exotic-image-psd 启动前复核未过(hash_mismatch),拒拉 worker(设计行为)',
        scope: '可选组件;非 externalBin;本轮未验证、未改动其注册表/校验/签名',
      },
      crossVolumeDirectoryMove: { verified: !ctx.sameVolume, note: '本机单盘,跨卷路径未实测(见 directoryMove)' },
      modelDependentAiPaths: { verified: false, note: 'worker 握手不承载模型加载;推理/OCR/增强未跑' },
    },
    elapsedMs: Date.now() - t0,
    steps: results,
    passed: results.filter((r) => r.ok).length,
    failed: results.filter((r) => !r.ok).length,
  };
  // 报告名可带 --report-tag 后缀:负向对照(故意制造失败)必须另存一份,不能覆盖正式阶段报告——
  // 否则交付目录里留着一份写着 failed:1 的同名报告,而成功日志另在别处,谁看到都要误判。
  const reportPath = path.join(ctx.root, 'acceptance-report-' + ctx.stage + (ctx.reportTag ? '-' + ctx.reportTag : '') + '.json');
  fs.mkdirSync(ctx.root, { recursive: true });
  clearInterval(KEEPALIVE);
  // 阶段完整性:chain 必须真的跑到终态哨兵。缺失 = 中途异常退出(例如事件循环空转),
  // 不能因为「没有失败步骤」就判通过。
  if (ctx.stage === 'chain' && !results.some((r) => r.name === 'chain:done')) {
    report.incomplete = true;
    report.incompleteReason = '缺少终态哨兵 chain:done(实际执行 ' + results.length + ' 步)';
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2) + EOL);
    console.error('== chain 未跑完:' + report.incompleteReason + ' ==');
    console.error('报告:' + reportPath);
    const err = new Error('chain 未跑完:' + report.incompleteReason);
    err.__alreadyReported = true;
    throw err;
  }
  fs.writeFileSync(reportPath, JSON.stringify(report, null, 2) + EOL);
  // 🔴 门禁:有失败步骤就不能说「通过」,更不能 exit 0。
  // 之前的写法无论失败与否都打印「阶段通过」并走 .then(process.exit(0)),导致安装失败被静默吞掉——
  // 报告里 failed 明明非 0,退出码却是 0,调用方(人/CI)看到的是绿。
  if (report.failed > 0) {
    console.error('== ' + ctx.stage + ' 阶段失败:' + report.failed + ' 步失败 / ' + report.passed + ' 步通过 / ' + (Date.now() - t0) + 'ms ==');
    for (const s of results.filter((x) => !x.ok)) console.error('  [FAIL] ' + s.name + ': ' + (s.error || '').split(EOL)[0]);
    console.error('报告:' + reportPath);
    process.exitCode = 1;
    const err = new Error(ctx.stage + ' 阶段有 ' + report.failed + ' 步失败(见报告 ' + reportPath + ')');
    err.__alreadyReported = true;
    throw err;
  }
  console.log('== ' + ctx.stage + ' 阶段通过:' + report.passed + ' 步 / ' + (Date.now() - t0) + 'ms ==');
  console.log('报告:' + reportPath);
}

// 失败路径的统一回收:写报告 + 杀自有进程 + 卸自有安装。
// 只碰「本进程自己创建的」对象(自有 PID / 由 installDirFor 固定派生的自有安装目录),
// 不按进程名批量杀、不碰用户已装应用。
function reapOwnArtifacts(err) {
  clearInterval(KEEPALIVE);
  for (const child of OWN_CHILDREN) {
    try {
      killOurProcess(child);
    } catch (_) {}
  }
  const msg = String(err && err.message ? err.message : err);
  const stage = arg('stage', 'boot');
  const tag = arg('report-tag', null);
  const root = arg('root', path.join(REPO, 'target', 'acceptance'));
  const reportPath = path.join(root, 'acceptance-report-' + stage + (tag ? '-' + tag : '') + '.json');
  try {
    fs.mkdirSync(root, { recursive: true });
    fs.writeFileSync(
      reportPath,
      JSON.stringify(
        {
          stage,
          failedAtUtc: new Date().toISOString(),
          error: msg,
          steps: results,
          passed: results.filter((r) => r.ok).length,
          failed: results.filter((r) => !r.ok).length,
          installedDirExists: fs.existsSync(installDirFor(root)),
        },
        null,
        2
      ) + EOL
    );
    console.error('失败报告已写:' + reportPath);
  } catch (_) {}
  const dir = installDirFor(root);
  if (fs.existsSync(dir)) {
    // 🔴 只回收**真实存在的安装**。判据:除本脚本自己的标识文件外还有别的内容。
    // 2026-09-12 实测踩过:负向对照里根本没装成(缺安装包 → 安装步骤直接失败),但目录里还留着
    // 上一轮卸载后的自有标识文件,于是这里照样去调卸载器,报出「没有卸载器,需人工确认」——
    // 一条与事实相反的假告警(目录里没有任何已安装内容,本就无需回收)。
    const entries = fs.readdirSync(dir).filter((n) => n !== INSTALL_MARKER);
    if (entries.length === 0) {
      console.error('自有安装目录只剩本脚本标识文件,无安装内容,无需回收:' + dir);
    } else {
      // 有内容才走卸载;此时若仍缺卸载器,那是真的需要人工看的残留,保留告警。
      try {
        const r = uninstallAcceptanceEntry(dir);
        console.error('已回收自有安装:' + dir + '(ownMarkerLeftover=' + r.markerLeftover + ')');
      } catch (e) {
        console.error('自有安装回收失败(需人工确认):' + dir + ' — ' + (e && e.message ? e.message : e));
      }
    }
  }
}

main()
  .then(() => process.exit(0))
  .catch((err) => {
    // 阶段门禁已经打印过逐条失败并把完整报告落盘的情况:不要再补一条泛化消息、更不要覆盖报告
    // (覆盖会把 installedEntry / directoryMove 等字段抹掉,只留 stage/error/steps)。
    // 无论哪条路径失败,都要保证三件事落地:① 本次失败写进报告(覆盖上一轮的,避免交付目录里
    // 留着一份与最新失败不对应的旧报告——2026-09-12 实测踩过:08:28 失败而 JSON 还是 08:25 的);
    // ② 回收自己拉起的进程;③ 回收自有安装目录(装过才回收,且只回收自有那一份)。
    try {
      reapOwnArtifacts(err);
    } catch (_) {}
    if (err && err.__alreadyReported) {
      process.exit(1);
    }
    console.error('验收失败:' + (err && err.message ? err.message : err));
    // 多行 message 已自带上下文(逐条问题 + 修法),再补栈帧纯属重复噪音;单行才需要栈帧定位。
    if (err && err.stack && !String(err.message).includes(EOL)) {
      console.error(err.stack.split(EOL).slice(1, 4).join(EOL));
    }
    process.exit(1);
  });
