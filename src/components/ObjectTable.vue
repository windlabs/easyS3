<script setup lang="ts">
import { computed, ref } from "vue";
import type { Entry, FsItem } from "../types";
import { formatBytes, formatTime } from "../utils";
import { selection } from "../store";

const props = defineProps<{ entries: Entry[]; loading: boolean; hasMore: boolean }>();

const emit = defineEmits<{
  open: [entry: Entry];
  preview: [entry: Entry];
  detail: [entry: Entry];
  copy: [entry: Entry, remove: boolean];
  copyKey: [entry: Entry];
  openFile: [entry: Entry];
  download: [item: FsItem];
  del: [item: FsItem];
  loadMore: [];
}>();

type SortKey = "name" | "size" | "time" | "storage";
const sortKey = ref<SortKey>("name");
const sortAsc = ref(true);
const showStorage = ref(localStorage.getItem("easys3.show-storage-column") !== "false");

function toggleStorageColumn() {
  showStorage.value = !showStorage.value;
  localStorage.setItem("easys3.show-storage-column", String(showStorage.value));
}

function setSort(k: SortKey) {
  if (sortKey.value === k) {
    sortAsc.value = !sortAsc.value;
  } else {
    sortKey.value = k;
    sortAsc.value = true;
  }
}

// 文件夹始终在前（规格默认：名称升序、文件夹在前），组内按所选列排序
const sorted = computed(() => {
  const dir = sortAsc.value ? 1 : -1;
  const cmp = (a: Entry, b: Entry): number => {
    switch (sortKey.value) {
      case "size":
        return (a.size - b.size) * dir;
      case "time":
        return (a.last_modified ?? "").localeCompare(b.last_modified ?? "") * dir;
      case "storage":
        return (a.storage_class ?? "STANDARD").localeCompare(b.storage_class ?? "STANDARD") * dir;
      default:
        return a.name.localeCompare(b.name, "zh") * dir;
    }
  };
  const dirs = props.entries.filter((e) => e.is_dir).sort(cmp);
  const files = props.entries.filter((e) => !e.is_dir).sort(cmp);
  return [...dirs, ...files];
});

const allChecked = computed(
  () => sorted.value.length > 0 && sorted.value.every((e) => selection.has(e.key)),
);

function toggleAll() {
  if (allChecked.value) {
    for (const e of sorted.value) selection.delete(e.key);
  } else {
    for (const e of sorted.value) selection.add(e.key);
  }
}

function toggle(e: Entry) {
  if (selection.has(e.key)) {
    selection.delete(e.key);
  } else {
    selection.add(e.key);
  }
}

function asItem(e: Entry): FsItem {
  return { key: e.key, is_dir: e.is_dir };
}

// 滚动到底自动加载下一页（规格）
const wrap = ref<HTMLElement | null>(null);
function onScroll() {
  const el = wrap.value;
  if (!el || props.loading || !props.hasMore) return;
  if (el.scrollTop + el.clientHeight >= el.scrollHeight - 40) emit("loadMore");
}
</script>

<template>
  <div ref="wrap" class="table-wrap" @scroll.passive="onScroll">
    <table class="obj-table">
      <thead>
        <tr>
          <th class="col-check">
            <input type="checkbox" :checked="allChecked" @change="toggleAll" />
          </th>
          <th class="col-name sortable" @click="setSort('name')">
            名称<span class="arrow">{{ sortKey === "name" ? (sortAsc ? "↑" : "↓") : "" }}</span>
          </th>
          <th class="col-size sortable" @click="setSort('size')">
            大小<span class="arrow">{{ sortKey === "size" ? (sortAsc ? "↑" : "↓") : "" }}</span>
          </th>
          <th class="col-time sortable" @click="setSort('time')">
            修改时间<span class="arrow">{{ sortKey === "time" ? (sortAsc ? "↑" : "↓") : "" }}</span>
          </th>
          <th v-if="showStorage" class="col-storage sortable" @click="setSort('storage')">
            存储类别<span class="arrow">{{ sortKey === "storage" ? (sortAsc ? "↑" : "↓") : "" }}</span>
          </th>
          <th class="col-actions">操作</th>
        </tr>
      </thead>
      <tbody>
        <tr v-if="!sorted.length && !loading">
          <td :colspan="showStorage ? 6 : 5" class="empty-cell muted">当前目录为空</td>
        </tr>
        <tr
          v-for="e in sorted"
          :key="e.key"
          :class="{ selected: selection.has(e.key) }"
        >
          <td class="col-check">
            <input type="checkbox" :checked="selection.has(e.key)" @change="toggle(e)" />
          </td>
          <td class="col-name">
            <span class="file-icon">{{ e.is_dir ? "📁" : "📄" }}</span>
            <a
              v-if="e.is_dir"
              href="#"
              class="name-link"
              :title="e.key"
              @click.prevent="emit('open', e)"
              >{{ e.name }}</a
            >
            <span v-else class="name-text" :title="e.key">{{ e.name }}</span>
          </td>
          <td class="col-size muted">{{ e.is_dir ? "—" : formatBytes(e.size) }}</td>
          <td class="col-time muted">{{ e.is_dir ? "—" : formatTime(e.last_modified) }}</td>
          <td v-if="showStorage" class="col-storage muted">{{ e.is_dir ? "—" : (e.storage_class || "STANDARD") }}</td>
          <td class="col-actions">
            <button v-if="!e.is_dir" class="btn sm ghost" @click="emit('preview', e)">预览</button>
            <button v-if="!e.is_dir" class="btn sm ghost" @click="emit('detail', e)">详情</button>
            <button v-if="!e.is_dir" class="btn sm ghost" @click="emit('openFile', e)">打开</button>
            <button v-if="!e.is_dir" class="btn sm ghost" @click="emit('copy', e, false)">复制</button>
            <button v-if="!e.is_dir" class="btn sm ghost" @click="emit('copy', e, true)">移动/改名</button>
            <button class="btn sm ghost" @click="emit('copyKey', e)">复制Key</button>
            <button class="btn sm ghost" @click="emit('download', asItem(e))">下载</button>
            <button class="btn sm ghost danger" @click="emit('del', asItem(e))">删除</button>
          </td>
        </tr>
      </tbody>
    </table>
    <button class="column-toggle btn sm ghost" @click="toggleStorageColumn">
      {{ showStorage ? "隐藏存储类别" : "显示存储类别" }}
    </button>
    <div v-if="loading" class="table-foot muted">加载中…</div>
    <div v-else-if="hasMore" class="table-foot muted">向下滚动加载更多</div>
  </div>
</template>

<style scoped>
.table-wrap {
  flex: 1;
  overflow: auto;
  background: var(--panel);
  margin: 0 16px 16px;
  border: 1px solid var(--border);
  border-radius: 8px;
}
.obj-table {
  width: 100%;
  border-collapse: collapse;
}
.obj-table th,
.obj-table td {
  padding: 8px 10px;
  text-align: left;
  border-bottom: 1px solid var(--border);
  white-space: nowrap;
}
.obj-table thead th {
  position: sticky;
  top: 0;
  background: var(--header-bg);
  font-weight: 500;
  color: var(--muted);
  z-index: 1;
}
.obj-table tbody tr:hover {
  background: var(--hover);
}
.obj-table tbody tr.selected {
  background: var(--selected);
}
.sortable {
  cursor: pointer;
  user-select: none;
}
.sortable:hover {
  color: var(--primary);
}
.arrow {
  margin-left: 2px;
  font-size: 11px;
}
.col-check {
  width: 36px;
}
.col-name {
  max-width: 420px;
}
.col-size {
  width: 110px;
}
.col-time {
  width: 150px;
}
.col-storage { width: 120px; }
.col-actions {
  width: 240px;
}
.col-actions .btn {
  margin-right: 2px;
}
.file-icon {
  margin-right: 6px;
}
.name-link {
  color: var(--text);
  text-decoration: none;
}
.name-link:hover {
  color: var(--primary);
}
.name-text,
.name-link {
  display: inline-block;
  max-width: 360px;
  overflow: hidden;
  text-overflow: ellipsis;
  vertical-align: bottom;
}
.empty-cell {
  text-align: center;
  padding: 40px 0;
}
.table-foot {
  text-align: center;
  padding: 10px 0;
}
.column-toggle { margin: 6px 10px; }
</style>
