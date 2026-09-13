#!/usr/bin/env node
// 生成安装包内的完整第三方法律材料。
//
// NOTICE.md 只负责可审计的摘要；这里负责把实际二进制/内嵌源码对应的许可证全文
// 放进 target/legal，再由 Tauri 以目录资源映射进安装包。所有输入都在构建时硬失败，
// 避免“资源映射存在但源文件缺失”继续得到绿色结果。

import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { verifyVendoredArtifacts } from './lib/vendored-artifacts.mjs';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const packageLockPath = path.join(repo, 'package-lock.json');
const targetRoot = path.join(repo, 'target');
const legalDir = path.join(targetRoot, 'legal');
const stagingDir = path.join(targetRoot, 'legal.tmp');
const rawManifest = path.join(repo, 'crates', 'exotic-workers', 'raw-worker', 'Cargo.toml');
const vendorRoot = path.join(repo, 'third-party', 'licenses');
const licenseSourcesPath = path.join(repo, 'third-party', 'license-sources.json');

const SHIP_RUST_ROOTS = [
  { manifest: 'Cargo.toml', package: 'scrollery', target: 'x86_64-pc-windows-msvc' },
  { manifest: 'Cargo.toml', package: 'ai-worker', target: 'x86_64-pc-windows-msvc' },
  { manifest: 'Cargo.toml', package: 'psd-worker', target: 'x86_64-pc-windows-msvc' },
  { manifest: 'crates/exotic-workers/raw-worker/Cargo.toml', package: null, target: 'x86_64-pc-windows-gnu' },
];

const ORT_VERSION = '1.26.0';
const FFMPEG_SOURCE_VERSION = '2aefd64d48';
const BTBN_BUILD_REVISION = '8c736b2';

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function copyRequired(source, target) {
  if (!existsSync(source)) throw new Error(`法律材料缺失: ${path.relative(repo, source)}`);
  mkdirSync(path.dirname(target), { recursive: true });
  copyFileSync(source, target);
}

function vendorSourcePath(target) {
  const resolved = path.resolve(vendorRoot, ...target.split('/'));
  const rootPrefix = `${path.resolve(vendorRoot)}${path.sep}`;
  if (!resolved.startsWith(rootPrefix)) throw new Error(`法律材料路径越界: ${target}`);
  return resolved;
}

function loadLicenseSources() {
  if (!existsSync(licenseSourcesPath)) {
    throw new Error(`法律材料清单缺失: ${path.relative(repo, licenseSourcesPath)}(先运行 npm run vendor:licenses)`);
  }
  const manifest = JSON.parse(readFileSync(licenseSourcesPath, 'utf8'));
  if (manifest.version !== 1 || !Array.isArray(manifest.sources)) {
    throw new Error(`法律材料清单版本不受支持: ${path.relative(repo, licenseSourcesPath)}`);
  }
  if (manifest.ortVersion !== ORT_VERSION) {
    throw new Error(`法律材料清单 ORT 版本不是 ${ORT_VERSION}: ${manifest.ortVersion ?? 'missing'}`);
  }
  if (manifest.ffmpegSourceVersion !== FFMPEG_SOURCE_VERSION || manifest.btbnBuildRevision !== BTBN_BUILD_REVISION) {
    throw new Error(`法律材料清单 FFmpeg 来源版本不符: ${manifest.ffmpegSourceVersion ?? 'missing'} / ${manifest.btbnBuildRevision ?? 'missing'}`);
  }
  for (const source of manifest.sources) {
    if (!source.target || !source.sha256) throw new Error(`法律材料清单条目不完整: ${JSON.stringify(source)}`);
    const local = vendorSourcePath(source.target);
    if (!existsSync(local)) throw new Error(`已固定的法律材料缺失: ${path.relative(repo, local)}`);
    const got = sha256(readFileSync(local));
    if (got !== source.sha256) {
      throw new Error(`已固定的法律材料 hash 不符: ${path.relative(repo, local)}\n期望 ${source.sha256}\n实际 ${got}`);
    }
  }
  return manifest;
}

function copyVendoredSource(source, targetRoot) {
  copyRequired(vendorSourcePath(source.target), path.join(targetRoot, ...source.target.split('/')));
}

// license-sources.json 中按 kind 分组的已固定正文(附来源 URL 与 hash),随 legal/ 一并进包。
function pinnedTexts(manifest, kind) {
  return manifest.sources
    .filter((source) => source.kind === kind)
    .map(({ target, url, sha256: digest }) => ({ target, url, sha256: digest }));
}

function licenseText(value) {
  if (typeof value === 'string') return value;
  if (value && typeof value.type === 'string') return value.type;
  return '';
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function licenseIds(value, spdxSources) {
  const text = licenseText(value);
  return [...spdxSources.keys()]
    .sort((a, b) => b.length - a.length)
    .filter((id) => new RegExp(`(^|[^A-Za-z0-9.-])${escapeRegExp(id)}([^A-Za-z0-9.-]|$)`, 'i').test(text));
}

function findRsrawRoot() {
  const metadata = JSON.parse(execFileSync(
    'cargo',
    ['metadata', '--manifest-path', rawManifest, '--format-version', '1', '--locked'],
    { cwd: repo, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 },
  ));
  const pkg = metadata.packages.find((item) => item.name === 'rsraw-sys' && item.version === '0.1.1');
  if (!pkg) throw new Error('cargo metadata 未找到 rsraw-sys 0.1.1');
  return path.dirname(pkg.manifest_path);
}

function cargoRegistryPackages(manifest, needed) {
  const metadata = JSON.parse(execFileSync(
    'cargo',
    ['metadata', '--manifest-path', manifest, '--format-version', '1', '--locked'],
    { cwd: repo, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 },
  ));
  return metadata.packages.filter((pkg) =>
    pkg.source?.startsWith('registry+') && needed.has(`${pkg.name}@${pkg.version}`)
  );
}

function shippedCargoKeys() {
  const needed = new Set();
  for (const root of SHIP_RUST_ROOTS) {
    const args = ['tree', '--manifest-path', path.join(repo, root.manifest)];
    if (root.package) args.push('-p', root.package);
    args.push('-e', 'normal', '--locked', '--prefix', 'none', '--color', 'never', '--target', root.target, '--format', '{p}~{l}');
    const output = execFileSync('cargo', args, {
      cwd: repo,
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    });
    for (const line of output.split(/\r?\n/)) {
      const match = line.match(/^(\S+) v(\S+)(?: \([^)]*\))?~/);
      if (match && !/^[A-Za-z]:[\\/]|^\//.test(match[1])) needed.add(`${match[1]}@${match[2]}`);
    }
  }
  return needed;
}

function licenseFilesAt(root) {
  if (!existsSync(root)) return [];
  return readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isFile() && /^(license|copying|notice|copyright)(?:[._-].*)?$/i.test(entry.name))
    .map((entry) => entry.name)
    .sort();
}

function safePackageId(name, version) {
  return `${name}-${version}`.replace(/[^A-Za-z0-9._-]+/g, '_');
}

function collectPackageLicenseFiles(packages, eco, stagingRoot, spdxSources) {
  const seen = new Set();
  const copiedSpdx = new Set();
  const manifest = [];
  for (const pkg of packages) {
    const key = `${pkg.name}@${pkg.version}`;
    if (seen.has(key)) continue;
    seen.add(key);
    const packageRoot = path.dirname(pkg.manifest_path);
    const files = licenseFilesAt(packageRoot);
    const destinationRoot = path.join(stagingRoot, eco, safePackageId(pkg.name, pkg.version));
    for (const file of files) copyRequired(path.join(packageRoot, file), path.join(destinationRoot, file));
    const spdxFiles = [];
    if (files.length === 0) {
      for (const id of licenseIds(pkg.license, spdxSources)) {
        const source = spdxSources.get(id);
        const relative = `spdx/${id}.txt`;
        if (!copiedSpdx.has(id)) {
          copyRequired(source.path, path.join(stagingRoot, ...relative.split('/')));
          copiedSpdx.add(id);
        }
        spdxFiles.push(relative);
      }
    }
    manifest.push({
      eco,
      name: pkg.name,
      version: pkg.version,
      license: pkg.license ?? null,
      source: pkg.source,
      files,
      spdxFiles,
    });
  }
  return manifest;
}

function npmPackages(lock) {
  const packages = [];
  for (const [key, entry] of Object.entries(lock.packages || {})) {
    if (key === '' || !key.includes('node_modules/')) continue;
    const name = key.slice(key.lastIndexOf('node_modules/') + 'node_modules/'.length);
    if (entry.dev && name !== 'onnxruntime-node') continue;
    const packageJsonPath = path.join(repo, key, 'package.json');
    if (!existsSync(packageJsonPath)) continue;
    const packageJson = JSON.parse(readFileSync(packageJsonPath, 'utf8'));
    packages.push({
      name,
      version: entry.version,
      license: entry.license ?? packageJson.license ?? null,
      source: entry.resolved ?? null,
      manifest_path: packageJsonPath,
    });
  }
  return packages;
}

async function main() {
  const lock = JSON.parse(readFileSync(packageLockPath, 'utf8'));
  const ort = lock.packages?.['node_modules/onnxruntime-node'];
  if (!ort || ort.version !== ORT_VERSION) {
    throw new Error(`onnxruntime-node 版本未钉定到 ${ORT_VERSION}，请同步审查 URL/hash 后再构建`);
  }
  const sourceManifest = loadLicenseSources();
  // vendored 编译件的版本/字节校验(唯一实现):legal/ 里的 Graphviz/Lute 正文须指向确切的产物版本。
  verifyVendoredArtifacts(sourceManifest, repo);
  const spdxSources = new Map(
    sourceManifest.sources
      .filter((source) => source.kind === 'spdx' && source.licenseId)
      .map((source) => [source.licenseId, { ...source, path: vendorSourcePath(source.target) }]),
  );

  rmSync(stagingDir, { recursive: true, force: true });
  mkdirSync(stagingDir, { recursive: true });
  try {
    copyRequired(path.join(repo, 'src', 'vendor', 'foliate-js', 'LICENSE'), path.join(stagingDir, 'foliate-js', 'LICENSE'));

    const rsrawRoot = findRsrawRoot();
    for (const name of ['COPYRIGHT', 'LICENSE.CDDL', 'LICENSE.LGPL']) {
      copyRequired(path.join(rsrawRoot, 'LibRaw', name), path.join(stagingDir, 'LibRaw', name));
    }

    for (const source of sourceManifest.sources) copyVendoredSource(source, stagingDir);

    const packageLicenseRoot = path.join(stagingDir, 'package-licenses');
    const cargoKeys = shippedCargoKeys();
    const cargoPackages = [
      ...cargoRegistryPackages(path.join(repo, 'Cargo.toml'), cargoKeys),
      ...cargoRegistryPackages(rawManifest, cargoKeys),
    ];
    const cargoManifest = collectPackageLicenseFiles(cargoPackages, 'cargo', packageLicenseRoot, spdxSources);
    const npmManifest = collectPackageLicenseFiles(npmPackages(lock), 'npm', packageLicenseRoot, spdxSources);
    const packageManifest = [...cargoManifest, ...npmManifest];
    const unresolved = packageManifest.filter((item) => item.files.length === 0 && item.spdxFiles.length === 0);
    if (unresolved.length) {
      throw new Error(`实际发货包缺少可捆绑的许可证正文(${unresolved.length}): ${unresolved.map((item) => `${item.eco}:${item.name}@${item.version}(${item.license ?? 'unknown'})`).join(', ')}`);
    }

    writeFileSync(path.join(stagingDir, 'MANIFEST.json'), JSON.stringify({
      version: 1,
      sources: {
        foliateJs: 'src/vendor/foliate-js/LICENSE',
        libRaw: 'rsraw-sys@0.1.1/LibRaw/{COPYRIGHT,LICENSE.CDDL,LICENSE.LGPL}',
        // 已固定正文按 kind 分组收录:onnxruntime/ffmpeg 属运行时侧,graphviz/lute 属前端
        // vendored 编译件(graphviz 含 Graphviz 2.40.1 与 Viz.js 2.1.2 两份正文)。
        ...Object.fromEntries(
          ['onnxruntime', 'ffmpeg', 'graphviz', 'lute'].map((kind) => [
            kind,
            pinnedTexts(sourceManifest, kind),
          ]),
        ),
        pinnedManifest: 'third-party/license-sources.json',
      },
      packageLicenseFiles: packageManifest,
    }, null, 2) + '\n');

    rmSync(legalDir, { recursive: true, force: true });
    renameSync(stagingDir, legalDir);
    const files = packageManifest.reduce((sum, item) => sum + item.files.length, 0);
    const canonical = new Set(packageManifest.flatMap((item) => item.spdxFiles)).size;
    console.log(`✓ legal resources ready:${path.relative(repo, legalDir)}(package license files ${files}; pinned SPDX texts ${canonical}; unresolved ${unresolved.length})`);
  } catch (error) {
    rmSync(stagingDir, { recursive: true, force: true });
    throw error;
  }
}

main().catch((error) => {
  console.error(`[prepare-legal-resources] ${error.message}`);
  process.exit(1);
});
