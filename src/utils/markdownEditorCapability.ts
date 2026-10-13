/** 按当前 WebView 的实际 API 判断实验 Markdown 编辑器是否可用。 */
export function hasEditContext(): boolean {
  const globals = globalThis as typeof globalThis & { EditContext?: unknown }
  return typeof globals.EditContext === 'function'
}
