import { afterEach, describe, expect, it, vi } from 'vitest'

import { dismissStartupLayer } from './startupLayer'

interface FakeLayer {
  dataset: { state?: string }
  remove: () => void
}

function stubStartupLayer(layer: FakeLayer | null) {
  const getElementById = vi.fn((id: string) => (id === 'startup-layer' ? layer : null))
  vi.stubGlobal('document', { getElementById } as unknown as Document)
  return getElementById
}

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('dismissStartupLayer', () => {
  it('marks the layer ready and removes it after the fade-out window', () => {
    vi.useFakeTimers()
    const remove = vi.fn()
    const layer: FakeLayer = { dataset: { state: 'loading' }, remove }
    const getElementById = stubStartupLayer(layer)

    dismissStartupLayer()

    expect(getElementById).toHaveBeenCalledWith('startup-layer')
    expect(layer.dataset.state).toBe('ready')
    expect(remove).not.toHaveBeenCalled()

    vi.advanceTimersByTime(359)
    expect(remove).not.toHaveBeenCalled()

    vi.advanceTimersByTime(1)
    expect(remove).toHaveBeenCalledOnce()
  })

  it('does not schedule a second fade when the layer is already ready', () => {
    vi.useFakeTimers()
    const remove = vi.fn()
    const layer: FakeLayer = { dataset: { state: 'ready' }, remove }
    stubStartupLayer(layer)

    dismissStartupLayer()
    vi.runAllTimers()

    expect(remove).not.toHaveBeenCalled()
  })
})
