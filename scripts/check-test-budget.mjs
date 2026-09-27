#!/usr/bin/env node
// 使用 Vitest 的实际收集结果,包含 each 展开与 skipped/todo,避免数 it 文本或漏算跳过项。
import assert from 'node:assert/strict'
import { existsSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const MAX_TESTS = 100
const MAX_RUST_TESTS = 200

function enforceBudget(counts, limit = MAX_TESTS) {
  const total = counts.reduce((sum, entry) => sum + entry.count, 0)
  if (total === 0) throw new Error('未收集到测试,不能把空清单当作预算通过')
  if (total > limit) throw new Error(`用例 ${total} 超出预算 ${limit}`)
  return total
}

async function collectCounts(options = {}) {
  const { createVitest } = await import('vitest/node')
  const ctx = await createVitest('test', {
    root,
    watch: false,
    maxWorkers: 2,
    allowOnly: false,
    reporters: [],
    ...options,
  })
  try {
    const { testModules, unhandledErrors } = await ctx.collect()
    const errors = [...unhandledErrors]
    for (const module of testModules) {
      errors.push(...module.errors())
      for (const suite of module.children.allSuites()) errors.push(...suite.errors())
    }
    if (errors.length) {
      throw new Error(`测试收集失败: ${errors.map((e) => e?.message ?? String(e)).join('; ')}`)
    }
    // list --json 会过滤 skipped;直接枚举 allTests 才能保持预算口径。
    return testModules
      .map((module) => ({ file: module.moduleId, count: [...module.children.allTests()].length }))
      .sort((a, b) => b.count - a.count || a.file.localeCompare(b.file))
  } finally {
    await ctx.close()
  }
}

// 屏蔽注释、普通/原始字符串和字符字面量，避免把示例中的 #[test] 算成测试。
// 保留生命周期标记；块注释支持 Rust 的嵌套规则。
function rustCode(source) {
  let result = ''
  for (let i = 0; i < source.length;) {
    if (source.startsWith('//', i)) {
      const end = source.indexOf('\n', i)
      i = end < 0 ? source.length : end
      result += ' '
    } else if (source.startsWith('/*', i)) {
      let depth = 1
      i += 2
      while (i < source.length && depth) {
        if (source.startsWith('/*', i)) { depth++; i += 2 }
        else if (source.startsWith('*/', i)) { depth--; i += 2 }
        else i++
      }
      if (depth) throw new Error('Rust 块注释未闭合')
      result += ' '
    } else {
      const raw = /^(?:b|c)?r(#{0,255})"/.exec(source.slice(i))
      if (raw) {
        const end = source.indexOf(`"${raw[1]}`, i + raw[0].length)
        if (end < 0) throw new Error('Rust 原始字符串未闭合')
        i = end + 1 + raw[1].length
        result += ' '
      } else if (source[i] === '"') {
        i++
        while (i < source.length && source[i] !== '"') {
          i += source[i] === '\\' ? 2 : 1
        }
        if (i >= source.length) throw new Error('Rust 字符串未闭合')
        i++
        result += ' '
      } else {
        const character = /^'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^'\\])'/u.exec(source.slice(i))
        if (character) { i += character[0].length; result += ' ' }
        else result += source[i++]
      }
    }
  }
  return result
}

function countRustTests(source) {
  const code = rustCode(source)
  // 当前仓库使用普通 test 属性；引入生成测试的宏时须先升级展开计数口径。
  if (/\b(?:rstest|test_case|proptest|quickcheck)\b/.test(code)) {
    throw new Error('发现参数化/生成式 Rust 测试，需增加展开计数后再使用')
  }
  return [...code.matchAll(/#\s*\[\s*(?:[A-Za-z_]\w*\s*::\s*)*test\s*(?:\([^\]]*\))?\s*\]/g)].length
}

function collectRustCounts() {
  // Git 文件清单包含未跟踪源码，过滤工作树中已删文件；覆盖独立 workspace 和各 cfg 分支。
  const files = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard', '--', 'src-tauri', 'crates'], { cwd: root, encoding: 'utf8' })
    .split('\0').filter((file) => file.endsWith('.rs') && existsSync(join(root, file)))
  return [...new Set(files)].map((file) => ({
    file,
    count: countRustTests(readFileSync(join(root, file), 'utf8')),
  })).filter((entry) => entry.count > 0).sort((a, b) => b.count - a.count || a.file.localeCompare(b.file))
}

async function selftest() {
  assert.equal(enforceBudget([{ count: 99 }]), 99)
  assert.equal(enforceBudget([{ count: 50 }, { count: 50 }]), 100)
  assert.throws(() => enforceBudget([{ count: 101 }]), /超出预算/)
  assert.throws(() => enforceBudget([]), /未收集到测试/)
  assert.equal(enforceBudget([{ count: 199 }], MAX_RUST_TESTS), 199)
  assert.equal(enforceBudget([{ count: 100 }, { count: 100 }], MAX_RUST_TESTS), 200)
  assert.throws(() => enforceBudget([{ count: 201 }], MAX_RUST_TESTS), /超出预算/)
  assert.throws(() => enforceBudget([], MAX_RUST_TESTS), /未收集到测试/)
  assert.equal(countRustTests(`
// #[test]
/* nested /* #[test] */ comment */
const S: &str = r##"#[test]"##;
const B: &[u8] = br#"#[test]"#;
const Q: char = '"';
const NORMAL: &str = "#[test]";
fn lifetime<'a>(x: &'a str) {}
#[test] fn normal() {}
#[cfg(unix)] #[test] #[ignore] fn platform() {}
#[tokio::test(flavor = "current_thread")] async fn async_test() {}
`), 3)
  assert.throws(() => countRustTests('#[rstest] fn generated() {}'), /展开计数/)

  // 真实收集一个临时定义:3 项 each + 1 skipped + 1 todo,测试体抛错以保证未被执行。
  const temporary = mkdtempSync(join(tmpdir(), 'scrollery-test-budget-'))
  try {
    writeFileSync(join(temporary, 'budget.spec.mjs'), `
import { it } from ${JSON.stringify(import.meta.resolve('vitest'))}
it.each([1, 2, 3])('expanded %s', () => { throw new Error('must not execute') })
it.skip('skipped still counts', () => { throw new Error('must not execute') })
it.todo('todo still counts')
`)
    const counts = await collectCounts({ root: temporary, config: false, include: ['*.spec.mjs'] })
    assert.equal(counts.length, 1)
    assert.equal(enforceBudget(counts), 5)
    writeFileSync(join(temporary, 'budget.spec.mjs'), "throw new Error('budget collection failure')\n")
    const previousExitCode = process.exitCode
    await assert.rejects(
      () => collectCounts({ root: temporary, config: false, include: ['*.spec.mjs'] }),
      /测试收集失败:.*budget collection failure/,
    )
    // Vitest 会为收集失败设置全局退出码；负例已验证，不能让预期失败污染预算门。
    process.exitCode = previousExitCode
    console.log('[test-budget] selftest passed: 99/100/101, 199/200/201, empty, each, skipped, todo, errors, Rust literals/cfg/ignore')
  } finally {
    const actual = realpathSync(temporary)
    const boundary = realpathSync(tmpdir()) + sep
    if (!actual.startsWith(boundary)) throw new Error('临时目录清理越过授权边界')
    rmSync(actual, { recursive: true, force: true })
  }
}

try {
  if (process.argv.slice(2).some((arg) => arg !== '--selftest')) {
    throw new Error('用法: node scripts/check-test-budget.mjs [--selftest]')
  }
  if (process.argv.includes('--selftest')) await selftest()
  const counts = await collectCounts()
  for (const entry of counts) console.log(`${entry.count}\t${relative(root, entry.file)}`)
  const total = enforceBudget(counts)
  console.log(`[test-budget] Vitest PASS: ${total}/${MAX_TESTS} cases in ${counts.length} files`)
  const rustCounts = collectRustCounts()
  for (const entry of rustCounts) console.log(`${entry.count}\t${entry.file}`)
  const rustTotal = enforceBudget(rustCounts, MAX_RUST_TESTS)
  console.log(`[test-budget] Rust PASS: ${rustTotal}/${MAX_RUST_TESTS} definitions in ${rustCounts.length} files (cfg/ignore included)`)
} catch (error) {
  console.error(`[test-budget] FAIL: ${error.message}`)
  process.exitCode = 1
}
