<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";

export type ThemeMode = "system" | "light" | "dark";

const props = defineProps<{
  themeMode: ThemeMode;
  rememberDownloadDir: boolean;
  lastDownloadDir: string;
}>();

const emit = defineEmits<{
  setTheme: [mode: ThemeMode];
  setRememberDownloadDir: [value: boolean];
  changeDownloadDir: [];
  clearDownloadDir: [];
  openTransferSettings: [];
}>();

const open = ref(false);
const root = ref<HTMLElement | null>(null);
const trigger = ref<HTMLButtonElement | null>(null);
const menu = ref<HTMLElement | null>(null);

const displayedDownloadDir = computed(() => {
  const path = props.lastDownloadDir;
  if (path.length <= 34) return path;
  // 保留末段目录，路径尾部比盘符和中间目录更利于识别当前下载位置。
  return `…${path.slice(-32)}`;
});

function focusMenuItem(index: number) {
  const items = Array.from(menu.value?.querySelectorAll<HTMLButtonElement>(".menu-item:not(:disabled)") ?? []);
  if (!items.length) return;
  items[(index + items.length) % items.length]?.focus();
}

async function showMenu(focus: "first" | "last" | null = null) {
  open.value = true;
  await nextTick();
  if (focus === "first") focusMenuItem(0);
  if (focus === "last") focusMenuItem(-1);
}

function close(restoreFocus = false) {
  if (!open.value) return;
  open.value = false;
  if (restoreFocus) void nextTick(() => trigger.value?.focus());
}

function toggleMenu() {
  if (open.value) close();
  else void showMenu();
}

function selectTheme(mode: ThemeMode) {
  emit("setTheme", mode);
  close();
}

function toggleRemember() {
  // 保持菜单打开，方便首次启用后紧接着设置目录。
  emit("setRememberDownloadDir", !props.rememberDownloadDir);
}

function changeDirectory() {
  emit("changeDownloadDir");
  close();
}

function clearDirectory() {
  emit("clearDownloadDir");
  close();
}

function openTransferSettings() {
  emit("openTransferSettings");
  close();
}

function onDocumentPointerDown(event: PointerEvent) {
  if (open.value && root.value && !root.value.contains(event.target as Node)) close();
}

function onDocumentKeydown(event: KeyboardEvent) {
  if (event.key === "Escape" && open.value) {
    event.preventDefault();
    close(true);
  }
}

function onTriggerKeydown(event: KeyboardEvent) {
  if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    void showMenu("first");
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    void showMenu("last");
  }
}

function onMenuKeydown(event: KeyboardEvent) {
  const items = Array.from(menu.value?.querySelectorAll<HTMLButtonElement>(".menu-item:not(:disabled)") ?? []);
  const current = items.indexOf(document.activeElement as HTMLButtonElement);
  if (event.key === "ArrowDown") {
    event.preventDefault();
    focusMenuItem(current + 1);
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    focusMenuItem(current - 1);
  } else if (event.key === "Home") {
    event.preventDefault();
    focusMenuItem(0);
  } else if (event.key === "End") {
    event.preventDefault();
    focusMenuItem(-1);
  } else if (event.key === "Escape") {
    event.preventDefault();
    close(true);
  }
}

onMounted(() => {
  document.addEventListener("pointerdown", onDocumentPointerDown);
  document.addEventListener("keydown", onDocumentKeydown);
});

onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", onDocumentPointerDown);
  document.removeEventListener("keydown", onDocumentKeydown);
});
</script>

<template>
  <div ref="root" class="config-menu">
    <button
      ref="trigger"
      class="menu-trigger"
      type="button"
      aria-haspopup="menu"
      :aria-expanded="open"
      @click="toggleMenu"
      @keydown="onTriggerKeydown"
    >
      <span aria-hidden="true">⚙</span> 配置 <span class="menu-caret" aria-hidden="true">▾</span>
    </button>

    <div
      v-if="open"
      ref="menu"
      class="menu-popover"
      role="menu"
      aria-label="配置"
      @keydown="onMenuKeydown"
    >
      <div class="menu-section-title">主题</div>
      <button
        v-for="item in [
          { value: 'system', label: '跟随系统' },
          { value: 'light', label: '浅色' },
          { value: 'dark', label: '深色' },
        ] as { value: ThemeMode; label: string }[]"
        :key="item.value"
        class="menu-item"
        type="button"
        role="menuitemradio"
        :aria-checked="themeMode === item.value"
        @click="selectTheme(item.value)"
      >
        <span class="menu-check" aria-hidden="true">{{ themeMode === item.value ? "✓" : "" }}</span>
        {{ item.label }}
      </button>

      <div class="menu-separator" role="separator" />

      <button
        class="menu-item"
        type="button"
        role="menuitemcheckbox"
        :aria-checked="rememberDownloadDir"
        @click="toggleRemember"
      >
        <span class="menu-check" aria-hidden="true">{{ rememberDownloadDir ? "✓" : "" }}</span>
        记住下载目录
      </button>
      <div v-if="lastDownloadDir" class="menu-path" :title="lastDownloadDir">
        <span>{{ displayedDownloadDir }}</span>
        <span v-if="!rememberDownloadDir" class="menu-path-state">已保存，未启用</span>
      </div>
      <button class="menu-item" type="button" role="menuitem" @click="changeDirectory">
        <span class="menu-check" aria-hidden="true" />
        {{ lastDownloadDir ? "更换下载目录…" : "设置下载目录…" }}
      </button>
      <button
        class="menu-item"
        type="button"
        role="menuitem"
        :disabled="!lastDownloadDir"
        @click="clearDirectory"
      >
        <span class="menu-check" aria-hidden="true" />
        清除已记住目录
      </button>

      <div class="menu-separator" role="separator" />

      <button class="menu-item" type="button" role="menuitem" @click="openTransferSettings">
        <span class="menu-check" aria-hidden="true" />
        传输设置…
      </button>
    </div>
  </div>
</template>

<style scoped>
.config-menu { position: relative; align-self: stretch; margin-right: 8px; }
.menu-trigger {
  height: 100%;
  padding: 0 12px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text);
  cursor: pointer;
  font: inherit;
}
.menu-trigger:hover,
.menu-trigger[aria-expanded="true"] { background: var(--hover); color: var(--primary); }
.menu-trigger:focus-visible,
.menu-item:focus-visible { outline: 2px solid var(--primary); outline-offset: -2px; }
.menu-caret { margin-left: 4px; font-size: 11px; }
.menu-popover {
  position: absolute;
  z-index: 30;
  top: calc(100% + 1px);
  right: 0;
  width: min(280px, calc(100vw - 16px));
  padding: 6px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--panel);
  box-shadow: 0 8px 24px var(--shadow);
}
.menu-section-title { padding: 5px 8px 3px 26px; color: var(--muted); font-size: 12px; }
.menu-item {
  display: flex;
  align-items: center;
  width: 100%;
  min-height: 30px;
  padding: 5px 8px;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--text);
  cursor: pointer;
  font: inherit;
  text-align: left;
}
.menu-item:hover:not(:disabled) { background: var(--hover); color: var(--primary); }
.menu-item:disabled { color: var(--muted); cursor: not-allowed; }
.menu-check { width: 18px; flex: none; color: var(--primary); font-weight: 700; }
.menu-path {
  display: grid;
  gap: 2px;
  overflow: hidden;
  padding: 3px 8px 5px 26px;
  color: var(--muted);
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.menu-path-state { color: var(--warn); font-size: 11px; }
.menu-separator { height: 1px; margin: 6px 2px; background: var(--border); }
</style>
