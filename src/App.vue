<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
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
import { errorMessage, formatBytes, uuid } from "./utils";
import Breadcrumb from "./components/Breadcrumb.vue";
import ObjectTable from "./components/ObjectTable.vue";
import TaskCenter from "./components/TaskCenter.vue";
import ConnectionDialog from "./components/ConnectionDialog.vue";
import ConfirmDialog from "./components/ConfirmDialog.vue";
import ConflictDialog from "./components/ConflictDialog.vue";
import PreviewModal from "./components/PreviewModal.vue";
import ObjectDetailModal from "./components/ObjectDetailModal.vue";
import SettingsDialog from "./components/SettingsDialog.vue";
import ConfigMenu from "./components/ConfigMenu.vue";
import Toast from "./components/Toast.vue";

// ---------- 连接对话框 ----------
const showConnectionDialog = ref(false);
const editingConnection = ref<ConnectionConfig | null>(null);

// ---------- 上传确认 ----------
const showUploadConfirm = ref(false);
const uploadPlan = ref<UploadPlan | null>(null);
const uploadPaths = ref<string[]>([]);
const uploadStorageClass = ref("STANDARD");

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
const showObjectDetail = ref(false);
const objectDetail = ref<import("./types").ObjectDetail | null>(null);
const showSettings = ref(false);

// 新建文件夹与历史 multipart 残片管理
const newFolderName = ref("");
const showFolderDialog = ref(false);
const showMultipartDialog = ref(false);
const multipartUploads = ref<import("./types").MultipartUploadInfo[]>([]);
const selectedMultipart = ref(new Set<string>());
const multipartParts = ref<import("./types").MultipartPartInfo[]>([]);
const multipartPartsKey = ref("");

// ---------- 拖拽上传 ----------
const dragging = ref(false);

// ---------- 本地界面偏好（不含连接或凭据） ----------
type ThemeMode = "system" | "light" | "dark";
const themeMode = ref<ThemeMode>((localStorage.getItem("easys3.theme") as ThemeMode) || "system");
const rememberDownloadDir = ref(localStorage.getItem("easys3.remember-download-dir") !== "false");
const lastDownloadDir = ref(localStorage.getItem("easys3.last-download-dir") || "");

function applyTheme(mode = themeMode.value) {
  const dark = mode === "dark" || (mode === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

function setTheme(mode: ThemeMode) {
  themeMode.value = mode;
  localStorage.setItem("easys3.theme", mode);
  applyTheme(mode);
}

function setRememberDownloadDir(value: boolean) {
  rememberDownloadDir.value = value;
}

function clearLastDownloadDir() {
  lastDownloadDir.value = "";
  localStorage.removeItem("easys3.last-download-dir");
}

async function changeDownloadDir() {
  const dir = await openDialog({ directory: true, title: "选择默认下载目录" });
  if (!dir) return;
  lastDownloadDir.value = dir;
  localStorage.setItem("easys3.last-download-dir", dir);
}

watch(rememberDownloadDir, (value) => localStorage.setItem("easys3.remember-download-dir", String(value)));

// ---------- 退出保护（规格：仍有进行中任务时关窗需二次确认） ----------
const showCloseConfirm = ref(false);

function runningTaskCount(): number {
  return store.tasksStore.tasks.filter(
    (t) => t.status === "running" || t.status === "queued",
  ).length;
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
  applyTheme();
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
    if (themeMode.value === "system") applyTheme();
  });
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

const copyMode = ref(false);

function openConnectionDialog(connection: ConnectionConfig | null, copy = false) {
  editingConnection.value = connection;
  copyMode.value = copy;
  showConnectionDialog.value = true;
}

/** 复制连接：以现有连接为模板新建（规格：预填配置、新 id、名称加后缀、测试结果不复制） */
function copyConnection() {
  const src = store.currentConnection();
  if (!src) return;
  openConnectionDialog(
    { ...src, id: uuid(), name: `${src.name} 副本`, last_test: null },
    true,
  );
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
      uploadStorageClass.value,
    );
    store.tasksStore.open = true;
    showUploadConfirm.value = false;
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
  const remembered = rememberDownloadDir.value ? lastDownloadDir.value : "";
  const usableRemembered = remembered && await api.isDownloadDirectory(remembered);
  if (remembered && !usableRemembered) {
    clearLastDownloadDir();
    store.toast("上次下载目录不存在，请重新选择", "error");
  }
  const dir = usableRemembered ? remembered : await openDialog({ directory: true, title: "选择保存位置" });
  if (!dir) return;
  if (rememberDownloadDir.value) {
    lastDownloadDir.value = dir;
    localStorage.setItem("easys3.last-download-dir", dir);
  }
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

async function changeDownloadDirForTask() {
  const dir = await openDialog({ directory: true, title: "选择保存位置" });
  if (!dir) return;
  downloadDest.value = dir;
  if (rememberDownloadDir.value) {
    lastDownloadDir.value = dir;
    localStorage.setItem("easys3.last-download-dir", dir);
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
    showDeleteConfirm.value = false;
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

async function detail(entry: Entry) {
  try {
    objectDetail.value = await api.objectDetail(store.browse.bucket, entry.key);
    showObjectDetail.value = true;
  } catch (e) { store.toast(errorMessage(e), "error"); }
}

// ---------- 桶内复制 / 移动 / 重命名（任务化，进入任务中心） ----------

const showCopyDialog = ref(false);
const copyRemove = ref(false);
/** null = 多选工具栏入口；Some(entry) = 行按钮单对象入口 */
const copySingleEntry = ref<Entry | null>(null);
const copyTargetPrefix = ref("");
const copyNewName = ref("");
const copyConflict = ref<"overwrite" | "skip">("overwrite");

function openCopyDialog(entry: Entry | null, remove: boolean) {
  const count = entry ? 1 : store.selection.size;
  if (!entry && !count) return;
  copySingleEntry.value = entry;
  copyRemove.value = remove;
  copyTargetPrefix.value = store.browse.prefix;
  copyNewName.value = entry && !entry.is_dir ? entry.name : "";
  copyConflict.value = "overwrite";
  showCopyDialog.value = true;
}

const copyDialogLabel = computed(() => {
  const op = copyRemove.value ? "移动" : "复制";
  const entry = copySingleEntry.value;
  if (!entry) return `${op} ${store.selection.size} 个对象`;
  return entry.is_dir ? `${op}文件夹` : `${op} / 重命名`;
});

async function startCopyTask() {
  const entry = copySingleEntry.value;
  const items: FsItem[] = entry
    ? [{ key: entry.key, is_dir: entry.is_dir }]
    : selectionItems();
  if (!items.length) return;
  // 单文件：新名称必填（预填原名，改名即重命名，改前缀即移动）
  let newName: string | null = null;
  if (entry && !entry.is_dir) {
    const name = copyNewName.value.trim();
    if (!name) {
      store.toast("新名称不能为空", "error");
      return;
    }
    if (name.includes("/")) {
      store.toast("新名称不能包含 /", "error");
      return;
    }
    newName = name;
  }
  let prefix = copyTargetPrefix.value.trim();
  if (prefix && !prefix.endsWith("/")) prefix += "/";
  if (prefix === store.browse.prefix && !newName && !entry?.is_dir) {
    store.toast("目标位置与当前位置相同", "error");
    return;
  }
  try {
    await api.startCopy(
      store.browse.bucket,
      items,
      prefix,
      copyConflict.value,
      newName,
      copyRemove.value,
    );
    showCopyDialog.value = false;
    store.toast(
      copyRemove.value ? "移动任务已开始，可在任务中心查看" : "复制任务已开始，可在任务中心查看",
    );
  } catch (e) {
    store.toast(errorMessage(e), "error");
  }
}

async function createFolder() {
  const name = newFolderName.value.trim();
  if (!name || name.includes("/")) {
    store.toast("文件夹名称不能为空且不能包含 /", "error");
    return;
  }
  if (store.browse.entries.some((entry) => entry.name === name)) {
    store.toast("当前目录已存在同名条目", "error");
    return;
  }
  try {
    await api.createFolder(store.browse.bucket, store.browse.prefix, name);
    showFolderDialog.value = false;
    newFolderName.value = "";
    await store.refresh();
    store.toast("文件夹已创建");
  } catch (e) { store.toast(errorMessage(e), "error"); }
}

async function openMultipartDialog() {
  try {
    multipartUploads.value = await api.listMultipartUploads(store.browse.bucket);
    selectedMultipart.value.clear();
    showMultipartDialog.value = true;
  } catch (e) { store.toast(errorMessage(e), "error"); }
}

function multipartKey(upload: import("./types").MultipartUploadInfo) { return `${upload.key}\u0000${upload.upload_id}`; }

async function abortSelectedMultipart() {
  const uploads = multipartUploads.value.filter((upload) => selectedMultipart.value.has(multipartKey(upload)));
  if (!uploads.length || !confirm(`将中止 ${uploads.length} 个未完成上传，操作不可恢复，是否继续？`)) return;
  try {
    // 返回清理失败的条目：成功项从列表移除，失败项保留供重试
    const failed = await api.abortMultipartUploads(store.browse.bucket, uploads);
    const failedKeys = new Set(failed.map((u) => multipartKey(u)));
    multipartUploads.value = multipartUploads.value.filter(
      (upload) => !selectedMultipart.value.has(multipartKey(upload)) || failedKeys.has(multipartKey(upload)),
    );
    selectedMultipart.value.clear();
    // 分片明细面板可能已失效，一并重置
    multipartParts.value = [];
    multipartPartsKey.value = "";
    if (failed.length) {
      store.toast(`已清理 ${uploads.length - failed.length} 项，${failed.length} 项失败可重试`, "error");
    } else {
      store.toast(`已清理 ${uploads.length} 项未完成上传`);
    }
  } catch (e) { store.toast(errorMessage(e), "error"); }
}

async function viewMultipartParts(upload: import("./types").MultipartUploadInfo) {
  try {
    multipartParts.value = await api.listMultipartParts(store.browse.bucket, upload.key, upload.upload_id);
    multipartPartsKey.value = upload.key;
  } catch (e) { store.toast(errorMessage(e), "error"); }
}

async function copyKey(entry: Entry) {
  try {
    await writeText(entry.key);
    store.toast("已复制 Key");
  } catch {
    store.toast("复制失败", "error");
  }
}

async function openFile(entry: Entry) {
  try {
    await api.openObject(store.browse.bucket, entry.key);
    store.toast("已开始下载到临时目录，完成后将用默认程序打开");
  } catch (e) {
    store.toast(errorMessage(e), "error");
  }
}
</script>

<template>
  <div class="app-shell">
    <header class="app-menu-bar">
      <div class="app-brand">easyS3</div>
      <div class="app-menu-spacer" />
      <ConfigMenu
        :theme-mode="themeMode"
        :remember-download-dir="rememberDownloadDir"
        :last-download-dir="lastDownloadDir"
        @set-theme="setTheme"
        @set-remember-download-dir="setRememberDownloadDir"
        @change-download-dir="changeDownloadDir"
        @clear-download-dir="clearLastDownloadDir"
        @open-transfer-settings="showSettings = true"
      />
    </header>

    <div class="app-frame">
      <aside class="sidebar">
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
            <button
              class="btn sm"
              :disabled="!store.app.currentConnectionId"
              @click="copyConnection"
            >
              复制
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
             <button class="btn" @click="showFolderDialog = true">新建文件夹</button>
             <button class="btn" @click="openMultipartDialog">碎片管理</button>
          </template>
          <span v-if="store.selection.size" class="selected-count">
            已选 {{ store.selection.size }} 项
          </span>
          <button class="btn" :disabled="!store.selection.size" @click="downloadSelection">
            下载
          </button>
          <button class="btn" :disabled="!store.selection.size" @click="openCopyDialog(null, false)">
            复制
          </button>
          <button class="btn" :disabled="!store.selection.size" @click="openCopyDialog(null, true)">
            移动
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
           @detail="detail"
           @open-file="openFile"
           @copy="openCopyDialog"
          @copy-key="copyKey"
          @download="(item) => requestDownload([item])"
          @del="(item) => requestDelete([item])"
          @load-more="store.loadMore"
        />
      </template>
      </main>
    </div>

    <TaskCenter />

    <ConnectionDialog
      v-model="showConnectionDialog"
      :connection="editingConnection"
      :copy="copyMode"
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
      <label class="upload-storage">存储类别
        <select v-model="uploadStorageClass" class="select">
          <option value="STANDARD">STANDARD</option>
          <option value="STANDARD_IA">STANDARD_IA</option>
          <option value="ONEZONE_IA">ONEZONE_IA</option>
          <option value="GLACIER">GLACIER</option>
          <option value="DEEP_ARCHIVE">DEEP_ARCHIVE</option>
        </select>
      </label>
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
      :dest="downloadDest"
      @change-dest="changeDownloadDirForTask"
      @choose="confirmDownload"
    />

    <PreviewModal v-model="showPreview" :name="previewName" :data="previewData" />
    <ObjectDetailModal v-model="showObjectDetail" :detail="objectDetail" />
    <SettingsDialog v-model="showSettings" />

    <div v-if="showFolderDialog" class="modal-mask">
      <div class="modal small-modal">
        <div class="modal-head">新建文件夹</div>
        <div class="modal-body"><input v-model="newFolderName" class="input" placeholder="文件夹名称" @keyup.enter="createFolder" /></div>
        <div class="modal-foot"><button class="btn" @click="showFolderDialog = false">取消</button><button class="btn primary" @click="createFolder">创建</button></div>
      </div>
    </div>

    <div v-if="showCopyDialog" class="modal-mask">
      <div class="modal small-modal">
        <div class="modal-head">{{ copyDialogLabel }}</div>
        <div class="modal-body copy-form">
          <label class="copy-row">
            <span class="copy-label">目标前缀</span>
            <input v-model="copyTargetPrefix" class="input" :placeholder="store.browse.prefix || '桶根（空）'" @keyup.enter="startCopyTask" />
          </label>
          <p v-if="copySingleEntry?.is_dir" class="copy-hint">
            将递归复制文件夹下所有层级（含目录占位对象）；修改前缀末段即重命名文件夹。
          </p>
          <label v-if="copySingleEntry && !copySingleEntry.is_dir" class="copy-row">
            <span class="copy-label">新名称</span>
            <input v-model="copyNewName" class="input" placeholder="保持原名称可不改" @keyup.enter="startCopyTask" />
          </label>
          <div class="copy-row">
            <span class="copy-label">同名冲突</span>
            <label class="copy-radio"><input v-model="copyConflict" type="radio" value="overwrite" />覆盖</label>
            <label class="copy-radio"><input v-model="copyConflict" type="radio" value="skip" />跳过</label>
          </div>
          <p class="copy-hint">服务端复制，不经过本机中转；超过 5GB 的对象会明确提示不支持。任务可在任务中心取消与重试。</p>
        </div>
        <div class="modal-foot">
          <button class="btn" @click="showCopyDialog = false">取消</button>
          <button class="btn primary" @click="startCopyTask">开始{{ copyRemove ? "移动" : "复制" }}</button>
        </div>
      </div>
    </div>

    <div v-if="showMultipartDialog" class="modal-mask">
      <div class="modal multipart-modal">
        <div class="modal-head">未完成上传（{{ multipartUploads.length }}）</div>
        <div class="modal-body">
          <div v-if="!multipartUploads.length" class="muted">当前桶没有未完成上传。</div>
          <label v-for="upload in multipartUploads" :key="multipartKey(upload)" class="multipart-row">
            <input v-model="selectedMultipart" type="checkbox" :value="multipartKey(upload)" />
            <span class="break">{{ upload.key }}</span><span class="muted">{{ upload.initiated || "—" }}</span>
            <button class="btn sm ghost" @click.prevent="viewMultipartParts(upload)">分片</button>
          </label>
          <div v-if="multipartPartsKey" class="parts-panel">
            <b>{{ multipartPartsKey }}（{{ multipartParts.length }} 片）</b>
            <div v-for="part in multipartParts" :key="part.part_number" class="muted">第 {{ part.part_number }} 片 · {{ formatBytes(part.size) }} · {{ part.last_modified || "—" }}</div>
          </div>
        </div>
        <div class="modal-foot"><button class="btn" @click="showMultipartDialog = false">关闭</button><button class="btn danger" :disabled="!selectedMultipart.size" @click="abortSelectedMultipart">中止所选</button></div>
      </div>
    </div>

    <Toast />

    <div v-if="dragging" class="drop-overlay">
      <div class="drop-tip">松开鼠标，上传到当前目录</div>
    </div>
  </div>
</template>

<style scoped>
.app-shell {
  display: flex;
  flex-direction: column;
  height: 100%;
}
.app-menu-bar {
  display: flex;
  align-items: stretch;
  height: 38px;
  flex: none;
  background: var(--panel);
  border-bottom: 1px solid var(--border);
}
.app-brand {
  display: flex;
  align-items: center;
  padding: 0 16px;
  color: var(--text);
  font-size: 17px;
  font-weight: 700;
  letter-spacing: 0.5px;
}
.app-menu-spacer { flex: 1; }
.app-frame {
  display: flex;
  min-height: 0;
  flex: 1;
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
.connection-box {
  padding: 12px;
}
.connection-box .select {
  width: 100%;
  margin-bottom: 8px;
}
.connection-actions {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
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
.selected-count {
  color: var(--muted);
  font-size: 12px;
  white-space: nowrap;
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
.small-modal { min-width: 360px; }
.copy-form { display: grid; gap: 10px; }
.copy-row { display: flex; align-items: center; gap: 10px; }
.copy-label { flex: none; width: 64px; color: var(--muted); }
.copy-radio { display: flex; align-items: center; gap: 4px; }
.copy-hint { margin: 0; font-size: 12px; color: var(--muted); }
.copy-form { display: grid; gap: 10px; }
.copy-row { display: flex; align-items: center; gap: 10px; }
.copy-label { width: 64px; flex-shrink: 0; color: var(--muted); }
.copy-radio { display: flex; align-items: center; gap: 4px; }
.copy-hint { margin: 0; font-size: 12px; color: var(--muted); }
.multipart-modal { width: min(760px, 94vw); }
.multipart-row { display: grid; grid-template-columns: 22px 1fr 180px 48px; gap: 8px; padding: 7px 0; border-bottom: 1px solid var(--border); }
.break { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.parts-panel { margin-top: 12px; border-top: 1px solid var(--border); padding-top: 10px; max-height: 180px; overflow: auto; }
</style>
