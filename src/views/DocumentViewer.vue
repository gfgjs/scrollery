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
      <span v-if="isNativeFoliateKind && readerFraction !== null" class="doc-viewer__page">
        {{ Math.round(readerFraction * 100) }}%<span v-if="readerTocLabel">
          · {{ readerTocLabel }}</span
        >
      </span>
      <!-- 折叠簇(2026-07-17 窄窗治理):自此处起的全部控件进 useToolbarOverflow 统一折叠容器
           (Priority+,与顶栏 AppToolbar 同引擎、同 fold-item CSS 契约)。窄窗放不下时按 DOM 逆序
           逐项收进 ⋯ 溢出菜单(带 0.28s 折叠动画);宽窗全展开时布局与旧版一致——右对齐由本容器
           justify-end 接替旧 .doc-viewer__spacer。菜单变体在下方 UiPopover 内,只渲染已折叠项。 -->
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
          v-if="isNativeFoliateKind"
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
              :class="{ 'is-active': showSearch }"
              @click="toggleSearch"
              :title="t('doc.search')"

            >
              <Search :size="16" />
            </button>
          </div>
        </div>
        <!-- 目录（TOC）：仅 foliate 渲染器（txt/epub/md）。 -->
        <div
          v-if="isNativeFoliateKind"
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
          v-if="isNativeFoliateKind"
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
          v-if="isNativeFoliateKind"
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
          v-if="supportsNativeEdit"
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
          v-if="supportsNativeEdit"
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
                <option value="markdown-editor">
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
            :class="{ 'is-active': showSearch }"
            @click="menuAct(toggleSearch)"
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
              <option value="markdown-editor">
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
        <div v-if="editing && !isParallelEditorMode" class="doc-edit">
          <div class="doc-edit__bar">
            <input
              v-model="editLabel"
              class="doc-edit__label"
              :placeholder="t('doc.versionLabelPlaceholder')"
            />
            <button class="doc-viewer__btn doc-viewer__btn--primary" @click="saveNewVersion">
              <Save :size="14" /> {{ t('doc.saveNewVersion') }}
            </button>
            <button class="doc-viewer__btn" @click="overwriteSource">
              {{ t('doc.overwriteSource') }}
            </button>
            <button class="doc-viewer__btn" @click="cancelEdit">{{ t('common.cancel') }}</button>
          </div>
          <textarea v-model="editBuffer" class="doc-edit__area" spellcheck="false"></textarea>
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
            :item-id="id"
            :value="textContent ?? ''"
          />
          <MarkdownEditorDocumentModule
            v-else-if="isMarkdownEditorMode"
            :value="markdownEditorDraft ?? textContent ?? ''"
            @change="onMarkdownEditorChange"
          />
          <!-- 默认统一渲染器（foliate-js）：epub 交 makeBook 探测；txt/md 构造 SyntheticBook。 -->
          <BookReader
            v-else-if="kind === 'epub' || kind === 'text'"
            :key="readerKey"
            ref="readerRef"
            :url="kind === 'epub' ? url : undefined"
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
          />
          <div v-else class="doc-viewer__unsupported">
            <FileQuestion :size="48" />
            <p>{{ t('doc.unsupportedFormat', { format: detail.fileFormat }) }}</p>
            <button class="doc-viewer__btn doc-viewer__btn--primary" @click="openExternal">
              {{ t('common.openExternal') }}
            </button>
          </div>
        </template>
        <div v-else-if="error" class="doc-viewer__unsupported">{{ error }}</div>
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
        v-if="showReaderSettings && isNativeFoliateKind"
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
        :results="searchResults"
        :searching="searching"
        :progress="searchProgress"
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
// 文档浏览器（§5.1）：路由 /doc/:id。按格式分发到 pdf.js / epub.js / 文本渲染器；翻页逻辑
// 由 usePager 解耦（三种模式，存配置）；阅读进度按位置字符串保存/恢复（reading_progress 表）。
// 结构拆分（2026-07-25，纯结构移动）：逻辑域下沉至 src/composables/reader/*（composable 之间
// 不互相 import，由本文件做声明式装配）；模板 / 样式零改动，见 docs/planning/2026-07-25-
// 超长文件拆分方案/analysis/DocumentViewer-vue.md。
import { ref, computed, watch, onMounted, onBeforeUnmount, defineAsyncComponent } from 'vue'
import { useRoute, useRouter } from 'vue-router'
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
import { useDocActiveViewerSync } from '../composables/reader/useDocActiveViewerSync'

// 渲染器懒加载：foliate-js/pdfjs 仅在真正打开对应格式时才进入对应 chunk。
const PdfReader = defineAsyncComponent(() => import('../components/doc/PdfReader.vue'))
// 默认统一渲染器（R2）：epub + txt + md 走 foliate-js（BookReader）。旧 TextReader/EpubReader 已于 R2-8 退役删除。
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
const BookmarkPanel = defineAsyncComponent(() => import('../components/doc/BookmarkPanel.vue'))

const route = useRoute()
const router = useRouter()
const { t, locale } = useI18n()
const toast = useToastStore()

const id = computed(() => Number(route.params.id))
const detail = ref<MediaDetail | null>(null)
const error = ref('')
const initialPos = ref<string | null>(null)
const pageInfo = ref<{ page: number; pages: number } | null>(null)
const readerRef = ref<ReaderApi | null>(null)
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
// 护栏态(2026-07-17):BookReader 检出超限单片强制 scrolled → 本层提示一次 + 禁用流切换。换文档复位。
const flowForced = ref(false)
// epub 排版模式：'book' 自带（书样式胜出，默认，零回归）/ 'reader' 智能（阅读器排版覆盖）。仅 epub 有意义。
const styleMode = ref<'book' | 'reader'>(readSettingEnum(STYLE_MODE_KEY, STYLE_MODES, 'book'))
// 阅读主题（R3）：日/夜两槽全局选择（reader theme id 或 FOLLOW，默认 FOLLOW=跟随应用零回归）。
const readerThemeLight = ref(READER_THEME_FOLLOW)
const readerThemeDark = ref(READER_THEME_FOLLOW)
// app 当前是否暗色:决定阅读主题选择器编辑/展示哪一槽(主题 store 的有效模式权威)。
const ui = useUiStore()
const theme = useThemeStore()
const viewer = useViewerStore()
const appIsDark = computed(() => theme.isDark)
// 自动翻页（R4）：开关（每会话瞬态，换书重置）+ 间隔秒（全局持久化，设置面板调）。
const autoScroll = ref(false)
const readerAutoScrollSec = ref(5)
const showReaderSettings = ref(false)

// 替换规则（§5.2）：生效规则 → 替换函数；面板开关；reloadToken 用于规则变更后重渲染。
const replacer = ref<(t: string) => string>((t) => t)
const showRepl = ref(false)
const reloadToken = ref(0)

// remount 时序不变量（红线,§3 风险 2）：verticalChanged / needsRemount 是仅有的两条需
// capture-first 的 remount 路径,且必须先 captureCurrentPosition() 再 reloadToken.value++——顺序
// 颠倒会导致 remount 后阅读位置跳回开卷。实现只此一份,useReaderTypography/useReaderBookPrefs 均
// 经本函数触发,不各自另写一份 capture+bump。（另有 onReplChanged 直接 bump reloadToken、refreshText
// 经 requestRemount(false)，两者不涉及位置保持,无需 capture,不计入此两条。）
function captureCurrentPosition() {
  const cur = readerRef.value?.getCurrentLocation?.()
  if (cur) initialPos.value = cur.locator
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
const { bookEncoding, bookReflow, bookZhConvert, bookReaderTheme, onBookPrefsChange } =
  bookPrefsComposable

// 编辑/版本（§5.3，仅文本）：当前生效文本、编辑态与缓冲、当前版本 id、版本面板开关 + 动作（S8 + S27）。
const editVersion = useDocEditVersion({ id, requestRemount })
const {
  textContent,
  editing,
  editBuffer,
  editLabel,
  currentVersionId,
  showVersions,
  showProofread,
  startEdit,
  cancelEdit,
  saveNewVersion,
  overwriteSource,
  refreshText,
} = editVersion
// Markdown Editor POC 只保留当前窗口内的草稿，不写入版本链或源文件。
const markdownEditorDraft = ref<string | null>(null)

const url = computed(() => (detail.value ? convertFileSrc(detail.value.absPath) : ''))
const title = computed(() => detail.value?.fileName ?? t('routes.doc'))
// :key 含 reloadToken，使替换规则/版本变更后重建渲染器以重新套用。
const readerKey = computed(() => `${id.value}-${reloadToken.value}`)
// 替换仅支持 txt/epub（§5.2 首期）。
const supportsReplace = computed(() => kind.value === 'text' || kind.value === 'epub')
// 编辑 + 版本管理仅支持文本（§5.3）。
const supportsEdit = computed(() => kind.value === 'text')

// 格式 → 渲染器类别。文本渲染器只认 txt/md(阅读器方案 §6.4 扩展名清单收敛,2026-07-07)。
// 收敛依据:入库权威门 `src-tauri/src/utils/format.rs` 只放行 …txt md rtf odt… 等,
// 从不放行 markdown/log/json/csv/xml/yaml/yml/ini —— 它们在此前是**不可达死条目**(文件根本不进库),
// 一并删除防清单漂移(§2.5 记录了四处清单不一致)。
// rtf 移出:此前按纯文本渲染会直接暴露 `{\rtf1\ansi…` 控制字(坏体验),改回落「不支持→外部打开」
// (§5.13);gallery 文本卡样式仍含 rtf(见 MediaThumb.vue TEXT_CARD_FORMATS,职责不同)。
const TEXT_FORMATS = ['txt', 'md']
type MarkdownEngine = 'native' | 'vditor' | 'markdown-editor'
const kind = computed<'pdf' | 'epub' | 'text' | 'unsupported'>(() => {
  const f = (detail.value?.fileFormat ?? '').toLowerCase()
  if (f === 'pdf') return 'pdf'
  if (f === 'epub') return 'epub'
  if (TEXT_FORMATS.includes(f)) return 'text'
  return 'unsupported'
})
// 默认 txt/md 走 BookReader（R2-4/R2-4b）。isMarkdown 决定 SyntheticBook 构造分支。
const isMarkdown = computed(() => (detail.value?.fileFormat ?? '').toLowerCase() === 'md')
const isVditorMode = computed(() => isMarkdown.value && route.query.engine === 'vditor')
const isMarkdownEditorMode = computed(
  () => isMarkdown.value && route.query.engine === 'markdown-editor',
)
const isParallelEditorMode = computed(
  () => isVditorMode.value || isMarkdownEditorMode.value,
)
const selectedMarkdownEngine = computed<MarkdownEngine>(() => {
  if (isVditorMode.value) return 'vditor'
  if (isMarkdownEditorMode.value) return 'markdown-editor'
  return 'native'
})
// txt/md 的 SyntheticBook 文本源：版本/编码由后端解析，前端只传 itemId + isMarkdown + reflow（每书）。
const textSource = computed(() => ({
  itemId: id.value,
  isMarkdown: isMarkdown.value,
  reflow: bookReflow.value,
}))
// foliate 统一渲染器覆盖面：epub + text（txt/md）。仅 pdf 走独立滚动型渲染器。
const isFoliateKind = computed(() => kind.value === 'epub' || kind.value === 'text')
const isNativeFoliateKind = computed(() => isFoliateKind.value && !isParallelEditorMode.value)
const supportsNativeReplace = computed(() => supportsReplace.value && !isParallelEditorMode.value)
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
  if (isNativeFoliateKind.value) keys.push('readerFlow')
  if (kind.value === 'epub') keys.push('styleMode')
  if (isNativeFoliateKind.value)
    keys.push('search', 'toc', 'bookmarks', 'readerSettings', 'autoScroll', 'immersive')
  if (supportsNativeEdit.value && !editing.value) keys.push('edit')
  if (supportsNativeEdit.value) keys.push('versions', 'proofread')
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

// 书内搜索 / 导航(TOC) / 书签：三个互斥面板 composable，closeOtherPanels 由装配层回填闭包
// （§2.3：composable 之间不互相 import，此处引用彼此暴露的 ref/函数，非复制其逻辑）。三个
// closeOtherPanels 分别对应原 toggleSearch/toggleToc/toggleBookmarks 内联的「打开时关闭其余」。
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
    showToc.value = false
    showBookmarks.value = false
    closeSearch()
  }
}

// 全部可开合面板的开关。**新增面板必须登记于此**——进沉浸的收拢与 Esc 的分层退出都读它；
// 漏登记 = 沉浸态残留面板遮挡 + Esc 跳过面板层直接关掉阅读器，而没有任何门会红。
// showSearch 不在数组里：它的关闭另有清理（作废进行中迭代 + 清高亮），统一走 closeSearch()。
const panelFlags = [showToc, showReaderSettings, showBookmarks, showRepl, showVersions, showProofread]
const { anyPanelOpen, closeAllPanels } = useReaderPanels(panelFlags, closeSearch, showSearch)

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

function onReady() {
  // foliate 渲染器（epub + txt）自管滚轮/滚动、无对外滚动容器 → usePager 仅接键盘；
  // 滚动型渲染器（pdf + md 的 TextReader）绑定其滚动容器。
  pager.attach(isNativeFoliateKind.value ? null : (readerRef.value?.getScrollEl() ?? null))
}

// 阅读进度去抖保存（composable；红线：(itemId,pos) 配对不可简化为只存 pos，见 useReaderProgress
// 内注释，2026-07-10 审查 B12）。
const progress = useReaderProgress(id)
const { onProgress, flushProgress } = progress

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
  detail.value = null
  pageInfo.value = null
  initialPos.value = null
  textContent.value = null
  markdownEditorDraft.value = null
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
    initialPos.value = nextInitialPos
    if (!(await loadReplacer(itemId, generation))) return
    // 文本文档：取生效文本（当前版本或源）+ 当前版本 id（供编辑父版本）。
    const f = (d.fileFormat ?? '').toLowerCase()
    if (TEXT_FORMATS.includes(f)) {
      const nextTextContent = await invokeIpc<string>(IPC.GET_DOCUMENT_TEXT, {
        itemId,
      }).catch(() => null)
      if (!isCurrentDocumentLoad(generation, itemId)) return
      textContent.value = nextTextContent
      const cur = await invokeIpc<{ id: number } | null>(IPC.GET_CURRENT_VERSION, {
        itemId,
      }).catch(() => null)
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

function onMarkdownEngineChange(event: Event) {
  const target = event.currentTarget
  if (!(target instanceof HTMLSelectElement)) return
  setMarkdownEngine(normalizeMarkdownEngine(target.value))
}

function setMarkdownEngine(engine: MarkdownEngine) {
  if (!isMarkdown.value) return
  cancelEdit()
  closeAllPanels()
  showMoreMenu.value = false
  pager.detach()
  const query = { ...route.query }
  if (engine === 'native') delete query.engine
  else query.engine = engine
  void router.replace({ query })
}

function onMarkdownEditorChange(source: string) {
  markdownEditorDraft.value = source
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

// 沉浸态代理 + Esc 分层退出（composable；window keydown 监听器仍由本文件 onMounted/onBeforeUnmount
// 注册，保持原注册时机不变——红线，见 useDocImmersive.ts 顶部注释 / §3 风险 6）。
const docImmersive = useDocImmersive({ viewer, anyPanelOpen, closeAllPanels, goBack })
const { immersive, enterImmersive } = docImmersive

// activeViewer 单源接线（顶栏重构 P5 余项，composable）。viewerApi 内各 toggle 引用同一份函数
// 对象，不重建（红线，§3 风险 3：src/commands/builtins/viewer-reader.ts 按方法名调用）。
useDocActiveViewerSync({
  id,
  detail,
  title,
  immersive,
  goBack,
  toggleToc,
  toggleSearch,
  toggleBookmarks,
  toggleSettings: toggleReaderSettings,
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

onMounted(() => window.addEventListener('keydown', docImmersive.onGlobalKeydown))

onBeforeUnmount(() => {
  documentLoadGeneration++
  flushProgress()
  pager.detach()
  window.removeEventListener('keydown', docImmersive.onGlobalKeydown)
})
</script>

<style scoped src="./DocumentViewer.styles.css"></style>
