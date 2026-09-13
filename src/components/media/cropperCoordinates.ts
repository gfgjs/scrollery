import type { CropRectNorm } from '../../composables/useImageEditor'

export function normalizedToSelection(rect: CropRectNorm, width: number, height: number) {
  return {
    x: rect.x * width,
    y: rect.y * height,
    width: rect.width * width,
    height: rect.height * height,
  }
}

export function selectionToNormalized(
  rect: { x: number; y: number; width: number; height: number },
  width: number,
  height: number,
): CropRectNorm {
  return {
    x: rect.x / width,
    y: rect.y / height,
    width: rect.width / width,
    height: rect.height / height,
  }
}
