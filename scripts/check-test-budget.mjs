#!/usr/bin/env node
// 使用 Vitest 的实际收集结果,包含 each 展开与 skipped/todo,避免数 it 文本或漏算跳过项。
import assert from 'node:assert/strict'
import { mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const MAX_TESTS = 500

function enforceBudget(counts) {
  const total = counts.reduce((sum, entry) => sum + entry.count, 0)
  if (total === 0) throw new Error('未收集到测试,不能把空清单当作预算通过')
  if (total > MAX_TESTS) throw new Error(`展开用例 ${total} 超出预算 ${MAX_TESTS}`)
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

async function selftest() {
  assert.equal(enforceBudget([{ count: 499 }]), 499)
  assert.equal(enforceBudget([{ count: 250 }, { count: 250 }]), 500)
  assert.throws(() => enforceBudget([{ count: 501 }]), /超出预算/)
  assert.throws(() => enforceBudget([]), /未收集到测试/)

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
    await assert.rejects(
      () => collectCounts({ root: temporary, config: false, include: ['*.spec.mjs'] }),
      /测试收集失败:.*budget collection failure/,
    )
    console.log('[test-budget] selftest passed: 499/500/501, empty, each, skipped, todo, errors')
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
  console.log(`[test-budget] PASS: ${total}/${MAX_TESTS} cases in ${counts.length} files`)
} catch (error) {
  console.error(`[test-budget] FAIL: ${error.message}`)
  process.exitCode = 1
}
