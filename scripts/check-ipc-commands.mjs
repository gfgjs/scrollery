#!/usr/bin/env node
// 命令注册表是唯一真源；源码门禁不依赖可能陈旧的 target 缓存。
// 用法：node scripts/check-ipc-commands.mjs [--acl <本次构建的 app-commands.toml>]
// 自检：node scripts/check-ipc-commands.mjs --selftest
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = (path) => readFileSync(path, 'utf8');

function unique(values, label) {
  const result = new Set();
  for (const value of values) {
    if (result.has(value)) throw new Error(`${label} 重复：${value}`);
    result.add(value);
  }
  if (!result.size) throw new Error(`${label} 为空，无法对拍`);
  return result;
}

// 当前 Rust 清单采用逐行属性和函数声明；忽略文档中的宏示例，陌生声明形式直接报错。
function commandDefinitions(source, modulePath) {
  const definitions = [];
  const attributes = source.matchAll(/^\s*#\[tauri::command(?:\([^\n]*\))?\]/gm);
  for (const attribute of attributes) {
    const tail = source.slice(attribute.index + attribute[0].length);
    const declaration = /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)\s*\(/.exec(tail);
    if (!declaration) throw new Error(`${modulePath}：无法解析 command 后的函数声明`);
    definitions.push(declaration[1]);
  }
  return definitions;
}

function registeredCommands(source) {
  const cleaned = source.replace(/\/\/[^\n]*/g, '');
  const body = /generate_handler!\[([\s\S]*?)\]/.exec(cleaned)?.[1];
  if (body === undefined) throw new Error('未找到 generate_handler! 清单');
  return unique(body.split('\n').map((line) => line.trim()).filter(Boolean).map((line) => {
    if (!/^ipc::\w+(?:::\w+)+,$/.test(line)) {
      throw new Error(`无法解析注册条目：${line}`);
    }
    return line.slice(0, -1);
  }), '注册路径');
}

function frontendCommands(source) {
  // 通过语法树只读 IPC 对象，避免把注释或 EVENTS 当作命令。
  const file = ts.createSourceFile('ipc.ts', source, ts.ScriptTarget.Latest, true);
  const declaration = file.statements.filter(ts.isVariableStatement)
    .flatMap((statement) => [...statement.declarationList.declarations])
    .find((item) => ts.isIdentifier(item.name) && item.name.text === 'IPC');
  const initializer = declaration?.initializer;
  const object = initializer && ts.isAsExpression(initializer) ? initializer.expression : initializer;
  if (!object || !ts.isObjectLiteralExpression(object)) throw new Error('无法解析 IPC 对象');
  return unique(object.properties.map((property) => {
    if (!ts.isPropertyAssignment(property) || !ts.isStringLiteral(property.initializer)) {
      throw new Error(`IPC 必须使用显式字符串常量：${property.getText(file)}`);
    }
    return property.initializer.text;
  }), '前端命令');
}

function compare(expected, actual, label) {
  const missing = [...expected].filter((value) => !actual.has(value));
  const extra = [...actual].filter((value) => !expected.has(value));
  if (missing.length || extra.length) {
    throw new Error(`${label} 差集非零；缺少：[${missing.join(', ')}]；多余：[${extra.join(', ')}]`);
  }
}

function rustDefinitions(directory, sourceRoot = directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return rustDefinitions(path, sourceRoot);
    if (!entry.isFile() || !path.endsWith('.rs')) return [];
    const modulePath = relative(sourceRoot, path).replace(/\\/g, '/').replace(/\.rs$/, '')
      .replace(/\/mod$/, '').replaceAll('/', '::');
    return commandDefinitions(read(path), modulePath);
  });
}

function selftest() {
  const definition = '#[tauri::command]\npub async fn ping() {}';
  assert.deepEqual(commandDefinitions(`//! #[tauri::command]\n${definition}`, 'ipc::test'), ['ping']);
  assert.throws(() => commandDefinitions('#[tauri::command]\nunknown!();', 'ipc::test'), /无法解析/);
  const registered = registeredCommands('tauri::generate_handler![\n// ] 注释不应截断清单\nipc::test::ping,\n]');
  const names = unique([...registered].map((path) => path.split('::').at(-1)), '注册命令名');
  compare(names, unique(commandDefinitions(definition, 'ipc::test'), '定义'), '函数');
  assert.throws(() => registeredCommands('generate_handler![\nipc::test::ping,\nipc::test::ping,\n]'), /重复/);
  assert.throws(() => registeredCommands('generate_handler![\nunknown!(),\n]'), /无法解析/);
  compare(names, frontendCommands("export const IPC = { PING: 'ping' } as const; export const EVENTS = { DONE: 'done' } as const"), '前端');
  assert.throws(() => compare(names, frontendCommands("export const IPC = { PING: 'ping', EVENT: 'done' } as const"), '前端'), /多余：\[done\]/);
  assert.throws(() => compare(names, new Set(['wrong']), '前端'), /缺少：\[ping\]/);
  assert.throws(() => compare(names, new Set(['ping', 'dead']), '函数'), /多余：\[dead\]/);
  // 命令可从兼容模块重导出；路径是否合法由 Rust 编译器检查，门禁比较实际暴露的名字。
  compare(names, unique(commandDefinitions(definition, 'ipc::implementation'), '定义'), '重导出');
  assert.throws(() => frontendCommands("export const IPC = { A: 'ping', B: 'ping' } as const"), /重复/);
  console.log('✓ IPC 门禁自检通过（注释、死命令、缺失、重复、重导出与事件混入）');
}

try {
  const args = process.argv.slice(2);
  if (args.length === 1 && args[0] === '--selftest') {
    selftest();
  } else {
    if (args.length && !(args.length === 2 && args[0] === '--acl')) {
      throw new Error('用法：check-ipc-commands.mjs [--selftest | --acl <app-commands.toml>]');
    }
    const registered = registeredCommands(read(join(root, 'src-tauri/src/ipc/registry.rs')));
    const names = unique([...registered].map((path) => path.split('::').at(-1)), '注册命令名');
    const definitions = unique(rustDefinitions(join(root, 'src-tauri/src')), '命令定义');
    const frontend = frontendCommands(read(join(root, 'src/constants/ipc.ts')));
    compare(names, definitions, '命令定义与注册命令');
    compare(names, frontend, '前端与注册命令');
    if (args[0] === '--acl') {
      // 只接受明确指定的构建产物，不猜测多个 OUT_DIR 中哪份仍有效。
      const allow = /commands\.allow\s*=\s*\[([\s\S]*?)\]/.exec(read(resolve(args[1])))?.[1];
      if (allow === undefined) throw new Error('ACL 缺少 commands.allow');
      compare(names, unique([...allow.matchAll(/"([^"]+)"/g)].map((match) => match[1]), 'ACL 命令'), 'ACL 与注册命令');
    }
    console.log(`✓ IPC 一致性通过：定义 ${definitions.size} / 注册 ${names.size} / 前端 ${frontend.size}${args.length ? ' / ACL ' + names.size : ''}，差集均为 0`);
  }
} catch (error) {
  console.error(`✗ IPC 门禁失败：${error.message}`);
  process.exitCode = 1;
}
