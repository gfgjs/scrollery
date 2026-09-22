/** 离线导览交互；所有架构内容和检索条目来自构建产物。 */
(() => {
  const $ = (s) => document.querySelector(s)
  const documents = [...document.querySelectorAll('.document')]
  const search = $('#search')
  const results = $('#search-results')
  const items = $('#search-items')
  const index = JSON.parse($('#search-index').textContent)
  const dialog = $('#diagram-dialog')
  let expandedFigure = null
  let restorePosition = null

  function restoreFigure() {
    if (!expandedFigure) return
    restorePosition.replaceWith(expandedFigure)
    const button = expandedFigure.querySelector('[data-zoom="expand"]')
    button.hidden = false
    button.focus({ preventScroll: true })
    expandedFigure = null
  }

  function closeMenu() {
    document.body.classList.remove('menu-open')
    $('#menu-toggle').setAttribute('aria-expanded', 'false')
    $('#shade').hidden = true
  }

  function route(scroll = true) {
    if (dialog.open) { dialog.close(); restoreFigure() }
    let id
    try { id = decodeURIComponent(location.hash.slice(1)) } catch { id = '' }
    const target = document.getElementById(id) || $('#doc-readme')
    const active = target.closest('.document') || $('#doc-readme')
    for (const doc of documents) doc.hidden = doc !== active
    $('#current-document').textContent = active.dataset.title
    for (const anchor of document.querySelectorAll('#sidebar a')) {
      const selected = anchor.hash === `#${active.id}` || decodeURIComponent(anchor.hash) === `#${id}`
      if (selected) {
        anchor.setAttribute('aria-current', 'location')
        const group = anchor.closest('details')
        if (group) group.open = true
      } else anchor.removeAttribute('aria-current')
    }
    let parent = target.parentElement
    while (parent) {
      if (parent.tagName === 'DETAILS') parent.open = true
      parent = parent.parentElement
    }
    if (scroll) {
      if (target === active) window.scrollTo(0, 0)
      else target.scrollIntoView({ block: 'start' })
      if (target !== active) {
        target.setAttribute('tabindex', '-1')
        target.focus({ preventScroll: true })
      }
    }
    closeMenu()
    results.hidden = true
  }

  window.addEventListener('hashchange', () => route())
  document.addEventListener('click', (event) => {
    const anchor = event.target.closest('a[href^="#"]')
    if (!anchor || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return
    event.preventDefault()
    if (location.hash === anchor.hash) route()
    else location.hash = anchor.hash
  })
  $('#menu-toggle').addEventListener('click', () => {
    const opened = document.body.classList.toggle('menu-open')
    $('#menu-toggle').setAttribute('aria-expanded', String(opened))
    $('#shade').hidden = !opened
  })
  $('#shade').addEventListener('click', closeMenu)

  function highlight(element, value, term) {
    const lower = value.toLocaleLowerCase()
    let from = 0
    let next = lower.indexOf(term)
    while (next !== -1) {
      element.append(document.createTextNode(value.slice(from, next)))
      const mark = document.createElement('mark')
      mark.textContent = value.slice(next, next + term.length)
      element.append(mark)
      from = next + term.length
      next = lower.indexOf(term, from)
    }
    element.append(document.createTextNode(value.slice(from)))
  }

  function runSearch() {
    const term = search.value.trim().toLocaleLowerCase()
    items.replaceChildren()
    results.hidden = !term
    if (!term) return
    const matches = index.map((entry) => ({ ...entry,
      score: entry.title.toLocaleLowerCase().includes(term) ? 2 : entry.text.toLocaleLowerCase().includes(term) ? 1 : 0,
    })).filter((entry) => entry.score).sort((a, b) => b.score - a.score)
    $('#search-status').textContent = matches.length ? `${matches.length} 个结果${matches.length > 30 ? ' · 显示前 30 项' : ''}` : '没有匹配结果，请尝试功能名、流程 ID 或源码符号。'
    for (const entry of matches.slice(0, 30)) {
      const anchor = document.createElement('a')
      anchor.className = 'search-result'
      anchor.href = `#${entry.id}`
      const group = document.createElement('small')
      group.textContent = entry.group
      const title = document.createElement('strong')
      highlight(title, entry.title, term)
      const snippet = document.createElement('p')
      const offset = Math.max(0, entry.text.toLocaleLowerCase().indexOf(term) - 25)
      highlight(snippet, (offset ? '…' : '') + entry.text.slice(offset, offset + 135), term)
      anchor.append(group, title, snippet)
      items.append(anchor)
    }
  }
  search.addEventListener('input', runSearch)
  search.addEventListener('focus', () => { if (search.value) runSearch() })
  document.addEventListener('click', (event) => { if (!event.target.closest('.search')) results.hidden = true })
  document.addEventListener('keydown', (event) => {
    if (event.key === '/' && !/INPUT|TEXTAREA/.test(document.activeElement.tagName)) {
      event.preventDefault()
      search.focus()
    }
    if (event.key === 'Escape') { results.hidden = true; closeMenu() }
    if (!results.hidden && ['ArrowDown', 'ArrowUp', 'Enter'].includes(event.key)) {
      const links = [...items.querySelectorAll('a')]
      const pos = links.indexOf(document.activeElement)
      if (event.key === 'Enter' && document.activeElement === search) {
        event.preventDefault()
        links[0]?.click()
      } else if (event.key !== 'Enter' && links.length) {
        event.preventDefault()
        links[(pos + (event.key === 'ArrowDown' ? 1 : -1) + links.length) % links.length].focus()
      }
    }
  })

  document.addEventListener('click', (event) => {
    const button = event.target.closest('[data-zoom]')
    if (!button) return
    const figure = button.closest('figure')
    const stage = figure.querySelector('.diagram-stage')
    const viewport = figure.querySelector('.diagram-viewport')
    const action = button.dataset.zoom
    if (action === 'expand') {
      // 移动原图避免复制 SVG ID，关闭时归位，深链保持不变。
      expandedFigure = figure
      restorePosition = document.createComment('diagram-position')
      figure.before(restorePosition)
      $('#expanded-diagram').append(figure)
      button.hidden = true
      dialog.showModal()
      return
    }
    const padding = getComputedStyle(viewport)
    const available = viewport.clientWidth - parseFloat(padding.paddingLeft) - parseFloat(padding.paddingRight)
    const natural = Math.max(1, stage.querySelector('svg').viewBox.baseVal.width / available)
    const scale = action === 'reset' ? 1 : action === 'actual' ? natural : Math.min(Math.max(8, natural), Math.max(1, Number(stage.dataset.scale || 1) + (action === 'in' ? .25 : -.25)))
    stage.dataset.scale = String(scale)
    stage.style.width = `${scale * 100}%`
    figure.querySelector('output').value = `${Math.round(scale * 100)}%`
    if (action === 'reset') viewport.scrollTo(0, 0)
  })
  $('#close-diagram').addEventListener('click', () => dialog.close())
  dialog.addEventListener('close', restoreFigure)
  if (matchMedia('(max-width: 760px)').matches) {
    document.querySelectorAll('.reading-path').forEach((element) => { element.open = true })
  }
  route(Boolean(location.hash))
  document.documentElement.dataset.ready = 'true'
})()
