import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const canvasSource = readFileSync(
  fileURLToPath(new URL('./MediaGridCanvas.vue', import.meta.url)),
  'utf8',
)
const gridSource = readFileSync(fileURLToPath(new URL('./MediaGrid.vue', import.meta.url)), 'utf8')
const glassCss = readFileSync(
  fileURLToPath(new URL('../../assets/styles/glass.css', import.meta.url)),
  'utf8',
)

describe('gallery glass backdrop contract', () => {
  it('only enables a transparent canvas for the active Windows glass material', () => {
    expect(gridSource).toContain(':glass-background="galleryUsesGlass"')
    expect(gridSource).toMatch(
      /const galleryUsesGlass = computed\(\(\) => isWindows && ui\.windowMaterial !== 'none'\)/,
    )
    expect(canvasSource).toMatch(/glassBackground:\s*boolean/)
  })

  it('recreates the canvas for its immutable alpha mode and clears glass frames transparently', () => {
    expect(canvasSource).toContain(':key="glassBackground ? \'glass\' : \'opaque\'"')
    expect(canvasSource).toContain("getContext('2d', { alpha: props.glassBackground })")
    expect(canvasSource).toMatch(
      /if \(props\.glassBackground\) \{\s*ctx\.clearRect\(0, 0, w, h\)\s*\} else \{\s*ctx\.fillStyle = palette\.canvasGap/,
    )
  })

  it('keeps the DOM gallery host adjustable and transparent by default', () => {
    expect(glassCss).toMatch(
      /--glass-gallery-opacity:\s*0%;[\s\S]*?--glass-gallery-fill:\s*color-mix\([\s\S]*?var\(--color-bg-canvas\) var\(--glass-gallery-opacity\)/,
    )
    expect(glassCss).toMatch(
      /html\[data-glass\] body \.app-main > \.app-content \.media-grid-layout \{\s*background:\s*var\(--glass-gallery-fill\);/,
    )
  })
})
