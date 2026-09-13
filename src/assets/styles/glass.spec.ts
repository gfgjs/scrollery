import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const glassCss = readFileSync(fileURLToPath(new URL('./glass.css', import.meta.url)), 'utf8')

describe('window glass surface contract', () => {
  it('clears the bootstrap html background so the native backdrop can show', () => {
    expect(glassCss).toMatch(
      /html\[data-glass\]\s*\{[\s\S]*?background:\s*transparent;/,
    )
  })

  it('keeps the app content container transparent while text-heavy view roots carry the content fill', () => {
    expect(glassCss).toMatch(
      /html\[data-glass\] body \.app-main > \.app-content \{[\s\S]*?background:\s*transparent;/,
    )
    expect(glassCss).toMatch(
      /html\[data-glass\] body \.app-main > \.app-content > \.settings-view,[\s\S]*?\.doc-viewer \{[\s\S]*?background:\s*var\(--glass-content-fill\);/,
    )
    expect(glassCss).toMatch(
      /\.settings-view \.settings-header \{[\s\S]*?background:\s*transparent;/,
    )
  })

  it('does not double-composite the native material behind window controls', () => {
    expect(glassCss).toMatch(
      /html\[data-glass\] body \.window-chrome \.window-chrome__controls \{[\s\S]*?background-color:\s*transparent;[\s\S]*?background-image:\s*none;/,
    )
  })

  it('keeps sticky sidebar content readable with a translucent glass mask', () => {
    expect(glassCss).toMatch(
      /--glass-sticky-fill:[\s\S]*?html\[data-glass\] body \.app-sidebar \.acc-header,[\s\S]*?html\[data-glass\] body \.app-sidebar \.sticky-item \{[\s\S]*?background-color:\s*var\(--glass-sticky-fill\);[\s\S]*?backdrop-filter:\s*blur\(12px\)/,
    )
    expect(glassCss).toMatch(
      /\.tree-item:not\(\.sticky-item\)\s*\{[\s\S]*?background-color:\s*transparent;/,
    )
  })

  it('uses translucent surfaces for cards, search, and sidebar toggle controls', () => {
    expect(glassCss).toMatch(
      /\.tool__card\s*\{[\s\S]*?background-color:\s*var\(--glass-surface-fill\);/,
    )
    expect(glassCss).toMatch(
      /\.window-chrome \.toolbar__search-wrap\s*\{[\s\S]*?background-color:\s*var\(--glass-control-fill\);/,
    )
    expect(glassCss).toMatch(
      /\.toggle__thumb\s*\{[\s\S]*?background-color:\s*var\(--glass-control-thumb\);/,
    )
  })

  it('exposes independent opacity scales for the five glass surface groups', () => {
    expect(glassCss).toMatch(/--glass-chrome-scale:\s*1;/)
    expect(glassCss).toMatch(/calc\(72% \* var\(--glass-chrome-scale\)\)/)
    expect(glassCss).toMatch(/calc\(42% \* var\(--glass-sticky-scale\)\)/)
    expect(glassCss).toMatch(/calc\(24% \* var\(--glass-surface-scale\)\)/)
    expect(glassCss).toMatch(/calc\(38% \* var\(--glass-control-scale\)\)/)
    expect(glassCss).toMatch(/--glass-content-scale:\s*1;/)
    expect(glassCss).toMatch(
      /--glass-content-fill:\s*color-mix\([\s\S]*?var\(--color-bg-primary\) calc\(90% \* var\(--glass-content-scale\)\)/,
    )
  })

  it('exposes a separate gallery base overlay without changing opaque mode', () => {
    expect(glassCss).toMatch(/--glass-gallery-opacity:\s*0%;/)
    expect(glassCss).toMatch(
      /--glass-gallery-fill:\s*color-mix\([\s\S]*?var\(--color-bg-canvas\) var\(--glass-gallery-opacity\)/,
    )
    expect(glassCss).toMatch(
      /html\[data-glass\] body \.app-main > \.app-content \.media-grid-layout\s*\{[\s\S]*?background:\s*var\(--glass-gallery-fill\);/,
    )
  })
})
