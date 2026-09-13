// 启动层的超时提示必须是独立经典脚本：入口 bundle/Vue 卡住时仍能给出真实状态，
// 但不显示没有后端依据的百分比或阶段进度。
;(function () {
  var layer = document.getElementById('startup-layer')
  var status = document.getElementById('startup-layer-status')
  if (!layer || !status) return

  window.setTimeout(function () {
    if (layer.dataset.state === 'loading') {
      status.textContent = '启动时间较长，请稍候'
    }
  }, 12000)
})()
