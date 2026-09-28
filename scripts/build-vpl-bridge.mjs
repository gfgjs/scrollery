// 编译本项目薄桥；开发入口允许按需准备锁定的官方 dispatcher。
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { VPL_COMMIT } from './lib/vpl-version.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const source = path.join(root, 'target/native-vpl/source');
const install = path.join(root, 'target/native-vpl/install');
const build = path.join(root, 'target/native-vpl/bridge');
const smoke = process.argv.includes('--smoke');
function run(command, args, capture = false) {
  const result = spawnSync(command, args, {
    cwd: root, encoding: 'utf8', stdio: capture ? 'pipe' : 'inherit',
    shell: false, windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed (${result.status}): ${result.stderr ?? ''}`);
  return result.stdout?.trim() ?? '';
}
if (process.platform !== 'win32' || process.arch !== 'x64') {
  throw new Error('VPL bridge currently targets Windows x64');
}
// target 可被清理，开发启动时补齐源码或静态库，已有依赖直接复用。
if (process.argv.includes('--dev') &&
    (!existsSync(source) || !existsSync(path.join(install, 'lib/vpl.lib')))) {
  console.log('[VPL] 开发依赖缺失，正在准备锁定的 dispatcher（首次需要下载并编译）…');
  run(process.execPath, [path.join(root, 'scripts/build-vpl-dispatcher.mjs')]);
}
if (!existsSync(source)) {
  throw new Error('VPL dispatcher source is missing. Run node scripts/build-vpl-dispatcher.mjs from the repository root, then retry.');
}
if (run('git', ['-C', source, 'rev-parse', 'HEAD'], true) !== VPL_COMMIT) {
  throw new Error('Prepare the pinned dispatcher with node scripts/build-vpl-dispatcher.mjs first');
}
const vswhere = path.join(process.env['ProgramFiles(x86)'], 'Microsoft Visual Studio/Installer/vswhere.exe');
const cmake = run(vswhere, ['-latest', '-products', '*', '-find',
  'Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe'], true);
if (!cmake) throw new Error('Visual Studio CMake was not found');
run(cmake, ['-S', path.join(root, 'src-tauri/native/vpl'), '-B', build,
  '-G', 'Visual Studio 17 2022', '-A', 'x64', `-DCMAKE_PREFIX_PATH=${install}`,
  `-DCMAKE_INSTALL_PREFIX=${install}`, `-DSCROLLERY_VPL_SMOKE=${smoke ? 'ON' : 'OFF'}`]);
run(cmake, ['--build', build, '--config', 'Release', '--parallel', '2']);
run(cmake, ['--install', build, '--config', 'Release']);
if (smoke) run(path.join(build, 'Release/scrollery_vpl_smoke.exe'), []);
