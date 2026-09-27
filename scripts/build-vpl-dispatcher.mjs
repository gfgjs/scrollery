// 只构建锁定的官方 VPL dispatcher，不安装 runtime 或驱动。
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { VPL_COMMIT, VPL_VERSION } from './lib/vpl-version.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const source = path.join(root, 'target/native-vpl/source');
const build = path.join(root, 'target/native-vpl/build');
const install = path.join(root, 'target/native-vpl/install');

function run(command, args, capture = false) {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: 'utf8',
    stdio: capture ? 'pipe' : 'inherit',
    shell: false,
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed (${result.status}): ${result.stderr ?? ''}`);
  return result.stdout?.trim() ?? '';
}

if (process.platform !== 'win32' || process.arch !== 'x64') {
  throw new Error('VPL dispatcher preparation currently targets Windows x64');
}
if (!existsSync(source)) {
  run('git', ['clone', '--depth', '1', '--branch', `v${VPL_VERSION}`, 'https://github.com/intel/libvpl.git', source]);
}
if (run('git', ['-C', source, 'rev-parse', 'HEAD'], true) !== VPL_COMMIT) {
  throw new Error('VPL source does not match the pinned official commit');
}
if (run('git', ['-C', source, 'status', '--porcelain'], true)) {
  throw new Error('VPL source has local changes; refusing an unpinned build');
}
// 优先使用 VS 自带 CMake，无需另装全局版本。
const vswhere = path.join(process.env['ProgramFiles(x86)'], 'Microsoft Visual Studio/Installer/vswhere.exe');
const cmake = run(vswhere, ['-latest', '-products', '*', '-find',
  'Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe'], true);
if (!cmake) throw new Error('Visual Studio CMake was not found');
run(cmake, ['-S', source, '-B', build, '-G', 'Visual Studio 17 2022', '-A', 'x64',
  '-DBUILD_SHARED_LIBS=OFF', '-DBUILD_TESTS=OFF', '-DBUILD_EXAMPLES=OFF',
  '-DBUILD_EXPERIMENTAL=OFF', '-DINSTALL_EXAMPLES=OFF', `-DCMAKE_INSTALL_PREFIX=${install}`]);
run(cmake, ['--build', build, '--config', 'Release', '--parallel', '2']);
run(cmake, ['--install', build, '--config', 'Release']);
console.log(`VPL ${VPL_COMMIT} installed to ${install}`);
