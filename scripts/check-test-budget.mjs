#!/usr/bin/env node
// 全仓预算按实际定义收集；平台、feature 与 ignored 取并集，执行次数不重复计数。
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'
import { testCase } from './lib/test-case.mjs'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const normalized = (file) => relative(root, file).replaceAll('\\', '/')
const entry = (pool, file, name) => ({ id: `${pool}:${file}::${name}`, pool, file, name })

function enforceBudget(cases, policy) {
  if (!cases.length) throw new Error('未收集到测试')
  if (policy.baseline !== 300 || !Number.isSafeInteger(policy.limit) || policy.limit < 1) throw new Error('预算规则必须保持基准300及有效核定额度')
  if (cases.length > policy.limit) throw new Error(`全仓 ${cases.length} 超出核定额度 ${policy.limit}`)
  const ids = new Set(cases.map((c) => c.id))
  if (ids.size !== cases.length) throw new Error('测试身份重复')
  const approved = new Map(policy.cases.map((c) => [c.id, c]))
  if (approved.size !== policy.cases.length) throw new Error('核定清单身份重复')
  for (const c of cases) if (!approved.get(c.id)?.reason?.trim()) throw new Error(`测试缺少取舍依据: ${c.id}`)
  // 公开投影会按既有规则排除技能安装脚本；只按实际存活项核算，缺席项不能提供增额。
  const extensions = policy.cases.filter((c) => ids.has(c.id) && c.extensionReason?.trim()).length
  if (cases.length > policy.baseline + extensions) throw new Error('超过300的额度缺少逐项增额依据')
  return cases.length
}

async function collectVitest(options = {}) {
  const { createVitest } = await import('vitest/node')
  const ctx = await createVitest('test', { root, watch: false, maxWorkers: 2, allowOnly: false, reporters: [], ...options })
  try {
    const { testModules, unhandledErrors } = await ctx.collect()
    const errors = [...unhandledErrors]
    for (const module of testModules) {
      errors.push(...module.errors())
      for (const suite of module.children.allSuites()) errors.push(...suite.errors())
    }
    if (errors.length) throw new Error(`测试收集失败: ${errors.map((e) => e?.message ?? String(e)).join('; ')}`)
    return testModules.flatMap((module) => [...module.children.allTests()].map((test) => entry('vitest', normalized(module.moduleId), test.fullName)))
  } finally { await ctx.close() }
}

function sourceFiles() {
  return execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard', '-z', '--', 'src', 'src-tauri', 'crates', 'scripts', 'tools', '.github'], { cwd: root, encoding: 'utf8' })
    .split('\0').filter((file) => file && existsSync(join(root, file)) && !file.split('/').some((part) => ['target', 'node_modules', 'vendor', 'third_party'].includes(part)))
}

// 注释和字面量不产生 Rust 声明；保留偏移以便读取原文中的基准名称。
function maskRust(source) {
  let out = '', i = 0
  const blank = (text) => text.replace(/[^\r\n]/g, ' ')
  while (i < source.length) {
    const start = i
    if (source.startsWith('//', i)) { i = source.indexOf('\n', i); if (i < 0) i = source.length }
    else if (source.startsWith('/*', i)) {
      i += 2; let depth = 1
      while (i < source.length && depth) {
        if (source.startsWith('/*', i)) { depth++; i += 2 }
        else if (source.startsWith('*/', i)) { depth--; i += 2 }
        else i++
      }
    } else {
      const raw = /^(?:br|cr|r)(#*)"/.exec(source.slice(i))
      if (raw) { const end = source.indexOf('"' + raw[1], i + raw[0].length); i = end < 0 ? source.length : end + 1 + raw[1].length }
      else if (source[i] === '"') {
        i++
        while (i < source.length) { if (source[i] === '\\') i += 2; else if (source[i++] === '"') break }
      } else if (source[i] === "'" && /^'(?:\\.|[^'\\\r\n])'/u.test(source.slice(i))) i += /^'(?:\\.|[^'\\\r\n])'/u.exec(source.slice(i))[0].length
      else { out += source[i++]; continue }
    }
    out += blank(source.slice(start, i))
  }
  return out
}

function collectRust(file, source) {
  const masked = maskRust(source)
  if (/#\[\s*(?:rstest|test_case|proptest|quickcheck)\b/.test(masked)) throw new Error(`需要展开新的Rust测试宏: ${file}`)
  const cases = [...masked.matchAll(/#\[\s*(?:(?:tokio|async_std)::)?test\s*(?:\([^\]]*\))?\s*\][\s\S]*?\bfn\s+(\w+)/g)].map((m) => entry('rust', file, m[1]))
  for (const m of masked.matchAll(/\.bench_function\s*\(/g)) {
    const name = /^\s*"([^"]+)"/.exec(source.slice(m.index + m[0].length))
    if (!name) throw new Error(`需要展开参数化基准: ${file}`)
    cases.push(entry('benchmark', file, name[1]))
  }
  if (/\.bench_with_input\s*\(/.test(masked)) throw new Error(`需要展开新基准: ${file}`)
  // 此按需验收不使用Rust测试属性，仍以实际函数定义纳入全仓预算。
  if (file === 'src-tauri/examples/video_bench.rs' && /\bfn\s+rotation_assert\s*\(/.test(masked)) cases.push(entry('script', file, 'rotation-check'))
  return cases
}

function collectScripts(file, source) {
  const ast = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS)
  const calls = [], variables = new Map()
  function visit(node) {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer) variables.set(node.name.text, node.initializer)
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && ['testCase', 'runStep'].includes(node.expression.text)) {
      const name = node.arguments[0]
      if (!name || !ts.isStringLiteralLike(name)) throw new Error(`测试注册名称必须可静态展开: ${file}`)
      for (let parent = node.parent; parent; parent = parent.parent) {
        if (ts.isForStatement(parent) || ts.isForOfStatement(parent) || ts.isForInStatement(parent) || ts.isWhileStatement(parent)) throw new Error(`循环注册需要显式展开: ${file}:${name.text}`)
      }
      calls.push({ name: name.text, node })
    }
    ts.forEachChild(node, visit)
  }
  visit(ast)
  const elements = (node) => {
    if (!node || !ts.isArrayLiteralExpression(node)) throw new Error(`无法展开测试数组: ${file}`)
    return node.elements
  }
  return calls.flatMap(({ name, node }) => {
    if (node.expression.text === 'runStep' && ['chain:done', 'enhance:done', 'install:msi-not-installed'].includes(name)) return []
    let names = [name]
    if (name === 'workers:handshake') {
      names = elements(variables.get('EXPECTED_WORKERS')).map((e) => {
        const id = e.properties?.find((p) => p.name?.getText(ast) === 'id')?.initializer
        if (!id || !ts.isStringLiteralLike(id)) throw new Error('worker身份无法展开')
        return name + '/' + id.text
      })
    } else if (name === 'chain:new-command-acl') {
      const commands = []
      function ipc(n) {
        if (ts.isVariableDeclaration(n) && n.name.getText(ast) === 'commands') {
          for (const command of elements(n.initializer)) {
            if (!ts.isStringLiteralLike(command)) throw new Error('ACL命令无法展开')
            commands.push(command.text)
          }
        }
        if (ts.isCallExpression(n) && n.expression.getText(ast) === 'ipc' && ts.isStringLiteralLike(n.arguments[1])) commands.push(n.arguments[1].text)
        ts.forEachChild(n, ipc)
      }
      ipc(node.arguments[1]); names = commands.map((command) => name + '/' + command)
    } else if (['enhance:manifest-unready', 'enhance:worker-missing'].includes(name)) {
      names = elements(variables.get('attempts')).map((e) => {
        const command = elements(e)[0]
        if (!ts.isStringLiteralLike(command)) throw new Error('增强命令无法展开')
        return name + '/' + command.text
      })
    }
    if (!names.length) throw new Error(`测试展开为空: ${file}:${name}`)
    return names.map((name) => entry('script', file, name))
  })
}

async function collectAll() {
  const cases = await collectVitest()
  for (const file of sourceFiles()) {
    if (file.endsWith('.rs')) cases.push(...collectRust(file, readFileSync(join(root, file), 'utf8')))
    else if (file === 'scripts/test-portable-execution.ps1') {
      for (const m of readFileSync(join(root, file), 'utf8').matchAll(/@'\r?\n([\s\S]*?)\r?\n'@/g)) cases.push(...collectRust(file, m[1]))
    } else if (/^scripts\/.*\.(mjs|ts)$/.test(file) && !file.includes('.spec.')) cases.push(...collectScripts(file, readFileSync(join(root, file), 'utf8')))
    else if (/^\.github\/workflows\/.*\.yml$/.test(file) && /^\s*PICASA_SMOKE_TEST=1\s/m.test(readFileSync(join(root, file), 'utf8'))) cases.push(entry('script', file, 'startup-smoke'))
  }
  return cases.sort((a, b) => a.id.localeCompare(b.id))
}

async function selftest() {
  const make = (n) => Array.from({ length: n }, (_, i) => ({ id: String(i), reason: 'fixture', extensionReason: 'fixture' }))
  const policy = (n, limit = 600) => ({ baseline: 300, limit, cases: make(n) })
  testCase('budget:reject-601', () => assert.throws(() => enforceBudget(make(601), policy(601)), /超出/))
  testCase('budget:reject-empty', () => assert.throws(() => enforceBudget([], policy(0)), /未收集/))
  testCase('budget:require-extension-reason', () => assert.throws(() => enforceBudget(make(301), { ...policy(301, 301), cases: make(301).map((c) => ({ ...c, extensionReason: '' })) }), /增额依据/))
  testCase('budget:rust-cfg-and-literals', () => {
    const source = '// #[test] fn comment() {}\nconst S: &str = r##"#[test] fn string() {}"##;\n#[cfg(unix)] #[test] #[ignore] fn retained() {}'
    assert.deepEqual(collectRust('fixture.rs', source).map((c) => c.name), ['retained'])
  })
  testCase('budget:worker-expansion', () => assert.equal(collectScripts('fixture.mjs', "const EXPECTED_WORKERS=[{id:'a'},{id:'b'},{id:'c'}];runStep('workers:handshake',()=>{});").length, 3))
  testCase('budget:acl-expansion', () => assert.deepEqual(collectScripts('fixture.mjs', "runStep('chain:new-command-acl',()=>{const commands=['clear','retry'];ipc(cdp,commands[0]);ipc(cdp,commands[1]);});").map((c) => c.name), ['chain:new-command-acl/clear', 'chain:new-command-acl/retry']))
  testCase('budget:enhance-expansion', () => assert.equal(collectScripts('fixture.mjs', "const attempts=[['start',{}],['preview',{}],['download',{}]];runStep('enhance:manifest-unready',()=>{});").length, 3))
  const temporary = mkdtempSync(join(tmpdir(), 'scrollery-test-budget-'))
  try {
    await testCase('budget:vitest-expanded-and-skipped', async () => {
      writeFileSync(join(temporary, 'budget.spec.mjs'), `import { it } from ${JSON.stringify(import.meta.resolve('vitest'))}\nit.each([1,2,3])('expanded %s',()=>{throw Error('must not execute')});it.skip('skipped',()=>{throw Error('must not execute')});it.todo('todo')`)
      assert.equal((await collectVitest({ root: temporary, config: false, include: ['*.spec.mjs'] })).length, 5)
    })
    await testCase('budget:collection-errors', async () => {
      writeFileSync(join(temporary, 'budget.spec.mjs'), "throw new Error('budget collection failure')\n")
      const previousExitCode = process.exitCode
      await assert.rejects(() => collectVitest({ root: temporary, config: false, include: ['*.spec.mjs'] }), /测试收集失败:.*budget collection failure/)
      // Vitest 会设置进程退出码；这里的收集失败是预期的自检输入。
      process.exitCode = previousExitCode
    })
  } finally {
    const actual = realpathSync(temporary)
    if (!actual.startsWith(realpathSync(tmpdir()) + sep)) throw new Error('临时目录清理越过边界')
    rmSync(actual, { recursive: true, force: true })
  }
  console.log('[test-budget] 9 selftests passed')
}

try {
  const args = process.argv.slice(2)
  if (args.some((arg) => !['--selftest', '--inventory'].includes(arg))) throw new Error('用法: check-test-budget.mjs [--selftest] [--inventory]')
  if (args.includes('--selftest')) {
    await selftest()
    // 缩略图采样自检依赖 Windows CIM；清单收集仍在所有平台计入这六项。
    if (process.platform === 'win32') execFileSync(process.execPath, [join(root, 'scripts/bench/thumbnail-baseline-selftest.mjs')], { cwd: root, stdio: 'inherit' })
  }
  const cases = await collectAll()
  if (args.includes('--inventory')) console.log(JSON.stringify(cases, null, 2))
  else {
    const policy = JSON.parse(readFileSync(join(root, 'test-budget.json'), 'utf8'))
    const total = enforceBudget(cases, policy)
    for (const pool of ['vitest', 'rust', 'script', 'benchmark']) console.log(`${pool}: ${cases.filter((c) => c.pool === pool).length}`)
    console.log(`[test-budget] PASS: ${total}/${policy.limit}; baseline 300`)
  }
} catch (error) { console.error(`[test-budget] FAIL: ${error.message}`); process.exitCode = 1 }
