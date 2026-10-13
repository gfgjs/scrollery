<template>
  <div class="doc-viewer">
    <!-- 工具栏：返回 / 标题 / 页码 / 翻页模式 / 外部打开。沉浸模式下 v-show 隐藏（保留 DOM 与状态）。 -->
    <div class="doc-viewer__toolbar" v-show="!immersive">
      <!-- 侧栏开关：由本视图自持（2026-07-16）。此前它是 AppShell 的浮动钮（absolute top:10 left:12），
           侧栏收起时正好压在本工具栏左上角的返回控件上——两个带框小按钮**几何重叠**，一个跳走一个开侧栏，
           这才是真机「跟左侧栏的展开/收起按钮放在一起，容易误导」的实情。
           留在**左侧**：侧栏开关贴着它所控制的那条侧栏是通行惯例（且用户的浮动钮原本就在左边，肌肉记忆在此）。
           重叠已由「返回改 ghost 的 ← 书名」消除——两者现在一个是纯图标、一个是带文字的宽控件，形状即可区分；
           再加分隔线把「chrome 开关」与「退出」分组。
           判据同源见 mediaRoute.viewerHostsSidebarToggle（AppShell 据它停渲浮动钮，两处不会各出一个）。 -->
      <button
        class="doc-viewer__btn"
        :class="{ 'is-active': ui.viewerSidebarVisible }"
        @click="ui.toggleViewerSidebar()"
        :title="ui.viewerSidebarVisible ? t('sidebar.hideSidebar') : t('sidebar.showSidebar')"

      >
        <PanelLeftClose v-if="ui.viewerSidebarVisible" :size="16" />
        <PanelLeftOpen v-else :size="16" />
      </button>
      <span class="doc-viewer__sep"></span>
      <!-- 返回 + 书名合成**一个**控件（2026-07-16 真机：「返回按钮不好找、很粗糙、跟文档名也未对齐」）。
           合成而非并排的三个理由：
           ① 不好找 —— 此前 返回 与其余 6 个控件共用 .doc-viewer__btn，长得一模一样，没有退出感；
              整条书名成为可点区后，命中面积从一个小按钮扩到整个标题，且是页面上第一个视觉元素。
           ② 未对齐 —— 此前是「带 1px 边框 + 5/10 padding 的按钮」与「裸文本标题」两个盒子并排，
              基线对不上是结构性的、调 padding 治不好。合成一个 flex 行后二者共基线。
           ③ 与画廊同构 —— 这正是 MediaGrid 的 .view-back-bar「← 返回收藏夹 · 名字」同款读法，
              使「钻进来的子视图」在全应用有一致的退出语义。
           有意不抽公共组件：两处物理形态不同（画廊是内容区上方的整条 bar，此处是工具栏内的一段），
           共享外观为零 —— 抽了就是为 DRY 而 DRY（同 S1 UiToolbar「无共享外观则不建组件」的裁决）。 -->
      <button class="doc-viewer__back" @click="goBack" :title="t('common.back')">
        <ChevronLeft :size="18" />
        <span class="doc-viewer__title">{{ title }}</span>
      </button>
      <span v-if="pageInfo" class="doc-viewer__page"
        >{{ pageInfo.page }} / {{ pageInfo.pages }}</span
      >
      <!-- foliate 进度页脚（R2-3）：全书百分比 + 当前章名。 -->
      <span v-if="isNativeReading && readerFraction !== null" class="doc-viewer__page">
        {{ Math.round(readerFraction * 100) }}%<span v-if="readerTocLabel">
          · {{ readerTocLabel }}</span
        >
      </span>
      <!-- 工具栏控件由 useToolbarOverflow 按 DOM 逆序折叠进溢出菜单；
           本容器保持右对齐，UiPopover 只显示已折叠项。 -->
      <div
        ref="foldableEl"
        class="doc-viewer__foldable"
        :class="{ 'is-measuring': isMeasuring, 'is-settling': isSettling }"
      >
        <!-- 旧翻页模式选择仅 pdf 保留（txt/md 已交 foliate，用下方阅读流切换）。 -->
        <div
          v-if="kind === 'pdf'"
          class="fold-item"
          data-toolbar-item
          :class="{ 'fold-item--folded': folded('pagerMode') }"
        >
          <div class="fold-item__inner">
            <label class="doc-viewer__mode">
              <span>{{ t('doc.pagerMode') }}</span>
              <select v-model="pagerMode" @change="savePagerMode">
                <option value="scroll">{{ t('doc.pagerScroll') }}</option>
                <option value="wheel-snap">{{ t('doc.pagerWheelSnap') }}</option>
                <option value="keyboard">{{ t('doc.pagerKeyboard') }}</option>
              </select>
            </label>
          </div>
        </div>
        <!-- foliate 阅读流切换：翻页 ↔ 滚动。txt 与 epub 统一（同一 <foliate-view> 的 flow 属性）。
             flowForced(护栏,2026-07-17)时禁用:BookReader 已强制 scrolled,切换无效,禁用+题注说明而非静默吞点击。 -->
        <div
          v-if="isNativeReading"
          class="fold-item"
          data-toolbar-item
          :class="{ 'fold-item--folded': folded('readerFlow') }"
        >
          <div class="fold-item__inner">
            <label class="doc-viewer__mode">
              <span>{{ t('doc.readerFlow') }}</span>
              <select
                v-model="readerFlow"
                @change="saveReaderFlow"
                :disabled="flowForced"
                :title="flowForced ? t('doc.flowForcedScrolled') : undefined"
              >
                <option value="paginated">{{ t('doc.flowPaginated') }}</option>
                <option value="scrolled">{{ t('doc.flowScrolled') }}</option>
              </select>
            </label>
          </div>
        </div>
        <!-- epub 排版模式：自带（出版方样式胜出）↔ 智能（阅读器排版覆盖排版差/无排版的 epub）。仅 epub 有意义。 -->
        <div
          v-if="kind === 'epub'"
          class="fold-item"
          data-toolbar-item
          :class="{ 'fold-item--folded': folded('styleMode') }"
        >
          <div class="fold-item__inner">
            <label class="doc-viewer__mode">
              <span>{{ t('doc.styleMode') }}</span>
              <select v-model="styleMode" @change="saveStyleMode">
                <option value="book">{{ t('doc.styleBook') }}</option>
                <option value="reader">{{ t('doc.styleReader') }}</option>
              </select>
            </label>
          </div>
        </div>
        <!-- 书内搜索：仅 foliate 渲染器（txt/epub/md）。 -->
        <div
          v-if="isNativeFoliateKind"
          class="fold-item"
          data-toolbar-item
          :class="{ 'fold-item--folded': folded('search') }"
        >
          <div class="fold-item__inner">
            <button
              class="doc-viewer__btn"
              :class="{ 'is-active': editing ? textEditSearchRef?.isOpen : showSearch }"
              @click="toggleDocumentSearch"
              :title="t('doc.search')"

            >
              <Search :size="16" />
            </button>
          </div>
        </div>
        <!-- 目录（TOC）：仅 foliate 渲染器（txt/epub/md）。 -->
        <div
          v-if="isNativeReading"
          class="fold-item"
          data-toolbar-item
          :class="{ 'fold-item--folded': folded('toc') }"
        >
          <div class="fold-item__inner">
            <button
              class="doc-viewer__btn"
              :class="{ 'is-active': showToc }"
              @click="toggleToc"
              :title="t('doc.toc')"

            >
              <List :size="16" />
            </button>
          </div>
        </div>
        <!-- 书签：仅 foliate 渲染器（txt/epub/md）。 -->
        <div
          v-if="isNativeReading"
          class="fold-item"
          data-toolbar-item
          :class="{ 'fold-item--folded': folded('bookmarks') }"
        >
          <div class="fold-item__inner">
            <button
              class="doc-viewer__btn"
              :class="{ 'is-active': showBookmarks }"
              @click="toggleBookmarks"
              :title="t('doc.bookmarks')"

            >
              <Bookmark :size="16" />
            </button>
          </div>
        </div>
        <!-- 阅读排版设置（字号/行距）：仅 foliate 渲染器（txt/epub/md）。 -->
        <div
          v-if="isNativeFoliateKind"
          class="fold-item"
          data-toolbar-item
          :class="{ 'fold-item--folded': folded('readerSettings') }"
        >
          <div class="fold-item__inner">
            <button
              class="doc-viewer__btn"
              :class="{ 'is-active': showReaderSettings }"
              @click="toggleReaderSettings"
              :title="t('doc.readerSettings')"

            >
              <Type :size="16" />
            </button>
          </div>
        </div>
        <!-- 自动翻页（R4）：▶ 开始 / ⏸ 暂停；速率在设置面板调。 -->
        <div
          v-if="isNativeReading"
          class="fold-item"
          data-toolbar-item
          :class="{ 'fold-item--folded': folded('autoScroll') }"
        >
          <div class="fold-item__inner">
            <button
              class="doc-viewer__btn"
              :class="{ 'is-active': autoScroll }"
              @click="autoScroll = !autoScroll"
              :title="t('doc.autoScroll')"

            >
              <component :is="autoScroll ? Pause : Play" :size="16" />
            </button>
          </div>
        </div>
        <!-- 沉浸模式：隐藏工具栏专注阅读（Esc / 浮动按钮退出）。 -->
        <div
          v-if="isNativeFoliateKind"
          class="fold-item"
          data-toolbar-item
          :class="{ 'fold-item--folded': folded('immersive') }"
        >
          <div class="fold-item__inner">
            <button
              class="doc-viewer__btn"
              @click="enterImmersive"
              :title="t('doc.immersive')"

            >
              <Maximize2 :size="16" />
            </button>
          </div>
        </div>
        <div
          v-if="supportsNativeEdit && !editing"
          class="fold-item"
          data-toolbar-item
          :class="{ 'fold-item--folded': folded('edit') }"
        >
          <div class="fold-item__inner">
            <button
              class="doc-viewer__btn"
              @click="startEdit"
              :title="t('doc.edit')"

            >
              <Pencil :size="16" />
            </button>
          </div>
        </div>
        <div
          v-if="supportsNativeEdit && !editing"
          class="fold-item"
          data-toolbar-item
          :class="{ 'fold-item--folded': folded('versions') }"
        >
          <div class="fold-item__inner">
            <button
              class="doc-viewer__btn"
              :class="{ 'is-active': showVersions }"
              @click="showVersions = !showVersions"
              :title="t('doc.versions')"

            >
              <History :size="16" />
            </button>
          </div>
        </div>
        <div
          v-if="supportsNativeEdit && !editing"
          class="fold-item"
          data-toolbar-item
          :class="{ 'fold-item--folded': folded('proofread') }"
        >
          <div class="fold-item__inner">
            <button
              class="doc-viewer__btn"
              :class="{ 'is-active': showProofread }"
              @click="showProofread = !showProofread"
              :title="t('doc.proofread')"

            >
              <Sparkles :size="16" />
            </button>
          </div>
        </div>
        <div
          v-if="supportsNativeReplace"
          class="fold-item"
          data-toolbar-item
          :class="{ 'fold-item--folded': folded('replace') }"
        >
          <div class="fold-item__inner">
            <button
              class="doc-viewer__btn"
              :class="{ 'is-active': showRepl }"
              @click="showRepl = !showRepl"
              :title="t('doc.replace')"

            >
              <Replace :size="16" />
            </button>
          </div>
        </div>
        <div
          v-if="isMarkdown"
          class="fold-item"
          data-toolbar-item
          :class="{ 'fold-item--folded': folded('readerEngine') }"
        >
          <div class="fold-item__inner">
            <label class="doc-viewer__mode" :title="t('doc.readerEngine')">
              <Braces :size="16" />
              <select
                :value="selectedMarkdownEngine"

                @change="onMarkdownEngineChange"
              >
                <option value="native">{{ t('doc.readerEngineNative') }}</option>
                <option value="vditor">{{ t('doc.readerEngineVditor') }}</option>
                <option
                  value="markdown-editor"
                  :disabled="!markdownEditorAvailable"
                  :title="!markdownEditorAvailable ? t('doc.markdownEditorUnavailable') : undefined"
                >
                  {{ t('doc.readerEngineMarkdownEditor') }}
                </option>
              </select>
            </label>
          </div>
        </div>
        <div
          v-if="detail"
          class="fold-item"
          data-toolbar-item
          :class="{ 'fold-item--folded': folded('external') }"
        >
          <div class="fold-item__inner">
            <button
              class="doc-viewer__btn"
              @click="openExternal"
              :title="t('common.openExternal')"

            >
              <ExternalLink :size="16" />
            </button>
          </div>
        </div>
        <!-- ⋯ 溢出按钮:仅溢出时现身。放在折叠容器**内**——引擎 budget 已预留其宽
             (overflowButtonWidth),容器内占位恰好单算,不像放容器外会被双计。 -->
        <button
          v-show="hasOverflow"
          ref="moreBtnEl"
          class="doc-viewer__btn"
          :class="{ 'is-active': showMoreMenu }"
          @click="showMoreMenu = !showMoreMenu"
          :title="t('doc.moreTools')"
        >
          <MoreHorizontal :size="16" />
        </button>
      </div>
      <!-- ⋯ 溢出菜单:只渲染已折叠项,竖排「图标+文字」自描述行(select 项保留原控件)。
           关闭时机:动作型点击即关;select 调整不关(可连续调)。 -->
      <UiPopover v-model:open="showMoreMenu" :anchor="moreBtnEl" placement="bottom-end">
        <div class="doc-more">
          <label v-if="folded('pagerMode')" class="doc-more__select doc-viewer__mode">
            <span>{{ t('doc.pagerMode') }}</span>
            <select v-model="pagerMode" @change="savePagerMode">
              <option value="scroll">{{ t('doc.pagerScroll') }}</option>
              <option value="wheel-snap">{{ t('doc.pagerWheelSnap') }}</option>
              <option value="keyboard">{{ t('doc.pagerKeyboard') }}</option>
            </select>
          </label>
          <label v-if="folded('readerFlow')" class="doc-more__select doc-viewer__mode">
            <span>{{ t('doc.readerFlow') }}</span>
            <select
              v-model="readerFlow"
              @change="saveReaderFlow"
              :disabled="flowForced"
              :title="flowForced ? t('doc.flowForcedScrolled') : undefined"
            >
              <option value="paginated">{{ t('doc.flowPaginated') }}</option>
              <option value="scrolled">{{ t('doc.flowScrolled') }}</option>
            </select>
          </label>
          <label v-if="folded('styleMode')" class="doc-more__select doc-viewer__mode">
            <span>{{ t('doc.styleMode') }}</span>
            <select v-model="styleMode" @change="saveStyleMode">
              <option value="book">{{ t('doc.styleBook') }}</option>
              <option value="reader">{{ t('doc.styleReader') }}</option>
            </select>
          </label>
          <button
            v-if="folded('search')"
            class="doc-more__row"
            :class="{ 'is-active': editing ? textEditSearchRef?.isOpen : showSearch }"
            @click="menuAct(toggleDocumentSearch)"
          >
            <Search :size="16" /><span>{{ t('doc.search') }}</span>
          </button>
          <button
            v-if="folded('toc')"
            class="doc-more__row"
            :class="{ 'is-active': showToc }"
            @click="menuAct(toggleToc)"
          >
            <List :size="16" /><span>{{ t('doc.toc') }}</span>
          </button>
          <button
            v-if="folded('bookmarks')"
            class="doc-more__row"
            :class="{ 'is-active': showBookmarks }"
            @click="menuAct(toggleBookmarks)"
          >
            <Bookmark :size="16" /><span>{{ t('doc.bookmarks') }}</span>
          </button>
          <button
            v-if="folded('readerSettings')"
            class="doc-more__row"
            :class="{ 'is-active': showReaderSettings }"
            @click="menuAct(toggleReaderSettings)"
          >
            <Type :size="16" /><span>{{ t('doc.readerSettings') }}</span>
          </button>
          <button
            v-if="folded('autoScroll')"
            class="doc-more__row"
            :class="{ 'is-active': autoScroll }"
            @click="menuAct(() => (autoScroll = !autoScroll))"
          >
            <component :is="autoScroll ? Pause : Play" :size="16" /><span>{{
              t('doc.autoScroll')
            }}</span>
          </button>
          <button
            v-if="folded('immersive')"
            class="doc-more__row"
            @click="menuAct(enterImmersive)"
          >
            <Maximize2 :size="16" /><span>{{ t('doc.immersive') }}</span>
          </button>
          <button v-if="folded('edit')" class="doc-more__row" @click="menuAct(startEdit)">
            <Pencil :size="16" /><span>{{ t('doc.edit') }}</span>
          </button>
          <button
            v-if="folded('versions')"
            class="doc-more__row"
            :class="{ 'is-active': showVersions }"
            @click="menuAct(() => (showVersions = !showVersions))"
          >
            <History :size="16" /><span>{{ t('doc.versions') }}</span>
          </button>
          <button
            v-if="folded('proofread')"
            class="doc-more__row"
            :class="{ 'is-active': showProofread }"
            @click="menuAct(() => (showProofread = !showProofread))"
          >
            <Sparkles :size="16" /><span>{{ t('doc.proofread') }}</span>
          </button>
          <button
            v-if="folded('replace')"
            class="doc-more__row"
            :class="{ 'is-active': showRepl }"
            @click="menuAct(() => (showRepl = !showRepl))"
          >
            <Replace :size="16" /><span>{{ t('doc.replace') }}</span>
          </button>
          <div
            v-if="folded('readerEngine')"
            class="doc-more__row doc-more__select"
            @click.stop
          >
            <Braces :size="16" />
            <select
              class="doc-more__engine-select"
              :value="selectedMarkdownEngine"

              @change="onMarkdownEngineChange"
            >
              <option value="native">{{ t('doc.readerEngineNative') }}</option>
              <option value="vditor">{{ t('doc.readerEngineVditor') }}</option>
              <option
                value="markdown-editor"
                :disabled="!markdownEditorAvailable"
                :title="!markdownEditorAvailable ? t('doc.markdownEditorUnavailable') : undefined"
              >
                {{ t('doc.readerEngineMarkdownEditor') }}
              </option>
            </select>
          </div>
          <button v-if="folded('external')" class="doc-more__row" @click="menuAct(openExternal)">
            <ExternalLink :size="16" /><span>{{ t('common.openExternal') }}</span>
          </button>
        </div>
      </UiPopover>
    </div>

    <!-- 沉浸模式浮动退出按钮：工具栏隐藏时提供退出入口（Esc 亦可）。 -->
    <button
      v-if="immersive"
      class="doc-viewer__immersive-exit"
      @click="immersive = false"
      :title="t('doc.exitImmersive')"

    >
      <Minimize2 :size="18" />
    </button>

    <!-- 渲染区 + 可选侧栏（替换 / 版本） -->
    <div class="doc-viewer__body">
      <div class="doc-viewer__reader">
        <!-- 编辑态（仅文本）：纯文本编辑 + 保存目标 -->
        <div v-if="editing && !isParallelEditorMode" class="doc-edit" :style="nativeEditorStyle">
          <div class="doc-edit__bar">
            <input
              v-model="editLabel"
              :disabled="editSaving"
              class="doc-edit__label"
              :placeholder="t('doc.versionLabelPlaceholder')"
            />
            <button class="doc-viewer__btn doc-viewer__btn--primary" :disabled="editSaving || !editDirty" :title="t('doc.saveAndContinue') + ' (Ctrl+S / ⌘S)'" @click="saveNativeEdit(true)">
              <Save :size="14" /> {{ t(editSaving ? 'edit.saving' : 'doc.saveNewVersion') }}
            </button>
            <button class="doc-viewer__btn" :disabled="editSaving || (!editDirty && currentVersionId === null)" @click="overwriteSource">
              {{ t('doc.overwriteSource') }}
            </button>
            <button class="doc-viewer__btn doc-edit__exit" :disabled="editSaving" @click="cancelNativeEdit">{{ t('doc.finishEditing') }}</button>
          </div>
          <TextEditSearch ref="textEditSearchRef" :value="editBuffer" :textarea="editArea" :disabled="false" :text-style="nativeEditorStyle" />
          <textarea ref="editArea" v-model="editBuffer" class="doc-edit__area" :aria-label="t('doc.edit')" spellcheck="false" @keydown="onNativeEditKeydown"></textarea>
          <div class="doc-edit__status">
            <span role="status">{{ t(editSaving ? 'edit.saving' : editDirty ? 'doc.editorUnsaved' : 'doc.editorSaved') }}</span>
            <TextEditPosition ref="textEditPositionRef" :value="editBuffer" :textarea="editArea" @open="textEditSearchRef?.close()" />
          </div>
        </div>

        <template v-else-if="detail">
          <PdfReader
            v-if="kind === 'pdf'"
            :key="readerKey"
            ref="readerRef"
            :url="url"
            :initial="initialPos"
            @ready="onReady"
            @progress="onProgress"
            @info="onInfo"
          />
          <VditorDocumentModule
            v-else-if="isVditorMode"
            :key="id"
            ref="markdownEditorRef"
            :item-id="id"
            :value="textContent ?? ''"
            :document-url="url"
            :initial="readerPositions.vditor"
            :is-dark="appIsDark"
            :typography="readerTypography"
            @change="onMarkdownEditorChange"
            @progress="onVditorProgress"
            @panel-open="closeNativePanels"
          />
          <MarkdownEditorDocumentModule
            v-else-if="isMarkdownEditorMode"
            :key="id"
            ref="markdownEditorRef"
            :item-id="id"
            :value="textContent ?? ''"
            @change="onMarkdownEditorChange"
          />
          <!-- 默认统一渲染器（foliate-js）：epub 交 makeBook 探测；txt/md 构造 SyntheticBook。 -->
          <BookReader
            v-else-if="kind === 'epub' || kind === 'text'"
            :key="readerKey"
            ref="readerRef"
            :url="url"
            :text-source="kind === 'text' ? textSource : undefined"
            :initial="initialPos"
            :replacer="replacer"
            :flow="readerFlow"
            :style-mode="styleMode"
            :typography="readerTypography"
            :page-turn="readerPageTurn"
            :zh-convert="bookZhConvert || undefined"
            :auto-scroll="autoScroll"
            :auto-scroll-sec="readerAutoScrollSec"
            :reader-theme-light="readerThemeLight"
            :reader-theme-dark="readerThemeDark"
            :book-reader-theme="bookReaderTheme"
            :vertical="readerVertical"
            :immersive="immersive"
            @ready="onReady"
            @progress="onProgress"
            @locate="onLocate"
            @toc="onToc"
            @flow-forced="onFlowForced"
            @document-link="openDocumentLink"
          />
          <div v-else class="doc-viewer__unsupported">
            <FileQuestion :size="48" />
            <p>{{ t('doc.unsupportedFormat', { format: detail.fileFormat }) }}</p>
            <button class="doc-viewer__btn doc-viewer__btn--primary" @click="openExternal">
              {{ t('common.openExternal') }}
            </button>
          </div>
        </template>
        <div v-else class="doc-viewer__unsupported">
          <template v-if="error">
            <p class="doc-viewer__load-error" role="alert">{{ error }}</p>
            <button class="doc-viewer__btn doc-viewer__btn--primary doc-viewer__retry" @click="load">{{ t('official.retry') }}</button>
          </template>
          <span v-else role="status">{{ t('common.loading') }}</span>
        </div>
      </div>

      <ReplacementPanel
        v-if="showRepl && supportsNativeReplace"
        :item-id="id"
        @changed="onReplChanged"
        @close="showRepl = false"
      />

      <VersionPanel
        v-if="showVersions && supportsNativeEdit"
        :item-id="id"
        @changed="refreshText"
        @close="showVersions = false"
      />

      <ProofreadPanel
        v-if="showProofread && supportsNativeEdit"
        :item-id="id"
        :text="textContent ?? ''"
        :current-version-id="currentVersionId"
        @changed="refreshText"
        @close="showProofread = false"
      />

      <ReaderSettingsPanel
        v-if="showReaderSettings && (isNativeFoliateKind || isVditorMode)"
        :editor-mode="isVditorMode || editing"
        :fixed-font-family="editing && isMarkdown"
        :font-size-px="readerFontSize"
        :line-height="readerLineHeight"
        :font-family="readerFontFamily"
        :max-inline-size-px="readerPageWidth"
        :page-turn="readerPageTurn"
        :auto-scroll-sec="readerAutoScrollSec"
        :reader-theme-light="readerThemeLight"
        :reader-theme-dark="readerThemeDark"
        :font-weight="readerFontWeight"
        :letter-spacing-em="readerLetterSpacing"
        :text-align="readerTextAlign"
        :title-scale="readerTitleScale"
        :paragraph-spacing-em="readerParagraphSpacing"
        :vertical="readerVertical"
        :is-dark="appIsDark"
        :book="
          isNativeFoliateKind
            ? {
                encoding: bookEncoding,
                reflow: bookReflow,
                zhConvert: bookZhConvert,
                theme: bookReaderTheme,
              }
            : null
        "
        :is-txt="isTxt"
        @change="onTypographyChange"
        @book-change="onBookPrefsChange"
        @close="showReaderSettings = false"
      />

      <TocPanel
        v-if="showToc && isNativeFoliateKind"
        :toc="readerToc"
        :active-href="readerTocHref"
        @navigate="onTocNavigate"
        @close="showToc = false"
      />

      <SearchPanel
        v-if="showSearch && isNativeFoliateKind"
        ref="searchPanelRef"
        :results="searchResults"
        :searching="searching"
        :progress="searchProgress"
        :failed="searchFailed"
        @search="onSearch"
        @navigate="onSearchNavigate"
        @close="closeSearch"
      />

      <BookmarkPanel
        v-if="showBookmarks && isNativeFoliateKind"
        :bookmarks="bookmarks"
        @add="onAddBookmark"
        @navigate="onBookmarkNavigate"
        @delete="onDeleteBookmark"
        @close="showBookmarks = false"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
// 文档浏览器路由 /doc/:id。PDF 使用 pdf.js，EPUB 与文本使用 foliate-js；翻页逻辑
// 由 usePager 解耦（三种模式，存配置）；阅读进度按位置字符串保存/恢复（reading_progress 表）。
// 阅读器 composable 在此装配，彼此不直接导入。
import { ref, computed, watch, onMounted, onBeforeUnmount, defineAsyncComponent, nextTick } from 'vue'
import { onBeforeRouteLeave, onBeforeRouteUpdate, useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { convertFileSrc } from '@tauri-apps/api/core'
import { open as shellOpen } from '@tauri-apps/plugin-shell'
import {
  ChevronLeft,
  ExternalLink,
  FileQuestion,
  Replace,
  Pencil,
  History,
  Save,
  Sparkles,
  Type,
  List,
  Search,
  Bookmark,
  Maximize2,
  Minimize2,
  Play,
  Pause,
  PanelLeftClose,
  PanelLeftOpen,
  MoreHorizontal,
  Braces,
} from '@lucide/vue'
import { IPC } from '../constants/ipc'
import { invokeIpc } from '../utils/ipc'
import { hasEditContext } from '../utils/markdownEditorCapability'
import { readSetting, writeSettings } from '../stores/settingsPersistence'
import { readSettingEnum } from '../composables/settingsValues'
import { useToastStore } from '../stores/toastStore'
import { useUiStore } from '../stores/uiStore'
import { useThemeStore } from '../stores/themeStore'
import { READER_THEME_FOLLOW } from '../themes/readerThemes'
import { usePager, type PagerMode } from '../composables/usePager'
import { useToolbarOverflow } from '../composables/useToolbarOverflow'
import UiPopover from '../components/ui/UiPopover.vue'
import { buildReplacer, type ReplacementRule } from '../utils/replacements'
import type { MediaDetail } from '../types/media'
import { useViewerStore } from '../stores/viewerStore'
import type { ReaderApi } from '../composables/reader/readerApiTypes'
import { useReaderProgress } from '../composables/reader/useReaderProgress'
import { useReaderNav } from '../composables/reader/useReaderNav'
import { useBookSearch } from '../composables/reader/useBookSearch'
import { useReaderBookmarks } from '../composables/reader/useReaderBookmarks'
import { useReaderPanels } from '../composables/reader/useReaderPanels'
import { useDocImmersive } from '../composables/reader/useDocImmersive'
import { useReaderTypography } from '../composables/reader/useReaderTypography'
import { useReaderBookPrefs } from '../composables/reader/useReaderBookPrefs'
import { useDocEditVersion } from '../composables/reader/useDocEditVersion'
import { indentSource } from '../components/doc/sourceIndent'
import { matchesTextSourceHash, sourceLineOffset } from '../utils/textSourceAnchor'
import { useDocActiveViewerSync } from '../composables/reader/useDocActiveViewerSync'
import { useConfirm } from '../composables/useConfirm'
import { FONT_STACKS } from '../utils/readerStyles'
import { decodeReaderPositions, encodeReaderPositions } from '../utils/readerPosition'
import type { MarkdownEngine } from '../types/reader'
import TextEditSearch from '../components/doc/TextEditSearch.vue'
import TextEditPosition from '../components/doc/TextEditPosition.vue'

// 渲染器懒加载：foliate-js/pdfjs 仅在真正打开对应格式时才进入对应 chunk。
const PdfReader = defineAsyncComponent(() => import('../components/doc/PdfReader.vue'))
// epub、txt、md 默认使用 foliate-js（BookReader）。
const BookReader = defineAsyncComponent(() => import('../components/doc/BookReader.vue'))
// Vditor 平行模块：仅由 Markdown 文档的 route query 启用，默认阅读器不变。
const VditorDocumentModule = defineAsyncComponent(
  () => import('../components/doc/VditorDocumentModule.vue'),
)
// Markdown Editor 实验模块：独立于 BookReader，仅由 route query 显式启用。
const MarkdownEditorDocumentModule = defineAsyncComponent(
  () => import('../components/doc/MarkdownEditorDocumentModule.vue'),
)
const ReplacementPanel = defineAsyncComponent(
  () => import('../components/doc/ReplacementPanel.vue'),
)
const VersionPanel = defineAsyncComponent(() => import('../components/doc/VersionPanel.vue'))
const ProofreadPanel = defineAsyncComponent(() => import('../components/doc/ProofreadPanel.vue'))
const ReaderSettingsPanel = defineAsyncComponent(
  () => import('../components/doc/ReaderSettingsPanel.vue'),
)
const TocPanel = defineAsyncComponent(() => import('../components/doc/TocPanel.vue'))
const SearchPanel = defineAsyncComponent(() => import('../components/doc/SearchPanel.vue'))
const searchPanelRef = ref<{ focusQuery: () => void } | null>(null)
const BookmarkPanel = defineAsyncComponent(() => import('../components/doc/BookmarkPanel.vue'))

const route = useRoute()
const router = useRouter()
const { t, locale } = useI18n()
const toast = useToastStore()

const id = computed(() => Number(route.params.id))
const detail = ref<MediaDetail | null>(null)
const error = ref('')
const initialPos = ref<string | null>(null)
const readerPositions = ref(decodeReaderPositions(null))
let progressItemId = Number.NaN
const pageInfo = ref<{ page: number; pages: number } | null>(null)
const readerRef = ref<ReaderApi | null>(null)
const markdownEditorRef = ref<{
  saveNow: () => Promise<boolean>
  toggleToc?: () => void
  toggleSearch?: () => void
  openSearch?: () => void
  openReplace?: () => void
  getCurrentPosition?: () => string | null
  closePanels?: () => boolean
} | null>(null)
// 三项阅读模式偏好(存中央设置):初值取权威快照,缺键回落各自默认;下方 watch 随快照变化应用。
const PAGER_MODE_KEY = 'doc_pager_mode'
const READER_FLOW_KEY = 'doc_reader_flow'
const STYLE_MODE_KEY = 'doc_epub_style_mode'
/** 取值候选集(schema 已限定枚举;此处仅用于快照未到/异常文本时的回落判定)。 */
const PAGER_MODES = ['scroll', 'wheel-snap', 'keyboard'] as const
const READER_FLOWS = ['paginated', 'scrolled'] as const
const STYLE_MODES = ['book', 'reader'] as const
const pagerMode = ref<PagerMode>(readSettingEnum(PAGER_MODE_KEY, PAGER_MODES, 'scroll'))
// foliate 阅读流（txt/epub）：翻页 / 滚动。运行时切换不 remount（不进 readerKey），由 BookReader watch 施加。
const readerFlow = ref<'paginated' | 'scrolled'>(
  readSettingEnum(READER_FLOW_KEY, READER_FLOWS, 'paginated'),
)
// BookReader 检出超限单片时强制滚动模式；本层提示并禁用流切换，换文档时复位。
const flowForced = ref(false)
// epub 排版模式：'book' 自带（书样式胜出，默认，零回归）/ 'reader' 智能（阅读器排版覆盖）。仅 epub 有意义。
const styleMode = ref<'book' | 'reader'>(readSettingEnum(STYLE_MODE_KEY, STYLE_MODES, 'book'))
// 阅读主题的日/夜两槽使用 reader theme id 或 FOLLOW（默认跟随应用）。
const readerThemeLight = ref(READER_THEME_FOLLOW)
const readerThemeDark = ref(READER_THEME_FOLLOW)
// app 当前是否暗色:决定阅读主题选择器编辑/展示哪一槽(主题 store 的有效模式权威)。
const ui = useUiStore()
const theme = useThemeStore()
const viewer = useViewerStore()
const appIsDark = computed(() => theme.isDark)
// 自动翻页开关在换书时重置，间隔秒数由全局设置持久化。
const autoScroll = ref(false)
const readerAutoScrollSec = ref(5)
const showReaderSettings = ref(false)

// 替换规则变更后通过 reloadToken 重建渲染器。
const replacer = ref<(t: string) => string>((t) => t)
const showRepl = ref(false)
const reloadToken = ref(0)

// verticalChanged / needsRemount 需要先捕获阅读位置再增加 reloadToken，
// 否则重挂载后会跳回开卷。useReaderTypography/useReaderBookPrefs 均经本函数触发。
// onReplChanged 和 refreshText 不涉及位置保持，无需捕获。
function captureCurrentPosition() {
  if (isVditorMode.value) {
    const position = markdownEditorRef.value?.getCurrentPosition?.()
    if (position) onVditorProgress(position, id.value)
    return
  }
  const cur = readerRef.value?.getCurrentLocation?.()
  if (cur) {
    initialPos.value = cur.locator
    onProgress(cur.locator)
  }
}
function requestRemount(captureFirst: boolean) {
  if (captureFirst) captureCurrentPosition()
  reloadToken.value++
}

// 排版全参数（R3）+ 变更处理：字号/行距/字体族/栏宽/字重/字距/对齐/章题缩放/段距/翻页动画/竖排。
const typographyComposable = useReaderTypography({
  readerAutoScrollSec,
  readerThemeLight,
  readerThemeDark,
  requestRemount,
})
const {
  readerFontSize,
  readerLineHeight,
  readerFontFamily,
  readerPageWidth,
  readerFontWeight,
  readerLetterSpacing,
  readerTextAlign,
  readerTitleScale,
  readerParagraphSpacing,
  readerPageTurn,
  readerVertical,
  readerTypography,
  onTypographyChange,
} = typographyComposable

// 每书设置（编码/重排/简繁/主题）+ 变更处理（S12 + S32）。
const bookPrefsComposable = useReaderBookPrefs({ id, requestRemount })
const { bookEncoding, bookReflow, bookZhConvert, bookReaderTheme, bookMarkdownEngine, onBookPrefsChange } =
  bookPrefsComposable

// 文本编辑与版本状态由 useDocEditVersion 管理。
const editVersion = useDocEditVersion({
  id,
  requestRemount,
  beforeSourceOverwrite: bookPrefsComposable.waitForPendingWrites,
  onSourceOverwritten: () => { bookEncoding.value = 'UTF-8' },
})
const {
  textContent,
  editing,
  dirty: editDirty,
  saving: editSaving,
  editBuffer,
  editLabel,
  currentVersionId,
  showVersions,
  showProofread,
  startEdit: startEditState,
  cancelEdit,
  saveNewVersion,
  overwriteSource,
  refreshText,
} = editVersion

const editArea = ref<HTMLTextAreaElement | null>(null)
let nativeEditPosition: {
  itemId: number
  source: string
  start: number
  end: number
  direction: 'forward' | 'backward' | 'none'
  scrollTop: number
  scrollLeft: number
} | null = null
watch(id, () => { nativeEditPosition = null })
const textEditSearchRef = ref<InstanceType<typeof TextEditSearch> | null>(null)
const textEditPositionRef = ref<InstanceType<typeof TextEditPosition> | null>(null)
const nativeEditorStyle = computed(() => ({
  '--doc-edit-font-size': `${readerFontSize.value}px`,
  '--doc-edit-line-height': String(readerLineHeight.value),
  '--doc-edit-font-family': isMarkdown.value ? 'var(--font-mono)' : FONT_STACKS[readerFontFamily.value],
  '--doc-edit-width': `${readerPageWidth.value}px`,
}))

function startEdit() {
  const readingAnchor = readerRef.value?.getCurrentSourceAnchor?.()
  captureCurrentPosition()
  closeAllPanels()
  startEditState()
  const source = editBuffer.value
  void nextTick(async () => {
    const area = editArea.value
    if (!area) return
    // 仅在原文完全相同时恢复偏移；版本切换或放弃草稿后不套用旧选区。
    const position = nativeEditPosition?.itemId === id.value && nativeEditPosition.source === area.value
      ? nativeEditPosition : null
    area.focus({ preventScroll: true })
    area.setSelectionRange(position?.start ?? 0, position?.end ?? 0, position?.direction ?? 'none')
    area.scrollTop = position?.scrollTop ?? 0
    area.scrollLeft = position?.scrollLeft ?? 0
    if (position || !readingAnchor) return
    const initialValue = area.value
    const offset = typeof readingAnchor.offset === 'number' ? readingAnchor.offset : sourceLineOffset(source, readingAnchor.line)
    if (offset === null || !(await matchesTextSourceHash(source, readingAnchor.sourceHash))) return
    // 摘要计算不阻塞输入；已经开始编辑、移动光标或切走时，迟到结果不能再改变位置。
    if (area !== editArea.value || area.value !== initialValue || document.activeElement !== area
      || area.selectionStart !== 0 || area.selectionEnd !== 0 || area.scrollTop !== 0) return
    area.setSelectionRange(offset, offset)
    area.blur()
    area.focus()
  })
}

function onNativeEditKeydown(event: KeyboardEvent) {
  const area = editArea.value
  if ((event.key !== 'Tab' && event.key !== 'Enter') || event.defaultPrevented || event.isComposing
    || event.ctrlKey || event.altKey || event.metaKey || !area || area.disabled || area.readOnly) return
  if (event.key === 'Enter') {
    if (event.shiftKey) return
    const lineStart = area.selectionStart === 0 ? 0 : area.value.lastIndexOf('\n', area.selectionStart - 1) + 1
    const indent = /^[\t ]+/.exec(area.value.slice(lineStart, area.selectionStart))?.[0]
    if (!indent) return
    // 只继承当前行的空白前缀，不把 TXT 中的列表样式或代码改写成 Markdown。
    event.preventDefault()
    if (document.execCommand('insertText', false, `\n${indent}`)) {
      area.dispatchEvent(new Event('input', { bubbles: true }))
    }
    return
  }
  event.preventDefault()
  const { selectionStart, selectionEnd, selectionDirection, scrollTop, scrollLeft } = area
  const result = indentSource(area.value, selectionStart, selectionEnd, event.shiftKey)
  if (!result.edits.length) return
  const first = result.edits[0]!
  const last = result.edits[result.edits.length - 1]!
  const end = last.position + last.remove
  const replacement = result.source.slice(first.position, end + result.source.length - area.value.length)
  // 用一次原生替换提交受影响行，保留浏览器撤销栈；直接赋 value 会丢失撤销历史。
  area.setSelectionRange(first.position, end)
  if (document.execCommand('insertText', false, replacement)) {
    // 与查找替换共用原生 input 契约，兼容未主动派发 input 的 WebView。
    area.dispatchEvent(new Event('input', { bubbles: true }))
    area.setSelectionRange(result.start, result.end, selectionDirection)
  } else {
    area.setSelectionRange(selectionStart, selectionEnd, selectionDirection)
  }
  area.scrollTop = scrollTop
  area.scrollLeft = scrollLeft
}

let pendingNativeSave: Promise<void> | null = null
let nativeSaveRequested = false
function saveNativeEdit(focusEditor = false): Promise<void> {
  if (!editing.value || !editDirty.value || (editSaving.value && !pendingNativeSave)) return Promise.resolve()
  const area = editArea.value
  const itemId = id.value
  if (focusEditor) area?.focus({ preventScroll: true })
  nativeSaveRequested = true
  if (pendingNativeSave) return pendingNativeSave
  // 保存使用提交时快照，正文继续可编辑；后来的 Ctrl+S 合并为顺序保存最新稿。
  pendingNativeSave = (async () => {
    do {
      nativeSaveRequested = false
      await saveNewVersion({ keepEditing: true })
    } while (nativeSaveRequested && editDirty.value && editing.value && id.value === itemId && editArea.value === area)
  })().finally(() => {
    nativeSaveRequested = false
    pendingNativeSave = null
  })
  return pendingNativeSave
}

async function cancelNativeEdit() {
  if (editSaving.value) return
  const area = editArea.value
  if (editDirty.value) {
    const { confirmed } = await confirmLeave({
      title: t('doc.discardEditTitle'),
      message: t('doc.discardEditMessage'),
      confirmText: t('doc.discardEditConfirm'),
      cancelText: t('doc.keepEditing'),
      danger: true,
    })
    if (!confirmed) {
      // 等待对话框释放焦点，避免随后把已归还正文的焦点抢回“结束编辑”按钮。
      await nextTick()
      if (area === editArea.value) area?.focus({ preventScroll: true })
      return
    }
  }
  if (area === editArea.value && !editSaving.value) {
    nativeEditPosition = area && !editDirty.value ? {
      itemId: id.value, source: area.value,
      start: area.selectionStart, end: area.selectionEnd, direction: area.selectionDirection,
      scrollTop: area.scrollTop, scrollLeft: area.scrollLeft,
    } : null
    cancelEdit()
  }
}

const { confirm: confirmLeave } = useConfirm()
let pendingEditLeave: Promise<boolean> | null = null

function prepareNativeEditLeave(): Promise<boolean> {
  if (pendingEditLeave) return pendingEditLeave
  if (editSaving.value) return saveNewVersion()
  if (!editDirty.value) return Promise.resolve(true)
  // 连续导航或退出请求共用同一次确认，避免后开的对话框取消前一条保存路径。
  pendingEditLeave = confirmLeave({
    title: t('doc.leaveEditTitle'),
    message: t('doc.leaveEditMessage'),
    confirmText: t('doc.saveVersionAndLeave'),
    cancelText: t('doc.keepEditing'),
  }).then(({ confirmed }) => confirmed ? saveNewVersion() : false)
    .finally(() => { pendingEditLeave = null })
  return pendingEditLeave
}

async function prepareDocumentLeave() {
  const allowed = await prepareNativeEditLeave()
  if (allowed) {
    captureCurrentPosition()
    flushProgress()
  }
  return allowed
}
onBeforeRouteLeave(prepareDocumentLeave)
onBeforeRouteUpdate(prepareDocumentLeave)

const url = computed(() => (detail.value ? convertFileSrc(detail.value.absPath) : ''))
const title = computed(() => detail.value?.fileName ?? t('routes.doc'))
// :key 含 reloadToken，使替换规则/版本变更后重建渲染器以重新套用。
const readerKey = computed(() => `${id.value}-${reloadToken.value}`)
// 替换支持 txt、md 和 epub。
const supportsReplace = computed(() => kind.value === 'text' || kind.value === 'epub')
// 编辑与版本管理仅支持 txt、md。
const supportsEdit = computed(() => kind.value === 'text')

// 文本渲染器只接收 txt/md。rtf 若按纯文本渲染会显示控制字，因此走不支持格式的外部打开入口。
// 画廊文本卡支持的格式范围独立于阅读器。
const TEXT_FORMATS = ['txt', 'md']
const kind = computed<'pdf' | 'epub' | 'text' | 'unsupported'>(() => {
  const f = (detail.value?.fileFormat ?? '').toLowerCase()
  if (f === 'pdf') return 'pdf'
  if (f === 'epub') return 'epub'
  if (TEXT_FORMATS.includes(f)) return 'text'
  return 'unsupported'
})
// 默认 txt/md 走 BookReader（R2-4/R2-4b）。isMarkdown 决定 SyntheticBook 构造分支。
const isMarkdown = computed(() => (detail.value?.fileFormat ?? '').toLowerCase() === 'md')
// 显式深链优先；从图库再次打开时恢复本篇文档上次选择。
const markdownEditorAvailable = hasEditContext()
const selectedMarkdownEngine = computed<MarkdownEngine>(() => {
  const requested = typeof route.query.engine === 'string'
    ? normalizeMarkdownEngine(route.query.engine)
    : bookMarkdownEngine.value
  // 能力缺失只影响当前展示，保留偏好供支持该 API 的环境恢复，也不触发路由保存屏障。
  return requested === 'markdown-editor' && !markdownEditorAvailable ? 'native' : requested
})
const isVditorMode = computed(() => isMarkdown.value && selectedMarkdownEngine.value === 'vditor')
const isMarkdownEditorMode = computed(
  () => isMarkdown.value && selectedMarkdownEngine.value === 'markdown-editor',
)
const isParallelEditorMode = computed(
  () => isVditorMode.value || isMarkdownEditorMode.value,
)
// txt/md 的 SyntheticBook 文本源：版本/编码由后端解析，前端只传 itemId + isMarkdown + reflow（每书）。
const textSource = computed(() => ({
  itemId: id.value,
  isMarkdown: isMarkdown.value,
  reflow: bookReflow.value,
}))
// foliate 统一渲染器覆盖面：epub + text（txt/md）。仅 pdf 走独立滚动型渲染器。
const isFoliateKind = computed(() => kind.value === 'epub' || kind.value === 'text')
const isNativeFoliateKind = computed(() => isFoliateKind.value && !isParallelEditorMode.value)
const isNativeReading = computed(() => isNativeFoliateKind.value && !editing.value)
const supportsNativeReplace = computed(() => supportsReplace.value && !isParallelEditorMode.value && !editing.value)
const supportsNativeEdit = computed(() => supportsEdit.value && !isParallelEditorMode.value)
// 仅 txt 有「每书」编码/重排设置（md 无编码问题、按整篇 markdown 渲染）。
const isTxt = computed(() => kind.value === 'text' && !isMarkdown.value)

// ── 工具栏窄窗折叠(2026-07-17):useToolbarOverflow(Priority+,与顶栏同引擎同 fold-item 契约) ──
const foldableEl = ref<HTMLElement | null>(null)
const moreBtnEl = ref<HTMLElement | null>(null)
const showMoreMenu = ref(false)
// 折叠项键序:**必须与模板内 data-toolbar-item 的 DOM 顺序与 v-if 条件逐项一致**(索引即优先级,
// 尾部先折)。kind/编辑态变化改变项集 → 该键也进 remeasureKey 触发重测。
const toolbarItemKeys = computed<string[]>(() => {
  const keys: string[] = []
  if (kind.value === 'pdf') keys.push('pagerMode')
  if (isNativeReading.value) keys.push('readerFlow')
  if (kind.value === 'epub') keys.push('styleMode')
  if (isNativeFoliateKind.value) keys.push('search')
  if (isNativeReading.value) keys.push('toc', 'bookmarks')
  if (isNativeFoliateKind.value) keys.push('readerSettings')
  if (isNativeReading.value) keys.push('autoScroll')
  if (isNativeFoliateKind.value) keys.push('immersive')
  if (supportsNativeEdit.value && !editing.value) keys.push('edit')
  if (supportsNativeEdit.value && !editing.value) keys.push('versions', 'proofread')
  if (supportsNativeReplace.value) keys.push('replace')
  if (isMarkdown.value) keys.push('readerEngine')
  if (detail.value) keys.push('external')
  return keys
})
const {
  visibleCount: toolbarVisibleCount,
  hasOverflow,
  isMeasuring,
  isSettling,
} = useToolbarOverflow({
  containerRef: foldableEl,
  overflowButtonWidth: 40,
  // locale 换文案 / 项集变化(换 kind、进出编辑态)→ 项宽缓存失效重测。
  remeasureKey: computed(() => `${locale.value}|${toolbarItemKeys.value.join(',')}`),
  // ⋯ 菜单开启期暂停重测:防量尺帧让锚点漂移、弹层横跳(引擎 deferWhile 契约)。
  deferWhile: showMoreMenu,
})
/** 内联折叠态:非测量帧且该键序号 >= 可见数。键不在项集(条件不满足)时 indexOf=-1 恒 false。 */
function folded(key: string): boolean {
  const i = toolbarItemKeys.value.indexOf(key)
  return !isMeasuring.value && i >= 0 && i >= toolbarVisibleCount.value
}
/** 菜单动作:执行并收起 ⋯ 菜单(select 类项不走此路,可连续调整)。 */
function menuAct(fn: () => void) {
  fn()
  showMoreMenu.value = false
}
// 全部项放得下时 ⋯ 消失,同步收起悬空菜单。
watch(hasOverflow, (v) => {
  if (!v) showMoreMenu.value = false
})

// 书内搜索、目录与书签面板互斥；装配层通过 closeOtherPanels 关闭其余面板。
const search = useBookSearch({
  readerRef,
  closeOtherPanels: () => {
    nav.showToc.value = false
    showReaderSettings.value = false
    bookmarksC.showBookmarks.value = false
  },
})
const nav = useReaderNav({
  readerRef,
  closeOtherPanels: () => {
    search.closeSearch()
    showReaderSettings.value = false
    bookmarksC.showBookmarks.value = false
  },
})
const bookmarksC = useReaderBookmarks({
  id,
  readerRef,
  closeOtherPanels: () => {
    nav.showToc.value = false
    showReaderSettings.value = false
    search.closeSearch()
  },
})
const {
  showSearch,
  searchResults,
  searching,
  searchProgress,
  searchFailed,
  toggleSearch,
  onSearch,
  onSearchNavigate,
  closeSearch,
} = search
const {
  readerFraction,
  readerTocLabel,
  readerTocHref,
  readerToc,
  showToc,
  onLocate,
  onToc,
  onTocNavigate,
  toggleToc,
} = nav
const {
  showBookmarks,
  bookmarks,
  onAddBookmark,
  onBookmarkNavigate,
  onDeleteBookmark,
  toggleBookmarks,
} = bookmarksC
// 阅读排版设置面板未拆独立 composable（无独立面板状态域，仅 showReaderSettings 一个 root 瞬态）；
// toggleReaderSettings 保留 root，与 toggleToc/toggleSearch/toggleBookmarks 同款互斥语义。
function toggleReaderSettings() {
  showReaderSettings.value = !showReaderSettings.value
  if (showReaderSettings.value) {
    markdownEditorRef.value?.closePanels?.()
    showToc.value = false
    showBookmarks.value = false
    closeSearch()
  }
}

// 全部可开合面板的开关。**新增面板必须登记于此**——进沉浸的收拢与 Esc 的分层退出都读它；
// 漏登记 = 沉浸态残留面板遮挡 + Esc 跳过面板层直接关掉阅读器，而没有任何门会红。
// showSearch 不在数组里：它的关闭另有清理（作废进行中迭代 + 清高亮），统一走 closeSearch()。
const panelFlags = [showToc, showReaderSettings, showBookmarks, showRepl, showVersions, showProofread]
const { anyPanelOpen, closeAllPanels: closeNativePanels } = useReaderPanels(panelFlags, closeSearch, showSearch)
function closeAllPanels() {
  closeNativePanels()
  markdownEditorRef.value?.closePanels?.()
  textEditSearchRef.value?.close()
  textEditPositionRef.value?.close()
}

function toggleDocumentSearch() {
  if (editing.value) {
    if (!textEditSearchRef.value?.close()) textEditSearchRef.value?.open()
  } else toggleSearch()
}

const pager = usePager({
  mode: () => pagerMode.value,
  next: () => readerRef.value?.next(),
  prev: () => readerRef.value?.prev(),
  container: () => readerRef.value?.getScrollEl() ?? null,
})

watch(isParallelEditorMode, (enabled) => {
  if (!enabled) return
  pager.detach()
  closeAllPanels()
  showMoreMenu.value = false
})

let documentLinkRequest = 0
let pendingDocumentAnchor: string | null = null
watch(() => route.fullPath, () => {
  documentLinkRequest++
  const anchor = route.query.docAnchor
  pendingDocumentAnchor = typeof anchor === 'string' && anchor ? anchor : null
}, { immediate: true, flush: 'sync' })

async function navigateDocumentHeading(fragment: string) {
  const reader = readerRef.value
  try {
    await reader?.goToHref?.(`#${encodeURIComponent(fragment)}`)
  } catch {
    if (reader === readerRef.value) toast.addToast('error', t('doc.documentLinkHeadingMissing'))
  }
}
async function openDocumentLink(href: string) {
  const sourceId = id.value
  const request = ++documentLinkRequest
  try {
    const target = await invokeIpc<{ itemId: number; fragment: string | null }>(IPC.RESOLVE_DOCUMENT_LINK, { itemId: sourceId, href })
    if (request !== documentLinkRequest || id.value !== sourceId) return
    if (target.itemId === sourceId) {
      if (target.fragment) await navigateDocumentHeading(target.fragment)
      return
    }
    // 文档超链接保留返回路径，并延续当前原生阅读方式；不改目标文档保存的引擎偏好。
    await router.push({ path: `/doc/${target.itemId}`, query: { engine: 'native', ...(target.fragment ? { docAnchor: target.fragment } : {}) } })
  } catch {
    if (request === documentLinkRequest && id.value === sourceId) toast.addToast('error', t('doc.documentLinkFailed'))
  }
}

function onReady() {
  // foliate 渲染器（epub + txt）自管滚轮/滚动、无对外滚动容器 → usePager 仅接键盘；
  // 滚动型渲染器（pdf + md 的 TextReader）绑定其滚动容器。
  pager.attach(isNativeFoliateKind.value ? null : (readerRef.value?.getScrollEl() ?? null))
  if (pendingDocumentAnchor && readerRef.value?.goToHref) {
    const fragment = pendingDocumentAnchor
    // 仅在本次导航首次就绪时应用，随后改设置或读写切换不应把读者拉回链接起点。
    pendingDocumentAnchor = null
    void navigateDocumentHeading(fragment)
  }
}

// 阅读进度去抖保存（composable；红线：(itemId,pos) 配对不可简化为只存 pos，见 useReaderProgress
// 内注释，2026-07-10 审查 B12）。
const progress = useReaderProgress(id)
const { flushProgress } = progress

function onProgress(position: string) {
  if (progressItemId !== id.value) return
  readerPositions.value.native = position
  progress.onProgress(encodeReaderPositions(readerPositions.value))
}

function onVditorProgress(position: string, itemId: number) {
  // 旧组件在路由换书后才卸载，不能把最后一次回调写进新文档。
  if (itemId !== progressItemId || itemId !== id.value) return
  readerPositions.value.vditor = position
  progress.onProgress(encodeReaderPositions(readerPositions.value))
}

watch(selectedMarkdownEngine, (engine) => {
  if (engine === 'native') initialPos.value = readerPositions.value.native
})

function onInfo(info: { page: number; pages: number }) {
  pageInfo.value = info
}

// 护栏触发:提示一次并禁用流切换(BookReader 内部已强制 scrolled,此处只管 UI 语义一致)。
function onFlowForced() {
  if (flowForced.value) return // remount(版本/规则变更)重复触发只提示一次
  flowForced.value = true
  toast.addToast('info', t('doc.flowForcedScrolled'))
}

let documentLoadGeneration = 0

function isCurrentDocumentLoad(generation: number, itemId: number): boolean {
  return generation === documentLoadGeneration && id.value === itemId
}

async function load() {
  const itemId = id.value
  const generation = ++documentLoadGeneration
  // 切换文档：先冲刷上一篇进度（flush 用捕获的旧 itemId,不受此刻 route id 已变影响），拆掉旧监听。
  flushProgress()
  pager.detach()
  progress.reset()
  progressItemId = itemId
  readerPositions.value = decodeReaderPositions(null)
  detail.value = null
  pageInfo.value = null
  initialPos.value = null
  textContent.value = null
  currentVersionId.value = null
  editing.value = false
  error.value = ''
  bookPrefsComposable.resetLocal()
  search.reset()
  bookmarksC.reset()
  immersive.value = false
  autoScroll.value = false
  flowForced.value = false
  nav.reset()
  try {
    const d = await invokeIpc<MediaDetail>(IPC.GET_MEDIA_DETAIL, { id: itemId })
    if (!isCurrentDocumentLoad(generation, itemId)) return
    const nextInitialPos = await invokeIpc<string | null>(IPC.GET_READING_PROGRESS, {
      itemId,
    }).catch(() => null)
    if (!isCurrentDocumentLoad(generation, itemId)) return
    readerPositions.value = decodeReaderPositions(nextInitialPos)
    initialPos.value = readerPositions.value.native
    if (!(await loadReplacer(itemId, generation))) return
    // 文本文档：取生效文本（当前版本或源）+ 当前版本 id（供编辑父版本）。
    const f = (d.fileFormat ?? '').toLowerCase()
    if (TEXT_FORMATS.includes(f)) {
      // 生效稿与父版本是编辑前提；读取失败不能当成空文档交给自动保存编辑器。
      const nextTextContent = await invokeIpc<string>(IPC.GET_DOCUMENT_TEXT, {
        itemId,
      })
      if (!isCurrentDocumentLoad(generation, itemId)) return
      textContent.value = nextTextContent
      const cur = await invokeIpc<{ id: number } | null>(IPC.GET_CURRENT_VERSION, {
        itemId,
      })
      if (!isCurrentDocumentLoad(generation, itemId)) return
      currentVersionId.value = cur?.id ?? null
    }
    // 每书设置：简繁（txt/md/epub 通用）+ 编码/重排（仅 txt）。从 reader_book_prefs 读，供设置面板显示 +
    // textSource.reflow 透传。须在 detail.value=d（触发 BookReader 挂载读 textSource）之前完成。
    if (f === 'txt' || f === 'md' || f === 'epub') {
      await bookPrefsComposable.loadForItem(itemId)
    }
    if (!isCurrentDocumentLoad(generation, itemId)) return
    detail.value = d
  } catch (e) {
    if (isCurrentDocumentLoad(generation, itemId)) {
      error.value = t('doc.openFailed', { error: (e as Error)?.message ?? e })
    }
  }
}

// 拉取该项生效的替换规则（global + item）并构建替换函数。
async function loadReplacer(
  itemId = id.value,
  generation = documentLoadGeneration,
): Promise<boolean> {
  const rules = await invokeIpc<ReplacementRule[]>(IPC.GET_EFFECTIVE_REPLACEMENTS, {
    itemId,
  }).catch(() => [])
  if (!isCurrentDocumentLoad(generation, itemId)) return false
  replacer.value = buildReplacer(rules)
  return true
}

// 规则变更：重建替换函数并重渲染当前文档（key 变化 → 渲染器重挂载重新套用）。
async function onReplChanged() {
  const generation = documentLoadGeneration
  if (await loadReplacer(id.value, generation)) reloadToken.value++
}

function goBack() {
  // 与 ContentViewer.close 同判据:history.state.back 才是「应用内有上一页」的可靠信号;
  // window.history.length 在深链直开时也 >1,会把用户退到应用外(2026-07-10 深审 LOW-1 统一)。
  if (router.options.history.state.back != null) router.back()
  else void router.push('/')
}

function normalizeMarkdownEngine(value: string): MarkdownEngine {
  if (value === 'vditor' || value === 'markdown-editor') return value
  return 'native'
}

async function onMarkdownEngineChange(event: Event) {
  const target = event.currentTarget
  if (!(target instanceof HTMLSelectElement)) return
  await setMarkdownEngine(normalizeMarkdownEngine(target.value))
  // 用户选择继续编辑时导航取消，原生 select 也要回到实际引擎。
  target.value = selectedMarkdownEngine.value
}

async function setMarkdownEngine(engine: MarkdownEngine) {
  if (engine === 'markdown-editor' && !markdownEditorAvailable) return
  if (!isMarkdown.value || engine === selectedMarkdownEngine.value) return
  const itemId = id.value
  const generation = documentLoadGeneration
  const query = { ...route.query }
  query.engine = engine
  const failure = await router.replace({ query })
  if (!failure && itemId === id.value && generation === documentLoadGeneration) {
    cancelEdit()
    closeAllPanels()
    showMoreMenu.value = false
    pager.detach()
    await bookPrefsComposable.saveMarkdownEngine(engine)
  }
}

function onMarkdownEditorChange(source: string) {
  // 导航守卫确保切换引擎前已保存；原生编辑也应从这份最新文本继续。
  textContent.value = source
}

async function openExternal() {
  if (detail.value) await shellOpen(detail.value.absPath).catch(() => {})
}

// 三项阅读模式偏好存中央设置(config.toml);写盘失败由中央服务统一提示,此处 catch 只为收掉 promise。
function savePagerMode() {
  writeSettings({ [PAGER_MODE_KEY]: pagerMode.value }).catch(() => {})
}
function saveReaderFlow() {
  writeSettings({ [READER_FLOW_KEY]: readerFlow.value }).catch(() => {})
}
function saveStyleMode() {
  writeSettings({ [STYLE_MODE_KEY]: styleMode.value }).catch(() => {})
}

// 沉浸态与 Esc 分层退出由 composable 管理；window keydown 监听在本视图生命周期内注册。
const docImmersive = useDocImmersive({ viewer, anyPanelOpen, closeAllPanels, goBack })
const { immersive, enterImmersive } = docImmersive

// activeViewer 的 viewerApi 保持各 toggle 函数引用稳定，供命令层按方法名调用。
const readerCommands = computed(() => {
  if (editing.value) return {}
  if (isVditorMode.value) {
    const editor = markdownEditorRef.value
    return { toggleToc: editor?.toggleToc, toggleSearch: editor?.toggleSearch, toggleSettings: toggleReaderSettings }
  }
  return isNativeFoliateKind.value
    ? { toggleToc, toggleSearch, toggleBookmarks, toggleSettings: toggleReaderSettings }
    : {}
})

useDocActiveViewerSync({
  id,
  detail,
  title,
  immersive,
  goBack,
  prepareClose: () => markdownEditorRef.value?.saveNow() ?? prepareNativeEditLeave(),
  readerCommands,
  viewer,
})

// 翻页模式 / 阅读流 / 排版模式随权威快照应用(启动水合、恢复默认、外部改文件):只应用,不写回。
// 自动翻页速率与阅读主题日夜槽的水合已随 useReaderTypography 内建(同一份设置来源),不在此重复。
// 勾选/下拉的用户改动经 saveXxx 显式提交,故本组 watch 不会与写盘互激。
watch(
  () => readSetting(PAGER_MODE_KEY),
  () => {
    pagerMode.value = readSettingEnum(PAGER_MODE_KEY, PAGER_MODES, 'scroll')
  },
)
watch(
  () => readSetting(READER_FLOW_KEY),
  () => {
    readerFlow.value = readSettingEnum(READER_FLOW_KEY, READER_FLOWS, 'paginated')
  },
)
watch(
  () => readSetting(STYLE_MODE_KEY),
  () => {
    styleMode.value = readSettingEnum(STYLE_MODE_KEY, STYLE_MODES, 'book')
  },
)

// 随路由 id 变化重载文档。排版全参数（14 项）的启动引导已随 useReaderTypography 内建，不在此重复。

watch(id, load, { immediate: true })

function onDocumentKeydown(event: KeyboardEvent) {
  if (event.isComposing) return
  if ((event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey
    && !event.repeat && editing.value && event.key.toLowerCase() === 'g') {
    event.preventDefault()
    textEditPositionRef.value?.open()
    return
  }
  if ((event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey
    && !event.isComposing && !event.repeat && editing.value
    && ['f', 'h'].includes(event.key.toLowerCase())) {
    event.preventDefault()
    textEditPositionRef.value?.close()
    textEditSearchRef.value?.open(event.key.toLowerCase() === 'h')
    return
  }
  if (event.key === 'Escape' && (textEditPositionRef.value?.close() || textEditSearchRef.value?.close())) {
    event.preventDefault()
    return
  }
  if ((event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey
    && !event.isComposing && !event.repeat && event.key.toLowerCase() === 's'
    && editing.value) {
    event.preventDefault()
    void saveNativeEdit()
    return
  }
  // 编辑区域外也能打开文档查找与替换；编辑器内由其捕获监听接管。
  if ((event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey
    && !event.repeat && event.key.toLowerCase() === 'h'
    && !editing.value && markdownEditorRef.value?.openReplace) {
    event.preventDefault()
    markdownEditorRef.value.openReplace()
    return
  }
  if ((event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey
    && !event.isComposing && !event.repeat && event.key.toLowerCase() === 'f'
    && !editing.value && (markdownEditorRef.value?.openSearch || isNativeFoliateKind.value)) {
    event.preventDefault()
    if (markdownEditorRef.value?.openSearch) markdownEditorRef.value.openSearch()
    else {
      if (!showSearch.value) toggleSearch()
      void nextTick(() => searchPanelRef.value?.focusQuery())
    }
    return
  }
  // 即时编辑器面板由组件持有，Esc 先关闭它们；沉浸仍沿用统一退出顺序。
  if (event.key === 'Escape' && !immersive.value && markdownEditorRef.value?.closePanels?.()) {
    event.preventDefault()
    return
  }
  docImmersive.onGlobalKeydown(event)
}

onMounted(() => window.addEventListener('keydown', onDocumentKeydown))

onBeforeUnmount(() => {
  documentLoadGeneration++
  documentLinkRequest++
  captureCurrentPosition()
  flushProgress()
  pager.detach()
  window.removeEventListener('keydown', onDocumentKeydown)
})
</script>

<style scoped src="./DocumentViewer.styles.css"></style>
