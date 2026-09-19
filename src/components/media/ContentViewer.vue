<template>
  <!-- 顶栏重构 P4-b:看图台从 body 覆盖层迁为 shell 内路由(/view/:id)。去 Teleport/backdrop,
       三层容器(content-viewer 填充 .app-content / __inner 守卫 media.detailItem / detail-panel
       原布局)刻意保留原缩进,内核(useMediaDetail 缩放/平移/翻页/信息/exotic/人脸)逐字节不动;
       过渡态:控制条暂留本组件,待 P5-5 收敛入顶栏上下文工具栏。 -->
  <div class="content-viewer">
    <div v-if="media.detailItem" class="content-viewer__inner">
      <!-- 面板 -->
      <!-- is-info-open:信息面板停靠时给面板加 padding-right 让出图片区(挤压式,替代原覆盖层)。
           padding 动画与信息面板 translateX 滑入同用 --transition-normal,逐帧同步不脱节。 -->
      <div class="detail-panel" :class="{ 'is-info-open': ui.viewerInfoVisible }">
        <!-- ── 视图器 ───────────────────────────────────────────────────── -->
        <!-- 点图不再关信息面板(2026-07-14):面板改停靠且仅手动开合,原 @click=onImageClick 撤除。 -->
        <div
          class="detail-viewer"
          :class="{ 'has-bottom-controls': detailControlsVisible }"
          ref="viewerRef"
          @wheel.prevent="onWheelHandler"
          @mousedown="onViewerPointerDown"
          @click="onViewerClick"
          @contextmenu.prevent="onContextMenu"
        >
          <!-- 不可用占位（缺失检测 Part2 §3.2）：卷离线 / 文件缺失 / 加载失败时给明确提示，
               而非任由 <img>/<video> 显示浏览器的 broken 图标（体验差、不知所以然）。 -->
          <div v-if="isUnavailable" class="detail-viewer__unavailable">
            <ImageOff :size="56" />
            <p class="detail-viewer__unavailable-title">{{ unavailableInfo.title }}</p>
            <p class="detail-viewer__unavailable-hint">{{ unavailableInfo.hint }}</p>
          </div>
          <!-- Exotic 授权 gate（Part5 T12）：未授权的 exotic 格式（如未购买 PSD）显示购买/激活引导，
               而非任由 <img> 尝试解码注定失败的原图。已授权 / 普通格式不进此分支（showExoticGate=false）。 -->
          <div v-else-if="showExoticGate" class="detail-viewer__gate" @click.stop>
            <PluginGate
              :entitlement="exoticGate.entitlement.value"
              :feature-name="exoticFeatureName"
              @activate="activateOpen = true"
            />
          </div>
          <template v-else>
            <EditOverlay
              v-if="detail.mediaType === 'image' && editor.status.value !== 'idle'"
              :detail="detail"
              :editor="editor"
              @close="onEditorClosed"
              @saved="onEditorSaved"
            />
            <template v-else-if="detail.mediaType === 'image'">
              <!-- 底层缩略图：全幅等比对齐，在大图未就绪时提供 0ms 背景占位 -->
              <img
                v-if="imagePlaceholder"
                ref="placeholderImgRef"
                :key="'thumb:' + detail.id"
                :src="imagePlaceholder"
                class="detail-viewer__img detail-viewer__img--placeholder"
                :class="{ 'is-dragging': state.isDragging.value }"
                :style="imageBoxStyle"

                draggable="false"

                @error="onImagePlaceholderError"
              />
              <!-- 顶层主图：直绑 absPath 资产源，小图/缓存图由浏览器 C++ 管道即刻直出，0 延迟无过渡感知 -->
              <img
                v-if="absPath"
                ref="imgRef"
                :key="'full:' + detail.id"
                :src="absPath"
                class="detail-viewer__img detail-viewer__img--highres"
                :class="{
                  'is-dragging': state.isDragging.value,
                }"
                :style="imageBoxStyle"
                draggable="false"
                @load="onImgLoad"
                @error="onVisibleImageError"
              />
              <!-- 无缩略图且在加载中时的居中轻量 Spinner -->
              <span
                v-if="!imagePlaceholder && !isImageLoaded && !loadError"
                class="detail-viewer__image-loading-spinner"

              />
            </template>
            <!-- 视频:自研播放器编排层(播放器线 GC)。transform 施加在内部 <video>、控制条不随缩放;
                 @loadedmetadata 继续驱动 updateZoomRatio;失败经其内部 VideoDiagnostics 接管(不走 onMediaError)。
                 源改走 useVideoSource(视频格式扩展子系统 design.md §5.3):direct/derived 才挂真实
                 <video>,其余态(preparing/needsComponent/needsHevcExt/needsConfirm/error/cancelled)只挂
                 引导覆盖层——cancelled 态显式给出「重新准备」入口(不留白,详见 useVideoSource 内注释),
                 videoRef 为空时命令层调用已按现状 optional chaining no-op(§顶部「视频动作透传」注释)。 -->
            <template v-else-if="detail.mediaType === 'video'">
              <VideoPlayer
                v-if="videoSource.src.value"
                ref="videoRef"
                :src="videoSource.src.value!"
                :poster="videoPoster"
                :transform="state.transform.value"
                :dragging="state.isDragging.value"
                :item-id="detail.id"
                :file-format="detail.fileFormat"
                :file-name="detail.fileName"
                :playback-position-ms="detail.playbackPositionMs"
                :video-meta="detail.videoMeta"
                :intrinsic-width="detail.width"
                :intrinsic-height="detail.height"
                :resolve-verdict="videoSource.verdict.value"
                :toolbar-hidden="controlsHidden"
                @loadedmetadata="updateZoomRatio"
                @toggle-toolbar="toggleControls"
              />
              <VideoPreparingOverlay
                v-if="videoOverlayMode"
                :mode="videoOverlayMode!"
                :percent="videoSource.percent.value"
                :stage="videoSource.stage.value"
                :estimate-bytes="videoSource.estimateBytes.value"
                :error-code="videoSource.errorCode.value"
                :downloading="videoSource.downloadingComponent.value"
                @cancel="videoSource.cancel()"
                @confirm="videoSource.confirm()"
                @download="videoSource.downloadComponent()"
                @retry="videoSource.retry()"
                @transcode-instead="videoSource.confirm()"
              />
            </template>
            <audio
              v-else-if="detail.mediaType === 'audio'"
              :src="absPath"
              class="detail-viewer__audio"
              controls
              autoplay
              @error="onMediaError"
            />
            <div v-else class="detail-viewer__document">
              <FileText :size="48" />
              <p>{{ detail.fileName }}</p>
            </div>
          </template>

          <!-- Live photo 视频覆盖层 -->
          <video
            v-if="state.isPlayingLive.value && state.liveVideoSrc.value"
            :src="state.liveVideoSrc.value"
            class="detail-viewer__live-video"
            autoplay
            loop
          />

          <!-- 人脸框（F6）：投影到图像内容矩形；pointer-events:none，绝不拦截缩放/拖拽点击。 -->
          <!-- 旋转态隐藏人脸框(P5):bbox 投影假设图未旋转,旋转下 getBoundingClientRect 含旋转
               会致框错位,故仅未旋转时显示(诚实边界,非缺陷)。 -->
          <div
            v-if="
              detail.mediaType === 'image' &&
              faces.length &&
              showFaces &&
              state.rotation.value % 360 === 0
            "
            class="face-overlay"
          >
            <div v-for="f in faces" :key="f.id" class="face-box" :style="faceBoxStyle(f)">
              <span v-if="f.personName" class="face-box__label">{{ f.personName }}</span>
            </div>
          </div>

          <!-- OCR 结果面板(T9):图像/视频两态共用同一 useOcr 单例状态,面板挂载点固定在此。 -->
          <OcrResultPanel />

          <!-- 底部操作栏显隐入口(2026-07-23 二迁):右上玻璃圆钮反操作直觉,已撤——视频态迁入
               播放控制条(VideoControlBar 切换钮,经 toggle-toolbar 事件回本组件 toggleControls);
               图像态保持点大图切换(onViewerClick)。 -->
        </div>

        <!-- ── 控制器 ────────────────────────────────────────────────── -->
        <!-- 沉浸模式(P4-c)隐藏本控制条(app shell 由 AppShell 联动隐);退出走浮动按钮 / Esc。
             controlsHidden(2026-07-18):点大图切换底部半透明操作栏显隐(与沉浸互不相干——沉浸连 app
             shell 一起隐,这里只隐本条),偏好存 config.toml(detail_controls_hidden)。 -->
        <div class="detail-controls" v-show="detailControlsVisible">
          <!-- 左侧 -->
          <div class="detail-controls__left">
            <UiIconButton :label="$t('detail.zoomOut')" @click="state.zoomOut()">
              <ZoomOut :size="18" />
            </UiIconButton>
            <span class="zoom-percentage" :class="{ 'zoom-highlight': isZoomChanged }">
              {{ Math.round(state.scale.value * zoomRatio * 100) }}%
            </span>
            <UiIconButton :label="$t('detail.zoomIn')" @click="state.zoomIn()">
              <ZoomIn :size="18" />
            </UiIconButton>
            <span
              v-if="media.navContext && media.navContext.currentIndex !== null"
              class="zoom-percentage"
              style="opacity: 0.8; margin-left: 8px"
            >
              {{ media.navContext.currentIndex + 1 }} /
              {{
                media.navContext.type === 'lens'
                  ? media.navContext.totalCount
                  : media.navContext.itemIds.length
              }}
            </span>
            <UiIconButton :label="zoomModeTitle" @click="handleToggleZoom">
              <Maximize v-if="state.zoomMode.value === 'auto'" :size="18" />
              <span
                v-else-if="state.zoomMode.value === 'original'"
                style="font-size: 12px; font-weight: 700"
                >1:1</span
              >
              <MoveHorizontal v-else-if="state.zoomMode.value === 'fit-width'" :size="18" />
              <MoveVertical v-else :size="18" />
            </UiIconButton>
            <!-- 旋转(P5):顺时针 90°,图/视可用；连续查看微操只在底栏保留一个可见入口。
                 单向累进(2026-07-18):按钮上叠当前归一角度(90°/180°/270°),已旋转时高亮 active——
                 让「当前朝向」一眼可见,不必靠转回 0 才知回到原位。 -->
            <UiIconButton
              v-if="detail.mediaType === 'image' || detail.mediaType === 'video'"
              :label="rotateLabel"
              :active="rotationDeg !== 0"
              class="detail-controls__rotate"
              @click="handleRotate"
            >
              <RotateCw :size="18" />
              <span v-if="rotationDeg !== 0" class="detail-controls__rotate-badge">{{
                `${rotationDeg}°`
              }}</span>
            </UiIconButton>
            <UiIconButton
              v-if="detail.isLivePhoto"
              :label="t('detail.livePhoto')"
              :active="state.isPlayingLive.value"
              @click="toggleLive"
            >
              LIVE
            </UiIconButton>
            <!-- 图片简单编辑入口(方案 C §7):仅静态图片;不支持的格式(GIF/HEIC/AVIF 等)
                 点击给出明确不支持提示,而非悄悄隐藏按钮(方案「显式不支持文案」)。 -->
            <UiIconButton
              v-if="detail.mediaType === 'image'"
              :label="t('edit.entry')"
              @click="openEditor"
            >
              <PencilLine :size="18" />
            </UiIconButton>
            <!-- OCR 文字提取入口(D-OCR-6):按钮常显,未授权/未装模型点击后由 useOcr 引导跳转,
                 不隐藏按钮(与编辑按钮「显式不支持文案」哲学一致)。 -->
            <UiIconButton
              v-if="detail.mediaType === 'image'"
              :label="t('ocr.entry')"
              :disabled="ocr.busy.value"
              @click="onOcrImage"
            >
              <ScanText :size="18" />
            </UiIconButton>
            <!-- 影像增强入口（P0 批 5）：仅静态图片；按钮常显，门控/引导在 EnhanceDialog 内闭环。 -->
            <UiIconButton
              v-if="detail.mediaType === 'image'"
              :label="t('enhance.entry')"
              @click="openEnhance"
            >
              <Sparkles :size="18" />
            </UiIconButton>
            <!-- 渲染色域切换(D-414):微操归左组;移动端锁 sRGB 不出入口,视频不支持渲染色域切换不出。 -->
            <ViewerColorMenu v-if="detail.mediaType === 'image' && !isMobilePlatform" />
          </div>

          <!-- 中间: 文件名 -->
          <div class="detail-controls__center">
            <span class="detail-controls__name" :title="detail.fileName">{{
              detail.fileName
            }}</span>
          </div>

          <!-- 右侧 -->
          <div class="detail-controls__right">
            <!-- 人脸蓝框显隐开关（问题5）：仅图像且检测到脸时出现，默认显示，偏好存 config.toml。 -->
            <UiIconButton
              v-if="detail.mediaType === 'image' && faces.length"
              :label="showFaces ? t('detail.hideFaceBoxes') : t('detail.showFaceBoxes')"
              :active="showFaces"
              @click="toggleFaces"
            >
              <ScanFace :size="18" />
            </UiIconButton>
            <UiIconButton
              :label="t('selection.favorite')"
              :active="detail.isFavorited"
              @click="toggleFav"
            >
              <Heart
                :size="18"
                :fill="detail.isFavorited ? 'currentColor' : 'none'"
                :stroke-width="detail.isFavorited ? 0 : 2"
              />
            </UiIconButton>
            <!-- 移动端 opener 不支持 reveal(D-001/R-09):动作整个不出。 -->
            <UiIconButton
              v-if="!isMobilePlatform"
              :label="t('contextMenu.showInExplorer')"
              @click="showInExplorer"
            >
              <FolderOpen :size="18" />
            </UiIconButton>
            <UiIconButton
              :label="$t('detail.info')"
              :active="ui.viewerInfoVisible"
              @click="ui.toggleViewerInfo()"
            >
              <Info :size="18" />
            </UiIconButton>
            <!-- 关闭（2026-07-16 真机「大图浏览器没有×按钮，只能通过ESC退出」）。此前退出命令
                 viewer.close 一直注册着（commands/builtins/viewer-image.ts），但只以小 ArrowLeft
                 图标待在自绘标题栏里——F11 沉浸态下标题栏藏起来，该路径恰在最需要它时不可见，
                 只剩 Esc。故底部控制条补一个 ×：全幅黑色表面上唯一常驻可见的退出入口，与标题栏
                 互补而非重复（同「查看器保留局部控制」方案 C）。置于最右并加分隔线，
                 防误当成又一个开关。 -->
            <span class="detail-controls__sep"></span>
            <UiIconButton :label="t('common.close')" @click="close">
              <X :size="18" />
            </UiIconButton>
          </div>
        </div>

        <!-- 沉浸模式浮动退出按钮(P4-c):控制条隐藏时提供退出入口(Esc 亦可)。 -->
        <button
          v-if="isImmersive"
          class="content-viewer__immersive-exit"
          @click="toggleImmersive"
          :title="t('detail.exitImmersive')"

        >
          <Minimize2 :size="20" />
        </button>

        <!-- ── 文件信息面板(停靠挤压式,ui.viewerInfoVisible 驱动;面板 padding 让出图片区)── -->
        <Transition name="slide">
          <div v-if="ui.viewerInfoVisible" class="detail-info">
            <div class="detail-info__header">
              <span>{{ t('detail.fileInfo') }}</span>
              <UiIconButton :label="$t('common.close')" @click="ui.toggleViewerInfo()">
                <X :size="16" />
              </UiIconButton>
            </div>

            <div class="info-section">
              <div class="info-row">
                <span class="info-label">{{ $t('detail.fileName') }}</span>
                <span class="info-value" :title="detail.fileName">{{ detail.fileName }}</span>
              </div>
              <div class="info-row">
                <span class="info-label">{{ $t('detail.fileSize') }}</span>
                <span class="info-value">{{ formatFileSize(detail.fileSize) }}</span>
              </div>
              <div class="info-row">
                <span class="info-label">{{ $t('detail.dimensions') }}</span>
                <span class="info-value" v-if="detail.width"
                  >{{ detail.width }} × {{ detail.height }}</span
                >
              </div>
              <div class="info-row">
                <span class="info-label">{{ $t('detail.format') }}</span>
                <span class="info-value">{{ detail.fileFormat.toUpperCase() }}</span>
              </div>
              <div
                class="info-row"
                style="flex-direction: column; align-items: flex-start; gap: 4px"
              >
                <span class="info-label">{{ t('detail.fullPath') }}</span>
                <span
                  class="info-value clickable-path"
                  :title="detail.absPath"
                  @click.stop.prevent="showInExplorer"
                  >{{ detail.absPath }}</span
                >
              </div>
            </div>

            <!-- EXIF -->
            <div v-if="detail.imageMeta" class="info-section">
              <div class="info-section__title">{{ $t('detail.exif') }}</div>
              <div v-if="detail.imageMeta.exifDatetime" class="info-row">
                <span class="info-label">{{ $t('detail.datetime') }}</span>
                <span class="info-value">{{ formatDateTime(detail.imageMeta.exifDatetime) }}</span>
              </div>
              <div v-if="detail.imageMeta.exifMake" class="info-row">
                <span class="info-label">{{ $t('detail.camera') }}</span>
                <span class="info-value"
                  >{{ detail.imageMeta.exifMake }} {{ detail.imageMeta.exifModel }}</span
                >
              </div>
              <div v-if="detail.imageMeta.exifFocalLength" class="info-row">
                <span class="info-label">{{ $t('detail.focalLength') }}</span>
                <span class="info-value">{{
                  formatFocalLength(detail.imageMeta.exifFocalLength)
                }}</span>
              </div>
              <div v-if="detail.imageMeta.exifAperture" class="info-row">
                <span class="info-label">{{ $t('detail.aperture') }}</span>
                <span class="info-value">{{ formatAperture(detail.imageMeta.exifAperture) }}</span>
              </div>
              <div v-if="detail.imageMeta.exifShutter" class="info-row">
                <span class="info-label">{{ $t('detail.exposure') }}</span>
                <span class="info-value">{{ detail.imageMeta.exifShutter }}s</span>
              </div>
              <div v-if="detail.imageMeta.exifIso" class="info-row">
                <span class="info-label">{{ $t('detail.iso') }}</span>
                <span class="info-value">{{ detail.imageMeta.exifIso }}</span>
              </div>
              <div v-if="detail.imageMeta.exifGpsLat" class="info-row">
                <span class="info-label">{{ $t('detail.location') }}</span>
                <span class="info-value">{{
                  formatGps(detail.imageMeta.exifGpsLat, detail.imageMeta.exifGpsLng!)
                }}</span>
              </div>
            </div>

            <!-- 评分 -->
            <div class="info-section">
              <div class="info-section__title">{{ t('detail.rating') }}</div>
              <div class="rating-stars">
                <button
                  v-for="n in 5"
                  :key="n"
                  class="star"
                  :class="{ filled: n <= (detail.rating ?? 0) }"

                  @click="setRating(n)"
                >
                  <Star
                    :size="20"
                    :fill="n <= (detail.rating ?? 0) ? 'currentColor' : 'none'"
                    :stroke-width="1.5"
                  />
                </button>
              </div>
            </div>

            <!-- 颜色标签：复用 ColorLabelPicker（点设/点当前清零），与工具栏批量设色、筛选同一控件。 -->
            <div class="info-section">
              <div class="info-section__title">{{ t('detail.colorLabel') }}</div>
              <ColorLabelPicker
                :model-value="detail.colorLabel ?? 0"
                :size="20"
                @change="setColorLabel"
              />
            </div>
          </div>
        </Transition>
      </div>
    </div>
  </div>

  <ContextMenu
    :visible="ctxMenu.visible"
    :x="ctxMenu.x"
    :y="ctxMenu.y"
    :items="ctxMenu.items"
    @update:visible="ctxMenu.visible = $event"
  />

  <FolderTreeSelectorDialog
    v-if="moveCopyDialog.isOpen"
    :title="moveCopyDialog.mode === 'move' ? t('common.moveToFolder') : t('common.copyToFolder')"
    @close="moveCopyDialog.isOpen = false"
    @confirm="onMoveCopyConfirm"
  />

  <!-- Exotic 激活对话框（Part5 T12）：gate 的「已购买？激活」入口 → 输入 token → 后端验签。 -->
  <ExoticActivateDialog
    :open="activateOpen"
    :plugin-id="exoticGate.entitlement.value?.pluginId ?? ''"
    :feature-name="exoticFeatureName"
    @close="activateOpen = false"
    @activated="onExoticActivated"
  />

  <!-- 编辑是编译进 Host 的付费内建 feature：入口保持可见，未授权时复用 PluginGate 购买/激活形态。 -->
  <UiDialog
    :open="editGateOpen"
    :title="t('edit.premiumName')"
    :close-label="t('common.close')"
    max-width="520px"
    @close="editGateOpen = false"
  >
    <PluginGate
      :entitlement="editingGate.entitlement.value"
      :loading="editingGate.loading.value"
      :feature-name="t('edit.premiumName')"
      :feature-desc="t('edit.premiumDesc')"
      @activate="openEditingActivation"
    />
  </UiDialog>
  <ExoticActivateDialog
    :open="editActivateOpen"
    :plugin-id="editingGate.entitlement.value?.pluginId ?? 'feature-editing'"
    :feature-name="t('edit.premiumName')"
    :activation-handler="editingGate.activate"
    @close="onEditingActivationClosed"
    @activated="onEditingActivated"
  />

  <!-- 影像增强对话框（P0 批 5）：单图入口触发。 -->
  <EnhanceDialog :open="enhanceOpen" :source="enhanceSource" @close="enhanceOpen = false" />
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { useTauriListen } from '../../composables/useTauriListen'
import { resolveAssetUrl } from '../../utils/assetUrl'
import { isMobilePlatform } from '../../utils/platform'
import { useI18n } from 'vue-i18n'
import ContextMenu from '../common/ContextMenu.vue'
import ColorLabelPicker from '../common/ColorLabelPicker.vue'
import FolderTreeSelectorDialog from '../common/FolderTreeSelectorDialog.vue'
import PluginGate from '../exotic/PluginGate.vue'
import ExoticActivateDialog from '../exotic/ExoticActivateDialog.vue'
import UiIconButton from '../ui/UiIconButton.vue'
import UiDialog from '../ui/UiDialog.vue'
import EditOverlay from './EditOverlay.vue'
import VideoPlayer from './player/VideoPlayer.vue'
import VideoPreparingOverlay from './player/VideoPreparingOverlay.vue'
import { useVideoSource, type VideoSourceMode } from '../../composables/player/useVideoSource'
import OcrResultPanel from './OcrResultPanel.vue'
import ViewerColorMenu from './ViewerColorMenu.vue'
import EnhanceDialog from '../enhance/EnhanceDialog.vue'
import type { EnhanceSource } from '../../types/enhance'
import { useImageEditor, requestDiscardEdits } from '../../composables/useImageEditor'
import { useViewerColorSource } from '../../composables/useViewerColorSource'
import { useEditingEntitlement } from '../../composables/useEditingEntitlement'
import { useExoticGate } from '../../composables/useExoticGate'
import { readSettingBool } from '../../composables/settingsValues'
import { writeSettings } from '../../stores/settingsPersistence'
import { useOcr } from '../../composables/useOcr'
import { gateModeFor } from '../../composables/usePluginEntitlement'
import { usePersonStore } from '../../stores/personStore'
import { useMediaStore } from '../../stores/mediaStore'
import { useDuplicateLensStore } from '../../stores/duplicateLensStore'
import { useToastStore } from '../../stores/toastStore'
import { useHistoryStore } from '../../stores/historyStore'
import { useUiStore } from '../../stores/uiStore'
import { useConfigStore } from '../../stores/configStore'
import { useMediaDetail } from '../../composables/useMediaDetail'
// 顶栏重构 P4-b:路由驱动 + activeViewer 单源。route 提供 :id 与 back,viewerStore 承载顶栏上下文。
import { useRoute, useRouter } from 'vue-router'
import {
  useViewerStore,
  toViewerFileInfo,
  type ActiveViewer,
  type ViewerApi,
} from '../../stores/viewerStore'
import { resolveViewerKind } from '../../utils/viewerKind'
import { isRawFormat } from './mediaGrid.helpers'
import {
  formatFileSize,
  formatDateTime,
  formatFocalLength,
  formatAperture,
  formatGps,
} from '../../utils/format'
import {
  X,
  ZoomIn,
  ZoomOut,
  Maximize,
  Minimize2,
  RotateCw,
  MoveHorizontal,
  MoveVertical,
  Heart,
  FolderOpen,
  Info,
  Star,
  FileText,
  ScanFace,
  ImageOff,
  PencilLine,
  ScanText,
  Sparkles,
} from '@lucide/vue'
import { EVENTS } from '../../constants/ipc'
// 超长文件拆分方案(2026-07-25 分析/analysis/ContentViewer-vue.md §2.1)下沉的 composable:
// 只接受 getter/ref 参数,不反向 import 组件;不得在内部重复调用已在本文件创建一次的
// useMediaDetail() 等单例状态源。
import { useContentViewerPosterSource } from '../../composables/useContentViewerPosterSource'
import { useContentViewerPreload } from '../../composables/useContentViewerPreload'
import { useContentViewerRouteNav } from '../../composables/useContentViewerRouteNav'
import { useContentViewerZoomRotation } from '../../composables/useContentViewerZoomRotation'
import { useContentViewerFaces } from '../../composables/useContentViewerFaces'
import { useContentViewerKeyboard } from '../../composables/useContentViewerKeyboard'
import { useContentViewerQuickActions } from '../../composables/useContentViewerQuickActions'
import { useContentViewerContextMenu } from '../../composables/useContentViewerContextMenu'

const media = useMediaStore()
const duplicateLens = useDuplicateLensStore()
const person = usePersonStore()
const toast = useToastStore()
const history = useHistoryStore()
const ui = useUiStore()
const config = useConfigStore()
const route = useRoute()
const router = useRouter()
const viewer = useViewerStore()
const { t } = useI18n()
// 图片简单编辑(方案 C §7):单实例贯穿本组件生命周期,EditOverlay 只呈现、不持有状态。
const editor = useImageEditor()
const editingGate = useEditingEntitlement()
const editGateOpen = ref(false)
const editActivateOpen = ref(false)
let editingActivationSucceeded = false
// OCR 文字提取(T9):模块级单例,与 VideoPlayer 内的 useOcr() 调用共享同一份 busy/panelOpen/result。
const ocr = useOcr()
// 影像增强(P0 批 5):对话框开阖 + 源信息(单图入口;多选入口若注册点在 MediaGrid.vue(userWIP)则不加)。
const enhanceOpen = ref(false)

const detail = computed(() => media.detailItem!)

// ── 查看器渲染色域换源(2026-07-23 自定义 ICC 与色域切换,方案 B §0⑥)──────────────────────
// 「原图先显 + 完成后换源」:absPath 优先取派生色域文件 URL,未就绪/不适用(srgb/移动端/非
// image)时回落原图,与 D-413(编辑链 sRGB 不动,EditOverlay 吃原图路径不经 absPath)互不影响。
const viewerColor = useViewerColorSource({
  detail: () =>
    media.detailItem ? { id: media.detailItem.id, mediaType: media.detailItem.mediaType } : null,
  target: () => config.viewerColorTarget,
  customId: () => config.viewerColorCustomId,
  isMobile: isMobilePlatform,
})
const originalAssetUrl = computed(() =>
  detail.value ? resolveAssetUrl(detail.value.absPath) : '',
)
const absPath = computed(() => viewerColor.displayUrl.value ?? originalAssetUrl.value)

// ── 图片加载占位 / 视频海报帧(下沉:useContentViewerPosterSource,含「封面 404 自愈」探测逻辑)──
const { thumbCacheDir, imagePlaceholder, onImagePlaceholderError, videoPoster } =
  useContentViewerPosterSource({
    detail: () => detail.value,
  })

const isBursting = ref(false)
const placeholderImgRef = ref<HTMLImageElement | null>(null)
const isImageLoaded = ref(false)

watch(
  () => detail.value?.id,
  () => {
    isImageLoaded.value = false
  },
)

// ── 邻近预加载与极速飞掠保护（Burst Mode） ───────────────────────────────────
const preload = useContentViewerPreload({
  detail: () => detail.value,
  media,
  thumbCacheDir: () => thumbCacheDir.value,
  isHighResReady: () => isImageLoaded.value,
  isBursting,
  isLensActive: () => duplicateLens.isLensActive,
})

const imageBoxStyle = computed(() => {
  const w = detail.value?.width
  const h = detail.value?.height
  return {
    aspectRatio: w && h ? `${w} / ${h}` : undefined,
    maxWidth: w ? `min(100%, ${w}px)` : '100%',
    maxHeight: h ? `min(100%, ${h}px)` : '100%',
    transform: state.transform.value,
  }
})


// ── 视频格式扩展子系统 · 播放链路(design.md §5.3)─────────────────────────────
// 只在查看视频项时给出非 null itemId;composable 内部 watch 该 getter,查看项切换时自动
// 复位状态并重新解析(切图/切视频/退出视频查看都经这一条路径,不需要本组件额外处理)。
const videoSource = useVideoSource(() =>
  detail.value?.mediaType === 'video' ? detail.value.id : null,
)
// direct/derived 两态由 VideoPlayer 自己吃 src;其余态才挂引导覆盖层(idle=尚未解析完成,
// 不闪一次多余覆盖层)。
const videoOverlayMode = computed<VideoSourceMode | null>(() => {
  if (detail.value?.mediaType !== 'video') return null
  const m = videoSource.mode.value
  return m === 'direct' || m === 'derived' || m === 'idle' ? null : m
})

// ── 不可用态（缺失检测 Part2 §3.2）──────────────────────────────────────────
// 卷离线 / 文件缺失 → 直接出明确提示，不尝试加载注定失败的源；
// availability='online' 但加载仍失败（文件被外部移动/删除而未重扫）→ @error 兜底。
const loadError = ref(false)
// 切换查看项时复位加载失败标志（否则上一张的失败会污染下一张）。
watch(
  () => media.detailItem?.id,
  () => {
    loadError.value = false
  },
)

const isUnavailable = computed(
  () =>
    (detail.value?.availability && detail.value.availability !== 'online') ||
    loadError.value,
)

const unavailableInfo = computed<{ title: string; hint: string }>(() => {
  const a = detail.value?.availability
  if (a === 'offline') {
    return { title: t('detail.unavailableOfflineTitle'), hint: t('detail.unavailableOfflineHint') }
  }
  if (a === 'missing') {
    return { title: t('detail.unavailableMissingTitle'), hint: t('detail.unavailableMissingHint') }
  }
  // RAW 相机原片(已注册、预览未接入解码):文件本身完好、只是查看器渲染不了原始编码——
  // 与「文件被移走/删除」是两回事，须区别于下方通用错误文案（RAW 半成品体验修复线）。
  if (detail.value && isRawFormat(detail.value.fileFormat)) {
    return { title: t('detail.unavailableRawTitle'), hint: t('detail.unavailableRawHint') }
  }
  // online 但加载失败：文件可能被外部移动/删除而尚未重扫。
  return { title: t('detail.unavailableErrorTitle'), hint: t('detail.unavailableErrorHint') }
})

function onMediaError() {
  // 图片错误由 useViewerImageSource 在不可见候选层判定；此入口只服务 audio 等原生媒体。
  loadError.value = true
}

function onImgLoad(event: Event) {
  const image = event.currentTarget as HTMLImageElement
  // 快速连翻时旧 DOM load 可能迟到；只有当前条目已提交 URL 才能驱动缩放/旋转复原。
  if (image.getAttribute('src') !== absPath.value) {
    return
  }
  isImageLoaded.value = true
  updateZoomRatio()
}

function onVisibleImageError(event: Event) {
  const image = event.currentTarget as HTMLImageElement
  if (image.getAttribute('src') !== absPath.value) return
  if (viewerColor.handleDisplayUrlError()) return
  loadError.value = true
}

// ── Exotic 授权 gate（Part5 T12 增量3）───────────────────────────────────────
// 打开某项时解析其 exotic 授权态；未授权（purchase/blocked）→ 视图区显 PluginGate 引导，
// 而非渲染注定失败的原图。普通格式（非 exotic catalog）resolveForItem 直接放行、不发 IPC。
const exoticGate = useExoticGate()
const activateOpen = ref(false)

// gate 只在「有产品未授权」或「纯不可用」时接管视图；已授权 / 放行走原渲染。
const showExoticGate = computed(() => {
  const m = gateModeFor(exoticGate.entitlement.value)
  return m === 'purchase' || m === 'blocked'
})
// 传给 gate / 激活对话框的功能名：用格式名（如 PSD）给出上下文；gate 无名时会退回通用标题。
const exoticFeatureName = computed(() =>
  detail.value ? detail.value.fileFormat.toUpperCase() : '',
)

// 查看项变化即重解析（immediate 覆盖「详情已开时组件才挂载」的情形）。
// 关闭 / 无项时清态，避免上一项的 gate 残留污染下一项。
watch(
  () => media.detailItem?.id,
  (id) => {
    if (!id || !media.detailItem) {
      exoticGate.reset()
      return
    }
    // 失败/普通格式内部已置 entitlement=null（放行），无需在此 try/catch。
    void exoticGate.resolveForItem(id, media.detailItem.fileFormat)
  },
  { immediate: true },
)

// 激活成功 → 重解析授权态（转 Authorized 后 showExoticGate 归 false，gate 自动撤下）。
async function onExoticActivated() {
  const item = media.detailItem
  if (item) await exoticGate.resolveForItem(item.id, item.fileFormat)
}

// ── 视图器状态 — 仅创建一次，不要放在 computed() 内部 ─────────────────────
// 如果在 computed() 内部调用 useMediaDetail()，每次响应式依赖变化时都会重新创建内部的 refs
// 并重新注册 document 事件监听器，从而导致 mousemove/mouseup 处理程序永久泄漏。
const state = useMediaDetail()

const viewerRef = ref<HTMLElement | null>(null)
const imgRef = ref<HTMLImageElement | null>(null)
// videoRef 现为 VideoPlayer 组件实例(GC):真实 <video> 元素经其 expose 的 videoEl 取,
// 尺寸/缩放读它;播放动作经 viewerApi 的视频方法透传(命令层 GD 批)。
const videoRef = ref<InstanceType<typeof VideoPlayer> | null>(null)

const isDisplayedImageCurrent = computed(() => isImageLoaded.value)

function currentImageElement(): HTMLImageElement | null {
  const image = imgRef.value
  if (
    image &&
    isDisplayedImageCurrent.value &&
    image.getAttribute('src') === absPath.value
  ) {
    return image
  }
  return placeholderImgRef.value ?? null
}

const zoomModeTitle = computed(() => {
  switch (state.zoomMode.value) {
    case 'auto':
      return t('detail.zoomModeAuto')
    case 'original':
      return t('detail.zoomModeOriginal')
    case 'fit-width':
      return t('detail.zoomModeFitWidth')
    case 'fit-height':
      return t('detail.zoomModeFitHeight')
    default:
      return t('detail.resetZoom')
  }
})

// ── 路由驱动翻页(下沉:useContentViewerRouteNav)。close/closeViewer 跨多域引用,留本组件。 ──
const { routeId, loadFromRoute, navigate } = useContentViewerRouteNav({
  media,
  editor,
  router,
  route,
  onNavigate: (offset) => preload.markNavigation(offset),
  getCachedAdjacent: (id, offset) => preload.getCachedAdjacent(id, offset),
  isLensActive: () => duplicateLens.isLensActive,
})

// ── 缩放比例 + 旋转(下沉:useContentViewerZoomRotation) / 人脸框(下沉:useContentViewerFaces) ──
// 两者原共用一个「查看项变化即复位」watch,且 updateZoomRatio 内联调 recomputeFaceLayout——拆分后
// 用 onSizeReady 回调 + facesRecompute 前向引用把调用序原样串起来(风险章节红线,不得丢失)。
let facesRecompute: (() => void) | null = null
const {
  zoomRatio,
  updateZoomRatio,
  isZoomChanged,
  rotationDeg,
  rotateLabel,
  handleRotate,
  handleToggleZoom,
} = useContentViewerZoomRotation({
  state,
  viewerRef,
  currentImageElement,
  videoRef,
  detail: () => detail.value,
  media,
  t,
  onSizeReady: () => facesRecompute?.(),
})
const { faces, showFaces, toggleFaces, faceBoxStyle, loadFacesFor, recomputeFaceLayout } =
  useContentViewerFaces({
    viewerRef,
    currentImageElement,
    transform: () => state.transform.value,
    personStore: person,
  })
facesRecompute = recomputeFaceLayout

// 查看项变化 → 重新拉取人脸(原与缩放/旋转复原同属一个 watch;本 watch 注册于
// useContentViewerZoomRotation 之后,保持「先复位缩放、再拉人脸」的原调用序)。
watch(
  () => media.detailItem,
  (item) => {
    loadFacesFor(item ? { id: item.id, mediaType: item.mediaType } : null)
  },
)

// 视图区尺寸变化 → 重算缩放基准与人脸框投影。信息面板停靠挤压(padding 动画逐帧改宽)、app 侧栏
// 开合、窗口缩放都改变视图区宽度;RO 是覆盖三者的单一信号(替代原「切信息面板 nextTick 一次」——
// 那对停靠动画会算在动画起始的旧宽上,末态错位)。RO 逐帧触发即让人脸框/缩放百分比全程跟随。
let viewerRO: ResizeObserver | null = null
if (typeof ResizeObserver !== 'undefined') {
  viewerRO = new ResizeObserver(() => updateZoomRatio())
}
watch(viewerRef, (el, prev) => {
  if (prev) viewerRO?.unobserve(prev)
  if (el) viewerRO?.observe(el)
})

// ── 快捷操作(下沉:useContentViewerQuickActions):收藏/评分/色标/资源管理器定位/Live Photo ──
const { toggleFav, setRating, setColorLabel, showInExplorer, toggleLive } =
  useContentViewerQuickActions({ detail: () => detail.value, media, toast, t, state })

// ── 右键菜单 + 移动/复制对话框(下沉:useContentViewerContextMenu) ──────────────────────────
const { ctxMenu, moveCopyDialog, onContextMenu, onMoveCopyConfirm } = useContentViewerContextMenu({
  detail: () => detail.value,
  media,
  history,
  toast,
  t,
  navigate,
  close,
})

// ── 图片简单编辑(方案 C §7)入口/收尾 ──────────────────────────────────────────
async function openEditor(): Promise<void> {
  if (!detail.value) return
  editGateOpen.value = true
  const entitlement = await editingGate.fetchEntitlement()
  if (entitlement.availability === 'authorized' && detail.value) {
    editGateOpen.value = false
    editor.open(detail.value)
  }
}

/** OCR 文字提取入口(T9):门控/toast/面板全在 useOcr 内闭环。 */
function onOcrImage(): void {
  if (!detail.value) return
  void ocr.extractFromImage(detail.value.id, detail.value.fileName)
}

// 影像增强源信息(P0 批 5):单图;ext/尺寸供「自动」建议与输出尺寸预估,拿不到即跳过对应逻辑。
const enhanceSource = computed<EnhanceSource | null>(() => {
  const d = detail.value
  if (!d || d.mediaType !== 'image') return null
  const ext = d.fileName.split('.').pop()?.toLowerCase()
  return {
    itemIds: [d.id],
    fileName: d.fileName,
    ext,
    width: d.width ?? undefined,
    height: d.height ?? undefined,
  }
})

/** 影像增强入口(P0 批 5):门控/引导/参数全在 EnhanceDialog 内闭环。 */
function openEnhance(): void {
  if (!detail.value) return
  enhanceOpen.value = true
}

// 深审裁决:切项(翻页/换看图)必须收口旧 OCR 结果面板,不得带着上一项的文字残留到新项。
watch(
  () => detail.value?.id,
  () => ocr.closePanel(),
)

function openEditingActivation(): void {
  editingActivationSucceeded = false
  editGateOpen.value = false
  editActivateOpen.value = true
}

function onEditingActivationClosed(): void {
  editActivateOpen.value = false
  if (!editingActivationSucceeded) editGateOpen.value = true
}

async function onEditingActivated(): Promise<void> {
  editingActivationSucceeded = true
  const entitlement = await editingGate.fetchEntitlement()
  if (entitlement.availability === 'authorized' && detail.value) editor.open(detail.value)
}

/** EditOverlay 取消/关闭(已内部走过「有改动则二次确认」)。 */
function onEditorClosed(): void {
  editor.close()
}

/** 保存成功:跳转新 item(与 navigate() 同款路由同步),关闭编辑覆层。 */
async function onEditorSaved(newItemId: number): Promise<void> {
  editor.close()
  await media.openDetail(newItemId, true)
  void router.replace(`/view/${newItemId}`)
  toast.addToast('success', t('edit.save'))
}

// ── 键盘快捷键(下沉:useContentViewerKeyboard,自带 mounted/unmounted 生命周期) ──────────────
useContentViewerKeyboard({
  media,
  editor,
  viewer,
  videoRef,
  detail: () => detail.value,
  close,
})

let accumulatedDelta = 0
let lastWheelNavTime = 0
let deltaTimer: ReturnType<typeof setTimeout> | null = null

function onWheelHandler(e: WheelEvent) {
  if (editor.status.value !== 'idle') return // 编辑中禁翻页(方案 §7),含滚轮翻页
  const handledZoom = state.onWheel(e)

  if (handledZoom !== true) {
    // 滚轮单位归一化（行 / 页 / 像素模式）
    const rawDelta =
      e.deltaMode === 1 ? e.deltaY * 33 : e.deltaMode === 2 ? e.deltaY * 100 : e.deltaY
    accumulatedDelta += rawDelta

    // 停止滚动 60ms 后清空累加器，避免跨手势残留
    if (deltaTimer) clearTimeout(deltaTimer)
    deltaTimer = setTimeout(() => {
      accumulatedDelta = 0
    }, 60)

    const now = Date.now()
    const timeSinceLast = now - lastWheelNavTime

    // 单格滚轮（delta >= 30）立即 0 延迟响应，连续飞转滚轮保持 70ms 舒适节拍
    if (Math.abs(accumulatedDelta) >= 30 && timeSinceLast >= 70) {
      const dir = accumulatedDelta > 0 ? 1 : -1
      accumulatedDelta = 0
      lastWheelNavTime = now
      void navigate(dir)
    }
  }
}

// 卷插拔监听（T13 §3.7 离线 UX 验收点「重连自动恢复」）：查看器打开时若卷重连，
// 刷新当前项可用态 → isUnavailable 归 false → 原图自动加载，无需用户手动关开。
// useTauriListen 处理 await 落定前卸载的竞态并在作用域销毁时解绑(P1-11)。
useTauriListen(EVENTS.VOLUMES_CHANGED, () => {
  if (media.detailItem) void media.refreshDetailAvailability()
})

function closeViewer() {
  // 离开查看器回来处(网格);P4-a 已让网格重挂载恢复滚动位。无 back 记录(未来 OS 深链直达)回退根。
  if (router.options.history.state.back != null) router.back()
  else void router.push('/')
}

/** 顶栏/命令层的「返回画廊」统一入口:编辑中先走「有改动二次确认」,确认放弃后才真正离开。 */
function close() {
  if (editor.status.value === 'idle') {
    closeViewer()
    return
  }
  if (editor.status.value === 'saving') return
  void (async () => {
    if (await requestDiscardEdits(editor)) {
      editor.close()
      closeViewer()
    }
  })()
}

// ── 沉浸模式(顶栏重构 P4-c)──────────────────────────────────────────────────
// 图片沉浸 = 经 viewerStore 隐藏 app shell(侧栏/标题栏/工具栏/状态栏,AppShell 按 isImmersive
// 联动)+ 隐本组件底部控制条,只留全屏图;退出=浮动按钮 / Esc(见 onKeydown)。离开查看器时
// viewer.clear 使 activeViewer=null → isImmersive 自然归假,无需手动复位。阅读器暂留其局部沉浸,
// P5 令 DocumentViewer 也 populate viewerStore 后统一(现两者机制并存)。
const isImmersive = computed(() => viewer.isImmersive)
/** 底部操作栏显隐设置键(后端 schema 注册,全局查看偏好)。 */
const CONTROLS_HIDDEN_KEY = 'detail_controls_hidden'
function toggleImmersive() {
  viewer.setImmersive(!viewer.isImmersive)
}

// ── 点大图切换底部操作栏显隐(2026-07-18 内容页需求)────────────────────────────
// 全局查看偏好,存中央设置(config.toml 键 detail_controls_hidden)。与沉浸模式正交:沉浸隐整个 app
// shell,这里只隐本组件底部半透明控制条。
const controlsHidden = ref(readSettingBool(CONTROLS_HIDDEN_KEY, false))
// 后端只应用:权威值变化(启动水合 / 恢复默认 / 外部改文件)→ 同步显示态;用户改动显式提交。
watch(
  () => readSettingBool(CONTROLS_HIDDEN_KEY, false),
  (v) => {
    controlsHidden.value = v
  },
)
function toggleControls() {
  controlsHidden.value = !controlsHidden.value
  // 写盘失败由中央服务统一提示;此处 catch 只为收掉 promise。
  writeSettings({ [CONTROLS_HIDDEN_KEY]: String(controlsHidden.value) }).catch(() => {})
}
// 底部操作栏可见性单源:v-show 与视频 chrome 抬升(.has-bottom-controls → --viewer-bottom-inset)
// 共用同一判据,防两处条件漂移导致视频控制条与缩放工具重叠。
// controlsHidden 对全媒体类型生效(2026-07-23):视频态唤出入口在播放控制条内切换钮(控制条
// 与本栏显隐状态独立,动鼠标即唤回控制条,钮恒可达);图像态保点大图切换,无「隐后无法唤出」死角。
const detailControlsVisible = computed(
  () => !isImmersive.value && !controlsHidden.value && editor.status.value === 'idle',
)

// 点击/拖拽判别:img 是 pointer-events:none,点击落在 .detail-viewer 容器上。记 mousedown 坐标,
// mouseup(click)时位移超阈值视作平移拖拽、不切换(否则拖动放大后的图会误触发隐藏操作栏)。
let pointerDownX = 0
let pointerDownY = 0
function onViewerPointerDown(e: MouseEvent) {
  pointerDownX = e.clientX
  pointerDownY = e.clientY
  state.startDrag(e)
}
function onViewerClick(e: MouseEvent) {
  if (editor.status.value !== 'idle') return // 编辑覆层自有交互,不切换底栏显隐
  // 仅图像:视频/音频有原生控件、gate/不可用占位有各自交互,均不拦截。
  if (detail.value?.mediaType !== 'image') return
  if (isUnavailable.value || showExoticGate.value) return
  if (Math.hypot(e.clientX - pointerDownX, e.clientY - pointerDownY) > 4) return
  toggleControls()
}

// ── activeViewer 单源 populate(顶栏重构 P4-b / L2)──────────────────────────
// 查看器暴露 ViewerApi 供命令层(L3)调用;同形对象 populate 进 viewerStore 供顶栏(L4)与命令
// when 谓词消费。图/视共用本组件,按 kind 区分(resolveViewerKind 单点推导),API 给能力全集。
const viewerApi: ViewerApi = {
  next: () => void navigate(1),
  prev: () => void navigate(-1),
  close,
  toggleImmersive,
  zoomIn: () => state.zoomIn(),
  zoomOut: () => state.zoomOut(),
  cycleZoomMode: () => handleToggleZoom(),
  rotate: () => handleRotate(),
  toggleInfo: () => ui.toggleViewerInfo(),
  edit: () => void openEditor(),
  // ── 视频动作透传(GC):经 VideoPlayer expose 的 PlayerApi;非视频项 videoRef 为空则 no-op。
  // 命令层(GD 批)经 activeViewer.api 调用这些方法(when: kind==='video')。
  playPause: () => videoRef.value?.playPause(),
  seekBy: (seconds: number) => videoRef.value?.seekBy(seconds),
  volumeBy: (delta: number) => videoRef.value?.volumeBy(delta),
  toggleMute: () => videoRef.value?.toggleMute(),
  setRate: (rate: number) => videoRef.value?.setRate(rate),
  rateStep: (dir: 1 | -1) => videoRef.value?.rateStep(dir),
  toggleLoop: () => videoRef.value?.toggleLoop(),
  togglePip: () => void videoRef.value?.togglePip(),
  toggleFullscreenPair: () => videoRef.value?.toggleFullscreenPair(),
  captureFrame: () => videoRef.value?.captureFrame(),
  isFullscreenPair: () => videoRef.value?.isFullscreenPair() ?? false,
}
defineExpose(viewerApi)

let viewerToken: number | null = null
function viewerSnapshot(): Omit<ActiveViewer, 'immersive'> {
  const it = media.detailItem!
  return {
    kind: resolveViewerKind(it.mediaType, it.fileFormat),
    mediaType: it.mediaType,
    fileFormat: it.fileFormat,
    id: it.id,
    path: it.absPath ?? null,
    title: it.fileName,
    api: viewerApi,
    // 底栏文件信息:detailItem 即 MediaDetail 全字段,直接投影。翻页换项经本 snapshot 重patch;
    // 面板/底栏改标量走 itemPatchSignal 桥(本组件原位改 detail.value 不触发 identity watch)。
    fileInfo: toViewerFileInfo(it),
  }
}

// detailItem 变化(路由加载 / 翻页)即同步顶栏上下文:首次 populate、后续 patch(沉浸态由 P4-c
// 掌管,viewerSnapshot 不含 immersive,patch 不覆盖它)。token 时序防御见 viewerStore。
watch(
  () => media.detailItem,
  (it) => {
    if (!it) return
    if (viewerToken === null) {
      viewerToken = viewer.populate({ ...viewerSnapshot(), immersive: false })
    } else {
      viewer.patch(viewerToken, viewerSnapshot())
    }
  },
  { immediate: true },
)

onMounted(() => {
  // 窗口缩放的重算已并入上面的 viewerRO(观察视图区自身,窗口变化会传导为其尺寸变化),
  // 且额外覆盖 app 侧栏开合 / 信息面板停靠等「窗口没变但视图区变了」的情形。
  // 路由已在场(导航到 /view/:id 才挂载本组件)→ 立即按 :id 加载。
  void loadFromRoute(routeId.value)
})
// 路由 :id 变化(前进后退 / 深链)→ 重新加载(翻页自身的 replace 由 loadFromRoute 守卫跳过)。
watch(routeId, (id) => void loadFromRoute(id))

onBeforeUnmount(() => {
  if (deltaTimer) {
    clearTimeout(deltaTimer)
    deltaTimer = null
  }
  viewerRO?.disconnect()
  state.cleanup()
  // 离开查看器:清 activeViewer 上下文(token 时序防御)+ 复位 mediaStore detail 态(isDetailOpen
  // 归假,useFullscreenExitGuard 的 Esc 让行随之解除)。
  if (viewerToken !== null) viewer.clear(viewerToken)
  media.closeDetail()
  ocr.closePanel()
})
</script>

<style scoped src="./ContentViewer.styles.css"></style>
