// src/composables/useViewerImageSource.ts
// 大图查看器的「预加载后提交」状态机：候选 URL 只在离屏 Image 完成 load + decode 后才进入
// 可见 <img>。跨条目切换立即撤下旧帧，避免加载/报错期间画面与当前条目不一致；同条目派生
// 换源保留当前帧。原图失败时进入错误态，派生源失败可由调用方切回原图继续尝试。

import { onScopeDispose, ref, watch, type Ref } from 'vue'

export interface ViewerImageSourceItem {
  id: number
  mediaType: string
  availability: string
}

export interface ViewerImageSourceInput {
  /** 当前查看项。仅 online image 会启动候选加载。 */
  item: () => ViewerImageSourceItem | null | undefined
  /** 当前候选 URL；可从原图异步升级为 ICC 派生图。 */
  source: () => string
  /**
   * 是否暂停原图解码（例如极速连翻飞掠中仅展示缩略图）。
   * 为 true 时暂不启动后台 Image.decode()，变为 false 后自动为当前项补启动。
   */
  paused?: () => boolean
  /**
   * 候选失败后的可选回退。返回 true 表示调用方已切换候选（例如派生图 → 原图），本状态机
   * 保留现有已提交帧并等待新候选；false 表示原始候选也失败，进入错误态。
   */
  handleCandidateError?: (url: string, itemId: number) => boolean
}

export interface ViewerImageSource {
  /** 只包含已经离屏加载并解码成功的可见 URL；空串表示当前没有可展示图片。 */
  displaySrc: Ref<string>
  /** displaySrc 所属条目。跨条目等待时为 null，同条目派生换源时保持当前条目。 */
  displayItemId: Ref<number | null>
  /** 当前候选仍在后台加载/解码。 */
  loading: Ref<boolean>
  /** 当前 online image 的原始候选确认失败。 */
  loadError: Ref<boolean>
  /** 极小的「预解码成功后文件又消失」竞态兜底，供可见 <img> @error 调用。 */
  handleDisplayError: (url: string) => void
}

export function useViewerImageSource(input: ViewerImageSourceInput): ViewerImageSource {
  const displaySrc = ref('')
  const displayItemId = ref<number | null>(null)
  const loading = ref(false)
  const loadError = ref(false)

  let generation = 0
  let pendingImage: HTMLImageElement | null = null

  function detach(image: HTMLImageElement | null): void {
    if (!image) return
    image.onload = null
    image.onerror = null
  }

  function cancelPending(): void {
    generation += 1
    detach(pendingImage)
    pendingImage = null
    loading.value = false
  }

  function clearDisplay(): void {
    displaySrc.value = ''
    displayItemId.value = null
  }

  function startCandidate(itemId: number, url: string): void {
    const myGeneration = ++generation
    detach(pendingImage)
    pendingImage = null
    loadError.value = false

    // detail 已切到新条目后，旧帧即使稳定不闪也会误导用户；立即交给宿主渲染目标缩略图或
    // 中性加载态。同一条目内部的 ICC 派生换源则保留当前原图，待派生解码成功再替换。
    if (displayItemId.value !== null && displayItemId.value !== itemId) clearDisplay()

    if (displayItemId.value === itemId && displaySrc.value === url) {
      loading.value = false
      return
    }

    loading.value = true
    const image = new Image()
    pendingImage = image
    image.decoding = 'async'

    function failCandidate(): void {
      if (myGeneration !== generation || pendingImage !== image) return
      detach(image)
      pendingImage = null
      loading.value = false
      if (input.handleCandidateError?.(url, itemId)) return
      // 原图失败：候选 URL 从未进入可见层，直接由加载态进入错误态。
      clearDisplay()
      loadError.value = true
    }

    image.onload = () => {
      // load 仅代表资源读取完成；损坏文件仍可能在 decode 阶段失败。只有真实解码成功后才
      // 提交可见源，避免把半有效候选重新绑定到可见 <img> 后再闪旧帧或二次报错。
      const decode = typeof image.decode === 'function' ? image.decode() : Promise.resolve()
      void decode.then(() => {
        if (myGeneration !== generation || pendingImage !== image) return
        detach(image)
        pendingImage = null
        displaySrc.value = url
        displayItemId.value = itemId
        loading.value = false
        loadError.value = false
      }, failCandidate)
    }

    image.onerror = failCandidate
    image.src = url
  }

  watch(
    () => {
      const item = input.item()
      return [
        item?.id ?? null,
        item?.mediaType ?? null,
        item?.availability ?? null,
        input.source(),
        input.paused?.() ?? false,
      ] as const
    },
    ([itemId, mediaType, availability, source, paused]) => {
      if (
        itemId === null ||
        mediaType !== 'image' ||
        availability !== 'online' ||
        source.length === 0
      ) {
        cancelPending()
        loadError.value = false
        clearDisplay()
        return
      }
      if (paused) {
        cancelPending()
        if (displayItemId.value !== null && displayItemId.value !== itemId) {
          clearDisplay()
        }
        return
      }
      startCandidate(itemId, source)
    },
    { immediate: true },
  )

  function handleDisplayError(url: string): void {
    if (!url || url !== displaySrc.value || displayItemId.value === null) return
    const itemId = displayItemId.value
    const current = input.item()
    // 已切换条目时，旧可见 DOM 的迟到事件不应污染当前候选。
    if (!current || current.id !== itemId) return

    const handled = input.handleCandidateError?.(url, itemId) ?? false
    clearDisplay()
    if (!handled) loadError.value = true
  }

  onScopeDispose(cancelPending)

  return { displaySrc, displayItemId, loading, loadError, handleDisplayError }
}
