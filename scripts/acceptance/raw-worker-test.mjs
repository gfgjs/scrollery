#!/usr/bin/env node
// raw-worker 独立验证(P1-2,2026-09-12)。
//
// why 需要独立入口:raw-worker 刻意不加入根 workspace(rsraw 的 build.rs 在 MSVC 下硬 panic),
// 只能用 --target x86_64-pc-windows-gnu 单独编译。于是它天然游离在 `cargo test --workspace` 之外——
// 谁都不会碰它的测试,回归只能靠人记得手跑。本脚本把这条验证固化成一条命令(CI job 直调)。
//
// 检查项:
//   ① 结构守卫:raw-worker 必须仍在根 workspace 之外(根 Cargo.toml 不列它为 member,其 Cargo.toml
//      留空 [workspace] 表)——回流会直接把整个 workspace 拖进 MSVC panic;
//   ② 结构守卫:root lockfile 不得收录 raw-worker(它有自己的 Cargo.lock);
//   ③ cargo fmt --check(cwd = raw-worker crate);
//   ④ cargo test --locked --release --target x86_64-pc-windows-gnu(真实编译+跑单测);
//   ⑤ 可分发产物(可选 --expect-binary):**先显式 cargo build --locked --release --target gnu**,
//      再验 target/<gnu>/release/raw-worker.exe。🔴 不能只靠 cargo test 顺带产物:cargo test 编的是
//      deps/ 下的测试 harness,普通二进制不会因此在冷 CI 上凭空出现——本机曾误把 9MB 旧 build 当
//      「测试顺带产出」的证据,冷 runner 立刻会缺(见 --cold-proof)。
//
// 用法:node scripts/acceptance/raw-worker-test.mjs [--expect-binary] [--skip-structural] [--cold-proof]
//      [--selftest]
//   --cold-proof  受控证明「测试不自产普通二进制」:先把本 crate 自己 target 下那个确切产物移到同目录
//                 暂存名(先过范围守卫,只认这一个绝对路径),只跑 cargo test 断言它仍未出现,再跑
//                 cargo build 断言它被重新产出;任一步异常都会把暂存产物放回原位。
// 退出码:全部通过 = 0;任一失败 = 1;selftest 自身损坏 = 2。
// 说明:本脚本只读写 crates/exotic-workers/raw-worker/** 与只读根 Cargo.toml/Cargo.lock,
// 不碰用户数据、不安装任何东西。

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const CRATE_DIR = path.join(REPO, 'crates', 'exotic-workers', 'raw-worker');
const GNU_TARGET = 'x86_64-pc-windows-gnu';
const EOL = '\n';

function hasFlag(name) {
  return process.argv.slice(2).includes('--' + name);
}

function run(cmd, args, cwd, label) {
  const r = spawnSync(cmd, args, { cwd, encoding: 'utf8', shell: false, maxBuffer: 32 * 1024 * 1024 });
  const output = (r.stdout || '') + (r.stderr || '');
  if (r.status !== 0) {
    const err = new Error(label + ' 失败(exit=' + r.status + ')' + EOL + output.trim().slice(-2000));
    err.output = output;
    throw err;
  }
  return output;
}

// ── 结构守卫(纯函数,供 selftest 喂样本)──────────────────────────────────────────

// 可分发产物的确切位置:本 crate 自己 target 下、gnu target、release、raw-worker.exe。
export function artifactPath(crateDir = CRATE_DIR) {
  return path.join(crateDir, 'target', GNU_TARGET, 'release', 'raw-worker.exe');
}

// 范围守卫:**只认这一个绝对路径**。冷证明要动盘上的产物,先证明「动的就是自己这份」——
// 误移/误删他处同名文件(根 target、deps 测试 harness、别的 crate)必须先被拦下。
export function assertArtifactScope(binPath, crateDir = CRATE_DIR) {
  const abs = path.resolve(binPath);
  const expected = path.resolve(artifactPath(crateDir));
  if (abs.toLowerCase() !== expected.toLowerCase()) {
    throw new Error('拒绝操作非本 crate 产物:' + abs + EOL + '  只允许:' + expected);
  }
  return abs;
}

// 根 workspace members 不允许出现 raw-worker(名字/路径任一形式)。
export function rootWorkspaceViolations(rootCargoToml) {
  const violations = [];
  const section = /\[workspace\]([\s\S]*?)(?=\n\[|$)/.exec(rootCargoToml);
  if (!section) {
    violations.push('根 Cargo.toml 无 [workspace] 段');
    return violations;
  }
  // 只看 members 列表本身:段外/段内注释里提到 raw-worker(比如解释它为何独立)不算违规。
  const members = /members\s*=\s*\[([\s\S]*?)\]/.exec(section[1]);
  if (members && /raw-worker/.test(members[1])) {
    violations.push('根 workspace members 收录 raw-worker(回流即触发 MSVC panic)');
  }
  return violations;
}

// 独立 crate 必须留空 [workspace] 表把自己摘出去,且不得被根 lockfile 收录。
export function crateIsolationViolations(crateCargoToml, rootLock) {
  const violations = [];
  if (!/^\[workspace\]\s*$/m.test(crateCargoToml)) {
    violations.push('raw-worker/Cargo.toml 缺空 [workspace] 表(会重新并入父 workspace)');
  }
  if (rootLock && /name = "raw-worker"/.test(rootLock)) {
    violations.push('根 Cargo.lock 收录了 raw-worker(独立 crate 应由自身 Cargo.lock 锁定)');
  }
  return violations;
}

function selftest() {
  const fails = [];
  const expect = (cond, msg) => {
    if (!cond) fails.push(msg);
  };
  const badRoot = '[workspace]\nmembers = ["src-tauri", "crates/exotic-workers/raw-worker"]\n';
  const goodRoot = '[workspace]\nmembers = ["src-tauri", "crates/exotic-workers/exotic-protocol"]\n';
  expect(rootWorkspaceViolations(badRoot).length === 1, '结构守卫漏检根 workspace 收录 raw-worker');
  expect(rootWorkspaceViolations(goodRoot).length === 0, '结构守卫对干净根 workspace 误报');
  // members 列表之外的提及(注释)不算违规——守卫只看 members 列表。
  expect(rootWorkspaceViolations('[workspace]\n# raw-worker 刻意独立\nmembers = ["src-tauri"]\n').length === 0, '结构守卫对注释提及误报');
  expect(rootWorkspaceViolations('[package]\nname = "x"\n').length === 1, '结构守卫漏检缺失 [workspace] 段');
  expect(crateIsolationViolations('[workspace]\n[package]\nname = "raw-worker"\n', '').length === 0, '结构守卫对合规 crate 误报');
  expect(crateIsolationViolations('[package]\nname = "raw-worker"\n', '').length === 1, '结构守卫漏检缺失空 workspace 表');
  expect(crateIsolationViolations('[workspace]\n', 'name = "raw-worker"\nversion = "0.1.0"').length === 1, '结构守卫漏检根 lockfile 收录');
  // 产物范围守卫:只认本 crate 那一个确切路径,其余形态一律拒绝。
  expect(assertArtifactScope(artifactPath()) === artifactPath(), '范围守卫误拦本 crate 产物');
  for (const bad of [
    path.join(REPO, 'target', 'release', 'raw-worker.exe'),
    path.join(CRATE_DIR, 'target', GNU_TARGET, 'release', 'deps', 'raw_worker-abc.exe'),
    path.join(CRATE_DIR, 'target', GNU_TARGET, 'debug', 'raw-worker.exe'),
    path.join(CRATE_DIR, 'target', 'release', 'raw-worker.exe'),
    path.join(REPO, 'crates', 'exotic-workers', 'exotic-protocol', 'target', GNU_TARGET, 'release', 'raw-worker.exe'),
  ]) {
    let caught = false;
    try {
      assertArtifactScope(bad);
    } catch (_) {
      caught = true;
    }
    expect(caught, '范围守卫未拦下非本 crate 产物:' + bad);
  }
  if (fails.length) {
    console.error('selftest 失败:');
    for (const f of fails) console.error('  - ' + f);
    process.exit(2);
  }
  console.log('✓ selftest 通过(结构守卫正反样本)');
}

// ── 冷证明:普通二进制只由 cargo build 产出 ────────────────────────────────────
// 机器上只要留着旧产物,「测试顺带产出二进制」这个错觉就永远验证不到(旧 build 顶着用)。故受控地
// 把产物移开:范围守卫只认本 crate 那一个绝对路径 → 移到同目录暂存名 → 只跑 cargo test,断言它
// 仍未出现 → 再跑 cargo build,断言它回归。任何异常都把暂存产物放回原位,不让机器比进来时更差。
function coldProof() {
  const bin = assertArtifactScope(artifactPath());
  const holdout = bin + '.holdout-cold-proof';
  const rel = (p) => path.relative(REPO, p);
  let moved = false;
  try {
    if (fs.existsSync(holdout)) throw new Error('暂存路径已存在,先人工确认:' + holdout);
    if (fs.existsSync(bin)) {
      fs.renameSync(bin, holdout);
      moved = true;
      console.log('· 已把本 crate 产物移开:' + rel(holdout));
    } else {
      console.log('· 本 crate 产物本就不存在(冷机),无需移开');
    }
    run('cargo', ['test', '--locked', '--release', '--target', GNU_TARGET], CRATE_DIR, 'cargo test');
    if (fs.existsSync(bin)) {
      throw new Error('cargo test 竟产出了普通二进制,本证明的前提不再成立:' + bin);
    }
    console.log('✓ cargo test 未产出普通二进制(测试 harness 只落 deps/;旧检查在冷 CI 必缺)');
    run('cargo', ['build', '--locked', '--release', '--target', GNU_TARGET], CRATE_DIR, 'cargo build');
    if (!fs.existsSync(bin)) throw new Error('cargo build 后产物仍缺失:' + bin);
    console.log('✓ cargo build 重新产出普通二进制(' + fs.statSync(bin).size + ' 字节)');
  } finally {
    if (moved) {
      if (fs.existsSync(bin)) {
        // 重建成功 → 暂存的旧产物已是冗余副本;只删自己刚移走的那一个确切文件。
        fs.rmSync(holdout, { force: true });
        console.log('· 已清理暂存副本(重建产物就位):' + rel(holdout));
      } else {
        fs.renameSync(holdout, bin);
        console.log('· 已放回原产物(重建未成功):' + rel(bin));
      }
    }
  }
}

function main() {
  selftest();
  if (hasFlag('selftest')) return;
  if (process.platform !== 'win32') {
    console.log('[raw-worker-test] 非 Windows:raw-worker 是 gnu-only 的 Windows sidecar,跳过。');
    return;
  }
  if (!fs.existsSync(CRATE_DIR)) throw new Error('raw-worker crate 不存在:' + CRATE_DIR);

  if (!hasFlag('skip-structural')) {
    const rootCargo = fs.readFileSync(path.join(REPO, 'Cargo.toml'), 'utf8');
    const crateCargo = fs.readFileSync(path.join(CRATE_DIR, 'Cargo.toml'), 'utf8');
    const rootLockPath = path.join(REPO, 'Cargo.lock');
    const rootLock = fs.existsSync(rootLockPath) ? fs.readFileSync(rootLockPath, 'utf8') : '';
    const violations = [...rootWorkspaceViolations(rootCargo), ...crateIsolationViolations(crateCargo, rootLock)];
    if (violations.length) {
      console.error('✗ 结构守卫失败:');
      for (const v of violations) console.error('  - ' + v);
      process.exit(1);
    }
    console.log('✓ 结构守卫:raw-worker 仍在根 workspace 之外(空 [workspace] 表 + 未进根 lockfile)');
  }

  // target 缺失时给出可执行指引,而不是让 cargo 报一段难读的 linker 错误。
  const targets = run('rustup', ['target', 'list', '--installed'], REPO, 'rustup target list');
  if (!targets.split(/\r?\n/).some((l) => l.trim() === GNU_TARGET)) {
    throw new Error('缺少 target ' + GNU_TARGET + ':先跑 rustup target add ' + GNU_TARGET + ',并确保 mingw-w64(dlltool/g++)在 PATH');
  }

  run('cargo', ['fmt', '--', '--check'], CRATE_DIR, 'cargo fmt --check');
  console.log('✓ cargo fmt --check(仅 raw-worker crate)');

  // 冷证明是独立取证模式:自己跑 test + build,跑完即结束(不与下面的常规路径重复跑)。
  if (hasFlag('cold-proof')) {
    coldProof();
    console.log('== 冷证明通过:普通二进制只由 cargo build 产出 ==');
    return;
  }

  const out = run('cargo', ['test', '--locked', '--release', '--target', GNU_TARGET], CRATE_DIR, 'cargo test');
  const summary = out.split(/\r?\n/).filter((l) => /^test result:/.test(l.trim()) || /Running unittests/.test(l));
  console.log('✓ cargo test --locked --release --target ' + GNU_TARGET);
  for (const line of summary) console.log('  ' + line.trim());

  if (hasFlag('expect-binary')) {
    const bin = assertArtifactScope(artifactPath());
    // 显式产出可分发二进制:cargo test 只编 deps/ 下的测试 harness,不刷新 release/raw-worker.exe。
    // 少了这一步,热机可能拿旧产物假绿(旧产物与当前源码未必同源),冷 CI 则直接缺文件。
    run('cargo', ['build', '--locked', '--release', '--target', GNU_TARGET], CRATE_DIR, 'cargo build');
    console.log('✓ cargo build --locked --release --target ' + GNU_TARGET);
    if (!fs.existsSync(bin)) throw new Error('cargo build 后产物仍缺失:' + bin);
    const stat = fs.statSync(bin);
    const head = fs.readFileSync(bin, { flag: 'r' }).subarray(0, 2).toString('latin1');
    if (head !== 'MZ') throw new Error('产物不是 PE 可执行文件(头两字节 ' + JSON.stringify(head) + '):' + bin);
    console.log('✓ 可分发产物:' + path.relative(REPO, bin) + '(' + stat.size + ' 字节)');
  }
  console.log('== raw-worker 独立验证通过 ==');
}

try {
  main();
} catch (err) {
  console.error('✗ ' + (err && err.message ? err.message : err));
  process.exit(1);
}
