<script setup lang="ts">
import { onMounted, ref } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import * as api from "./api";
import * as store from "./store";
import type {
  ConflictPolicy,
  Entry,
  FsItem,
  PreviewData,
  ConnectionConfig,
  UploadPlan,
} from "./types";
import { errorMessage, formatBytes } from "./utils";
import Breadcrumb from "./components/Breadcrumb.vue";
import ObjectTable from "./components/ObjectTable.vue";
import TaskCenter from "./components/TaskCenter.vue";
import ConnectionDialog from "./components/ConnectionDialog.vue";
import ConfirmDialog from "./components/ConfirmDialog.vue";
import ConflictDialog from "./components/ConflictDialog.vue";
import PreviewModal from "./components/PreviewModal.vue";
import Toast from "./components/Toast.vue";

// ---------- 连接对话框 ----------
const showConnectionDialog = ref(false);
const editingConnection = ref<ConnectionConfig | null>(null);

// ---------- 上传确认 ----------
const showUploadConfirm = ref(false);
const uploadPlan = ref<UploadPlan | null>(null);
const uploadPaths = ref<string[]>([]);

// ---------- 删除确认 ----------
const showDeleteConfirm = ref(false);
const deleteItems = ref<FsItem[]>([]);
const deleteCount = ref(0);

// ---------- 下载冲突 ----------
const showConflict = ref(false);
const downloadItems = ref<FsItem[]>([]);
const downloadDest = ref("");

// ---------- 预览 ----------
const showPreview = ref(false);
const previewData = ref<PreviewData | null>(null);
const previewName = ref("");

// ---------- 拖拽上传 ----------
const dragging = ref(false);

// ---------- 退出保护（规格：仍有进行中任务时关窗需二次确认） ----------
const showCloseConfirm = ref(false);

function runningTaskCount(): number {
  return store.tasksStore.tasks.filter((t) => t.status === "running").length;
}

async function confirmQuit() {
  showCloseConfirm.value = false;
  await getCurrentWindow().destroy();
}

// 前缀筛选防抖（规格：前缀语义，非子串搜索）
let filterTimer: ReturnType<typeof setTimeout> | undefined;
function onFilterInput(e: Event) {
  const v = (e.target as HTMLInputElement).value;
  if (filterTimer) clearTimeout(filterTimer);
  filterTimer = setTimeout(() => void store.setFilter(v), 300);
}

onMounted(async () => {
  try {
    await store.refreshState();
    await store.initTasks();
    if (store.currentConnection()) await store.enterConnection();
  } catch (e) {
    store.toast(errorMessage(e), "error");
  }
  // 拖拽上传：任意路径拖入窗口即上传到当前目录（规格）
  await getCurrentWebview().onDragDropEvent((ev) => {
    const p = ev.payload;
    if (p.type === "enter" || p.type === "over") {
      dragging.value = true;
    } else {
      dragging.value = false;
    }
    if (p.type === "drop") void beginUpload(p.paths);
  });
  // 退出保护：仍有进行中任务时拦截关窗，二次确认后退出（规格）
  await getCurrentWindow().onCloseRequested((ev) => {
    if (!runningTaskCount()) return;
    ev.preventDefault();
    showCloseConfirm.value = true;
  });
});

// ---------- 连接 ----------

function openConnectionDialog(connection: ConnectionConfig | null) {
  editingConnection.value = connection;
  showConnectionDialog.value = true;
}

async function onConnectionChanged() {
  await store.refreshState();
  await store.enterConnection();
}

function onConnectionSelect(e: Event) {
  const id = (e.target as HTMLSelectElement).value;
  if (id) void store.switchConnection(id);
}

// ---------- 上传 ----------

async function pickUpload(directory: boolean) {
  const picked = await openDialog({ multiple: !directory, directory });
  if (!picked) return;
  const paths = Array.isArray(picked) ? picked : [picked];
  await beginUpload(paths);
}

async function beginUpload(paths: string[]) {
  if (store.browse.mode !== "objects") {
    store.toast("请先进入一个桶再上传", "error");
    return;
  }
  try {
    uploadPlan.value = await api.planUpload(paths, store.browse.prefix);
    uploadPaths.value = paths;
    showUploadConfirm.value = true;
  } catch (e) {
    store.toast(errorMessage(e), "error");
  }
}

async function confirmUpload() {
  if (!uploadPaths.value.length) return;
  try {
    await api.startUpload(
      store.browse.bucket,
      store.browse.prefix,
      uploadPaths.value,
    );
    store.tasksStore.open = true;
  } catch (e) {
    store.toast(errorMessage(e), "error");
  }
}

// ---------- 下载 ----------

function selectionItems(): FsItem[] {
  return store.browse.entries
    .filter((e) => store.selection.has(e.key))
    .map((e) => ({ key: e.key, is_dir: e.is_dir }));
}

async function requestDownload(items: FsItem[]) {
  if (!items.length) return;
  const dir = await openDialog({ directory: true, title: "选择保存位置" });
  if (!dir) return;
  downloadItems.value = items;
  downloadDest.value = dir;
  showConflict.value = true;
}

function downloadSelection() {
  void requestDownload(selectionItems());
}

async function confirmDownload(policy: ConflictPolicy) {
  showConflict.value = false;
  try {
    await api.startDownload(
      store.browse.bucket,
      downloadItems.value,
      downloadDest.value,
      policy,
    );
    store.tasksStore.open = true;
  } catch (e) {
    store.toast(errorMessage(e), "error");
  }
}

// ---------- 删除 ----------

async function requestDelete(items: FsItem[]) {
  if (!items.length) return;
  try {
    // 文件夹需递归统计实际对象数后再确认（规格）
    let count = 0;
    for (const it of items) {
      if (it.is_dir) {
        count += await api.countObjects(store.browse.bucket, it.key);
      } else {
        count += 1;
      }
    }
    deleteItems.value = items;
    deleteCount.value = count;
    showDeleteConfirm.value = true;
  } catch (e) {
    store.toast(errorMessage(e), "error");
  }
}

function deleteSelection() {
  void requestDelete(selectionItems());
}

async function confirmDelete() {
  try {
    await api.startDelete(store.browse.bucket, deleteItems.value);
    store.tasksStore.open = true;
    store.selection.clear();
  } catch (e) {
    store.toast(errorMessage(e), "error");
  }
}

// ---------- 预览 / 复制 Key ----------

async function preview(entry: Entry) {
  try {
    previewData.value = await api.previewObject(
      store.browse.bucket,
      entry.key,
    );
    previewName.value = entry.name;
    showPreview.value = true;
  } catch (e) {
    store.toast(errorMessage(e), "error");
  }
}

async function copyKey(entry: Entry) {
  try {
    await writeText(entry.key);
    store.toast("已复制 Key");
  } catch {
    store.toast("复制失败", "error");
  }
}
</script>

<template>
  <div class="app-shell">
    <aside class="sidebar">
      <div class="brand">easyS3</div>
      <div class="connection-box">
        <select
          class="select"
          :value="store.app.currentConnectionId ?? ''"
          @change="onConnectionSelect"
        >
          <option value="" disabled>选择连接…</option>
          <option v-for="p in store.app.connections" :key="p.id" :value="p.id">
            {{ p.name }}
          </option>
        </select>
        <div class="connection-actions">
          <button class="btn sm" @click="openConnectionDialog(null)">新建连接</button>
          <button
            class="btn sm"
            :disabled="!store.app.currentConnectionId"
            @click="openConnectionDialog(store.currentConnection())"
          >
            编辑
          </button>
        </div>
      </div>
      <div v-if="store.browse.mode === 'buckets'" class="bucket-list">
        <div class="bucket-title">桶列表</div>
        <div v-if="store.browse.loading" class="muted pad">加载中…</div>
        <template v-else>
          <div v-if="!store.browse.buckets.length" class="muted pad">
            暂无桶。若账号无 ListBuckets 权限，可在连接设置中填写限定桶。
          </div>
          <a
            v-for="b in store.browse.buckets"
            :key="b"
            class="bucket-item"
            :title="b"
            @click="store.selectBucket(b)"
          >
            <span class="bucket-icon">🪣</span>{{ b }}
          </a>
        </template>
      </div>
      <div
        v-else-if="store.browse.mode === 'objects' && !store.currentConnection()?.default_bucket"
        class="bucket-list"
      >
        <a class="bucket-item" title="返回桶列表" @click="store.backToBuckets()">
          <span class="bucket-icon">⬅️</span>返回桶列表
        </a>
      </div>
    </aside>

    <main class="main">
      <div v-if="!store.app.loaded" class="center muted">加载中…</div>

      <div v-else-if="!store.app.connections.length" class="center">
        <div class="empty-title">还没有连接</div>
        <p class="muted">
          连接 = 一条 S3 服务配置（endpoint、访问密钥等），支持配置多个并随时切换。
        </p>
        <button class="btn primary" @click="openConnectionDialog(null)">
          新建连接
        </button>
      </div>

      <div v-else-if="store.browse.mode === 'empty'" class="center">
        <div class="empty-title">请选择左侧连接</div>
      </div>

      <template v-else>
        <div class="toolbar">
          <Breadcrumb
            v-if="store.browse.mode === 'objects'"
            :bucket="store.browse.bucket"
            :prefix="store.browse.prefix"
            :show-buckets-root="!store.currentConnection()?.default_bucket"
            @navigate="store.navigatePrefix"
            @show-buckets="store.backToBuckets()"
          />
          <div v-else class="bucket-label">桶列表</div>
          <div class="spacer" />
          <template v-if="store.browse.mode === 'objects'">
            <input
              class="input filter"
              placeholder="按前缀筛选"
              :value="store.browse.filter"
              @input="onFilterInput"
            />
            <button class="btn" @click="pickUpload(false)">上传文件</button>
            <button class="btn" @click="pickUpload(true)">上传文件夹</button>
          </template>
          <button class="btn" :disabled="!store.selection.size" @click="downloadSelection">
            下载
          </button>
          <button
            class="btn danger"
            :disabled="!store.selection.size"
            @click="deleteSelection"
          >
            删除
          </button>
          <button class="btn ghost" @click="store.refresh()">刷新</button>
        </div>

        <div v-if="store.browse.error" class="error-banner">
          {{ store.browse.error }}
        </div>

        <ObjectTable
          :entries="store.browse.entries"
          :loading="store.browse.loading"
          :has-more="!!store.browse.nextToken"
          @open="store.openEntry"
          @preview="preview"
          @copy-key="copyKey"
          @download="(item) => requestDownload([item])"
          @del="(item) => requestDelete([item])"
          @load-more="store.loadMore"
        />
      </template>
    </main>

    <TaskCenter />

    <ConnectionDialog
      v-model="showConnectionDialog"
      :connection="editingConnection"
      @saved="onConnectionChanged"
      @deleted="onConnectionChanged"
    />

    <ConfirmDialog
      v-model="showUploadConfirm"
      title="确认上传"
      ok-text="开始上传"
      @confirm="confirmUpload"
    >
      <p v-if="uploadPlan">
        将上传 <b>{{ uploadPlan.files.length }}</b> 个文件到
        <b>{{ store.browse.prefix || "桶根目录" }}</b>
        ，共 {{ formatBytes(uploadPlan.total_bytes) }}。
      </p>
      <p class="warn-text">注意：同名对象将被覆盖。</p>
    </ConfirmDialog>

    <ConfirmDialog
      v-model="showDeleteConfirm"
      title="确认删除"
      ok-text="删除"
      danger
      @confirm="confirmDelete"
    >
      <p>
        即将删除 <b>{{ deleteCount }}</b> 个对象（含文件夹内的全部对象）。
      </p>
      <p class="warn-text">删除操作不可恢复，请确认。</p>
    </ConfirmDialog>

    <ConfirmDialog
      v-model="showCloseConfirm"
      title="确认退出"
      ok-text="仍要退出"
      danger
      @confirm="confirmQuit"
    >
      <p>当前仍有 <b>{{ runningTaskCount() }}</b> 个进行中的传输任务。</p>
      <p class="warn-text">退出会中断这些任务，且进度不会保留。</p>
    </ConfirmDialog>

    <ConflictDialog
      v-model="showConflict"
      :count="downloadItems.length"
      @choose="confirmDownload"
    />

    <PreviewModal v-model="showPreview" :name="previewName" :data="previewData" />

    <Toast />

    <div v-if="dragging" class="drop-overlay">
      <div class="drop-tip">松开鼠标，上传到当前目录</div>
    </div>
  </div>
</template>

<style scoped>
.app-shell {
  display: flex;
  height: 100%;
}
.sidebar {
  width: var(--sidebar-w);
  flex-shrink: 0;
  background: var(--panel);
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  overflow-y: auto;
}
.brand {
  padding: 16px;
  font-size: 18px;
  font-weight: 700;
  letter-spacing: 0.5px;
}
.connection-box {
  padding: 0 12px 12px;
}
.connection-box .select {
  width: 100%;
  margin-bottom: 8px;
}
.connection-actions {
  display: flex;
  gap: 6px;
}
.connection-actions .btn {
  flex: 1;
}
.bucket-list {
  border-top: 1px solid var(--border);
  padding: 12px;
  flex: 1;
}
.bucket-title {
  color: var(--muted);
  margin-bottom: 8px;
}
.bucket-list .pad {
  padding: 4px 0;
}
.bucket-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  border-radius: 6px;
  cursor: pointer;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.bucket-item:hover {
  background: rgba(51, 112, 255, 0.07);
  color: var(--primary);
}
.bucket-icon {
  flex-shrink: 0;
}
.main {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  overflow: hidden;
}
.center {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
}
.empty-title {
  font-size: 16px;
  font-weight: 600;
}
.toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 16px;
  flex-wrap: wrap;
}
.toolbar .spacer {
  flex: 1;
}
.bucket-label {
  font-weight: 600;
}
.filter {
  width: 160px;
}
.drop-overlay {
  position: fixed;
  inset: 0;
  background: rgba(51, 112, 255, 0.12);
  border: 2px dashed var(--primary);
  z-index: 400;
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
}
.drop-tip {
  background: var(--primary);
  color: #fff;
  padding: 12px 24px;
  border-radius: 8px;
  font-size: 15px;
}
</style>
