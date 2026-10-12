<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref, watch } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { listen } from "@tauri-apps/api/event";
import { check } from "@tauri-apps/plugin-updater";
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
  SyncPlan,
  UsageStats,
  AuditEntry,
  SearchEntry,
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
const uploadChangesOnly = ref(false);
const uploadVerifyAfter = ref(false);
const uploadEncryption = ref("none");
const uploadKmsKeyId = ref("");

// ---------- 删除确认 ----------
const showDeleteConfirm = ref(false);
const deleteItems = ref<FsItem[]>([]);
const deleteCount = ref(0);
const deleteToRecycle = ref(false);
const showRecycle = ref(false);
const recycleEntries = ref<Entry[]>([]);
const recyclePrefix = ref("");
const recycleSelected = ref(new Set<string>());
const recycleRestorePrefix = ref("");

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
const showConnectionTransfer = ref(false);
const includeExportSecrets = ref(false);
const connectionTransferPassword = ref("");
const importOverwrite = ref(false);
const awsProfiles = ref<ConnectionConfig[]>([]);
const showSearch = ref(false);
const searchQuery = ref("");
const searchAcrossBuckets = ref(false);
const searchResults = ref<SearchEntry[]>([]);
const searching = ref(false);
const searchError = ref("");
const inventoryResults = ref<import("./types").InventoryEntry[]>([]);
const showSyncDialog = ref(false);
const syncDirection = ref<"upload" | "download">("upload");
const syncLocalDir = ref("");
const syncPropagateDelete = ref(false);
const syncPlan = ref<SyncPlan | null>(null);
const syncPlanning = ref(false);
const newBucketName = ref("");
const showBucketDialog = ref(false);
const showBucketManage = ref(false);
const managedBucket = ref("");
const bucketVersionStatus = ref("");
const bucketFilter = ref("");
const showPresign = ref(false);
const presignMode = ref<"get" | "put">("get");
const presignSeconds = ref(3600);
const presignKey = ref("");
const presignUrls = ref<string[]>([]);
const showUsage = ref(false);
const usageStats = ref<UsageStats | null>(null);
const showCost = ref(false);
const costBuckets = ref<{ bucket: string; stats: UsageStats }[]>([]);
const costLoading = ref(false);
const costPoints = ref<{ date: string; amount: string; unit: string }[]>([]);
const costServices = ref<{ service: string; amount: string; unit: string }[]>([]);
const costError = ref("");
const usageLoading = ref(false);
const gridView = ref(localStorage.getItem("easys3.grid-view") === "true");
function toggleGridView() { gridView.value = !gridView.value; localStorage.setItem("easys3.grid-view", String(gridView.value)); }
const showCommandPalette = ref(false);
const updateMessage = ref("");
const showAudit = ref(false);
const auditEntries = ref<AuditEntry[]>([]);
const auditDays = ref(0);
const showCodeExport = ref(false);
const showPolicy = ref(false);
const policyText = ref("");
const policyOriginal = ref("");
const policyResource = ref<"policy" | "cors" | "lifecycle" | "website">("policy");
const objectLockInfo = ref<import("./types").ObjectLockInfo | null>(null);
const policyDiff = computed(() => {
  const oldLines = policyOriginal.value.split("\n"); const newLines = policyText.value.split("\n"); const max = Math.max(oldLines.length, newLines.length); const out: { kind: string; text: string }[] = [];
  for (let index = 0; index < max; index++) { if (oldLines[index] === newLines[index]) continue; if (oldLines[index] !== undefined) out.push({ kind: "removed", text: `- ${oldLines[index]}` }); if (newLines[index] !== undefined) out.push({ kind: "added", text: `+ ${newLines[index]}` }); }
  return out;
});
const codeExportFormat = ref("terraform");
const codeExportResult = ref("");
const showSelect = ref(false);
const selectExpression = ref("SELECT * FROM s3object s LIMIT 100");
const selectFormat = ref<"csv" | "json">("csv");
const selectResult = ref<import("./types").SelectResult | null>(null);
const commandQuery = ref("");
const isReadOnly = computed(() => !!store.currentConnection()?.read_only);

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

function onGlobalKeydown(event: KeyboardEvent) {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") { event.preventDefault(); showCommandPalette.value = true; commandQuery.value = ""; }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "r") { event.preventDefault(); void store.refresh(); }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "u") { event.preventDefault(); if (!isReadOnly.value) void pickUpload(false); }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") { event.preventDefault(); showSearch.value = true; }
  if (event.key === "Escape") showCommandPalette.value = false;
}
onMounted(() => window.addEventListener("keydown", onGlobalKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onGlobalKeydown));
function onPaste(event: ClipboardEvent) {
  if (isReadOnly.value || store.browse.mode !== "objects") return;
  const files = Array.from(event.clipboardData?.files ?? []);
  // Tauri WebView 在支持 file path 的平台会暴露 path；无 path 时再回退为图片读取。
  const paths = files.map((file) => (file as File & { path?: string }).path).filter((path): path is string => !!path);
  if (paths.length) { event.preventDefault(); void beginUpload(paths); return; }
  if (files.some((file) => file.type.startsWith("image/"))) { event.preventDefault(); void pasteUpload(); }
}
onMounted(() => window.addEventListener("paste", onPaste));
onBeforeUnmount(() => window.removeEventListener("paste", onPaste));
async function pasteUpload() {
  if (isReadOnly.value || store.browse.mode !== "objects") return;
  try { const path = await api.pasteClipboardImage(); if (path) await beginUpload([path]); } catch { /* 非图片剪贴板静默忽略 */ }
}

const commands = computed(() => [
  { label: "刷新", key: "Ctrl/Cmd+R", run: () => void store.refresh() },
  { label: "上传文件", key: "Ctrl/Cmd+U", run: () => { if (!isReadOnly.value) void pickUpload(false); } },
  { label: "全局搜索", key: "Ctrl/Cmd+F", run: () => { showSearch.value = true; } },
  { label: "新窗口", key: "", run: () => void openBrowserWindow() },
  { label: "返回桶列表", key: "", run: () => void store.backToBuckets() },
].filter((item) => item.label.toLowerCase().includes(commandQuery.value.toLowerCase())));

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
    uploadChangesOnly.value = false;
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
    if (uploadChangesOnly.value && uploadPlan.value) {
      await api.startUploadPlan(store.browse.bucket, uploadPlan.value, uploadStorageClass.value, uploadVerifyAfter.value, uploadEncryption.value, uploadKmsKeyId.value || null);
    } else {
      await api.startUpload(store.browse.bucket, store.browse.prefix, uploadPaths.value, uploadStorageClass.value, uploadVerifyAfter.value, uploadEncryption.value, uploadKmsKeyId.value || null);
    }
    store.tasksStore.open = true;
    showUploadConfirm.value = false;
  } catch (e) {
    store.toast(errorMessage(e), "error");
  }
}

async function refreshUploadPlan() {
  if (!uploadPaths.value.length) return;
  try {
    uploadPlan.value = uploadChangesOnly.value
      ? await api.planUploadChanges(store.browse.bucket, uploadPaths.value, store.browse.prefix)
      : await api.planUpload(uploadPaths.value, store.browse.prefix);
  } catch (e) { store.toast(errorMessage(e), "error"); }
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

async function downloadZipSelection() {
  const items = selectionItems();
  if (!items.length) return;
  const path = await (await import("@tauri-apps/plugin-dialog")).save({
    title: "保存 ZIP 文件",
    defaultPath: "easyS3-download.zip",
    filters: [{ name: "ZIP", extensions: ["zip"] }],
  });
  if (!path) return;
  try {
    await api.startZipDownload(store.browse.bucket, items, path);
    store.tasksStore.open = true;
    store.toast("ZIP 打包下载任务已开始");
  } catch (e) { store.toast(errorMessage(e), "error"); }
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
    deleteToRecycle.value = false;
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
    const recyclePrefix = store.currentConnection()?.recycle_prefix;
    if (deleteToRecycle.value && recyclePrefix) {
      const prefix = `${recyclePrefix.replace(/\/+$/, "")}/${new Date().toISOString().replace(/[:.]/g, "-")}/`;
      await api.startCopy(store.browse.bucket, deleteItems.value, prefix, store.browse.bucket, "overwrite", null, true);
    } else {
      await api.startDelete(store.browse.bucket, deleteItems.value);
    }
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
async function savePreviewText(content: string) {
  if (!previewName.value || !confirm("将覆盖远端对象，他人此间的修改会被覆盖。是否继续？")) return;
  try { await api.saveTextObject(store.browse.bucket, store.browse.prefix + previewName.value, content); previewData.value = { kind: "text", content }; await store.refresh(); store.toast("文本已保存"); }
  catch (e) { store.toast(errorMessage(e), "error"); }
}
async function runSelect() { if (!objectDetail.value) return; try { const id = selectFormat.value === "json" ? await api.startSelectJsonObject(store.browse.bucket, objectDetail.value.key, selectExpression.value) : await api.startSelectCsvObject(store.browse.bucket, objectDetail.value.key, selectExpression.value); store.tasksStore.open = true; const unlisten = await listen<import("./types").SelectResult & { id: string; error: string | null }>("query-result", (event) => { if (event.payload.id !== id) return; if (event.payload.error) store.toast(event.payload.error, "error"); else selectResult.value = event.payload; unlisten(); }); } catch (e) { store.toast(errorMessage(e), "error"); } }
async function exportSelectResult(json: boolean) { if (!selectResult.value) return; const path = await (await import("@tauri-apps/plugin-dialog")).save({ title: "导出查询结果", defaultPath: json ? "s3-select-result.json" : "s3-select-result.csv" }); if (!path) return; const content = json ? JSON.stringify({ records: selectResult.value.records, bytes_scanned: selectResult.value.bytes_scanned, bytes_processed: selectResult.value.bytes_processed, bytes_returned: selectResult.value.bytes_returned }, null, 2) : selectResult.value.records; try { await api.exportPresignList(path, [content]); store.toast("查询结果已导出"); } catch (e) { store.toast(errorMessage(e), "error"); } }

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
const copyTargetBucket = ref("");
const copyNewName = ref("");
const copyConflict = ref<"overwrite" | "skip">("overwrite");

function openCopyDialog(entry: Entry | null, remove: boolean) {
  const count = entry ? 1 : store.selection.size;
  if (!entry && !count) return;
  copySingleEntry.value = entry;
  copyRemove.value = remove;
  copyTargetPrefix.value = store.browse.prefix;
  copyTargetBucket.value = store.browse.bucket;
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
  if (copyTargetBucket.value === store.browse.bucket && prefix === store.browse.prefix && !newName && !entry?.is_dir) {
    store.toast("目标位置与当前位置相同", "error");
    return;
  }
  try {
    await api.startCopy(
      store.browse.bucket,
      items,
      prefix,
      copyTargetBucket.value,
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

function openPresign(mode: "get" | "put") {
  if (store.browse.mode !== "objects") return;
  if (mode === "get" && !store.selection.size) {
    store.toast("请先选择要分享的对象", "error");
    return;
  }
  presignMode.value = mode;
  presignKey.value = mode === "put" ? store.browse.prefix : "";
  presignUrls.value = [];
  showPresign.value = true;
}

async function generatePresign() {
  if (!Number.isInteger(presignSeconds.value) || presignSeconds.value < 1 || presignSeconds.value > 604800) {
    store.toast("有效期必须在 1 秒至 7 天之间", "error");
    return;
  }
  try {
    presignUrls.value = presignMode.value === "get"
      ? await api.presignGet(store.browse.bucket, selectionItems().filter((item) => !item.is_dir).map((item) => item.key), presignSeconds.value)
      : [await api.presignPut(store.browse.bucket, presignKey.value, presignSeconds.value)];
    if (!presignUrls.value.length) store.toast("文件夹不能直接生成下载链接，请选择文件", "error");
  } catch (e) { store.toast(errorMessage(e), "error"); }
}

async function copyPresignUrls() {
  try { await writeText(presignUrls.value.join("\n")); store.toast("链接已复制"); }
  catch { store.toast("复制失败", "error"); }
}

async function exportPresignUrls() {
  const path = await (await import("@tauri-apps/plugin-dialog")).save({ title: "导出链接清单", defaultPath: "easyS3-links.txt" });
  if (!path) return;
  try {
    await api.exportPresignList(path, presignUrls.value);
    store.toast("链接清单已导出");
  } catch (e) { store.toast(errorMessage(e), "error"); }
}

async function openFile(entry: Entry) {
  try {
    await api.openObject(store.browse.bucket, entry.key);
    store.toast("已开始下载到临时目录，完成后将用默认程序打开");
  } catch (e) {
    store.toast(errorMessage(e), "error");
  }
}
async function loadRecycle() { const prefix = store.currentConnection()?.recycle_prefix || ""; if (!prefix) return; try { const result = await api.listObjects(store.browse.bucket, prefix, null); recyclePrefix.value = prefix; recycleEntries.value = result.entries; recycleSelected.value.clear(); recycleRestorePrefix.value = store.browse.prefix; showRecycle.value = true; } catch (e) { store.toast(errorMessage(e), "error"); } }
async function emptyRecycle() { if (!recycleEntries.value.length || !confirm(`将永久删除回收站中 ${recycleEntries.value.length} 项，是否继续？`)) return; try { await api.startDelete(store.browse.bucket, recycleEntries.value.map((entry) => ({ key: entry.key, is_dir: entry.is_dir }))); store.toast("回收站清空任务已开始"); showRecycle.value = false; } catch (e) { store.toast(errorMessage(e), "error"); } }
function recycleItem(entry: Entry) { return { key: entry.key, is_dir: entry.is_dir }; }
async function restoreRecycle() { const selected = recycleEntries.value.filter((entry) => recycleSelected.value.has(entry.key)); if (!selected.length) return; const prefix = recycleRestorePrefix.value.trim() ? `${recycleRestorePrefix.value.trim().replace(/\/+$/, "")}/` : ""; if (!confirm(`将恢复 ${selected.length} 项到 ${prefix || "桶根"}，是否继续？`)) return; try { await api.startCopy(store.browse.bucket, selected.map(recycleItem), prefix, store.browse.bucket, "overwrite", null, true); store.toast("回收站恢复任务已开始；请选择的对象将移动到目标前缀"); showRecycle.value = false; } catch (e) { store.toast(errorMessage(e), "error"); } }
async function loadUsage() { usageLoading.value = true; usageStats.value = null; try { const id = await api.startPrefixUsage(store.browse.bucket, store.browse.prefix); store.tasksStore.open = true; const unlisten = await listen<{ id: string; object_count: number; total_bytes: number; error: string | null }>("usage-result", (event) => { if (event.payload.id !== id) return; if (event.payload.error) store.toast(event.payload.error, "error"); else usageStats.value = { object_count: event.payload.object_count, total_bytes: event.payload.total_bytes }; usageLoading.value = false; unlisten(); }); } catch (e) { store.toast(errorMessage(e), "error"); usageLoading.value = false; } }
async function loadCostFallback() { costLoading.value = true; costBuckets.value = []; costPoints.value = []; costServices.value = []; costError.value = ""; try { [costPoints.value, costServices.value] = await Promise.all([api.costOverview(), api.costByService()]); } catch (e) { costError.value = errorMessage(e); try { for (const bucket of store.browse.buckets) { const stats = await api.prefixUsage(bucket, ""); costBuckets.value.push({ bucket, stats }); } } catch (fallback) { store.toast(errorMessage(fallback), "error"); } } finally { costLoading.value = false; } }
async function exportUsage() { if (!usageStats.value) return; const path = await (await import("@tauri-apps/plugin-dialog")).save({ title: "导出用量统计", defaultPath: "easys3-usage.json" }); if (!path) return; try { await api.exportPresignList(path, [JSON.stringify({ bucket: store.browse.bucket, prefix: store.browse.prefix, ...usageStats.value })]); store.toast("统计已导出"); } catch (e) { store.toast(errorMessage(e), "error"); } }

async function openBrowserWindow() {
  try { await api.openBrowserWindow(); }
  catch (e) { store.toast(errorMessage(e), "error"); }
}

async function checkForUpdate() {
  updateMessage.value = "正在检查更新…";
  try {
    const update = await check();
    updateMessage.value = update ? `发现新版本 ${update.version}，将在下载完成后安装。` : "当前已是最新版本。";
    if (update) { await update.downloadAndInstall(); }
  } catch (e) { updateMessage.value = `检查更新失败：${errorMessage(e)}`; }
}
async function generateCodeExport() { try { codeExportResult.value = await api.generateIac(store.browse.bucket, codeExportFormat.value); } catch (e) { store.toast(errorMessage(e), "error"); } }
async function loadPolicyResource() { try { policyText.value = policyResource.value === "policy" ? await api.bucketPolicy(store.browse.bucket) : policyResource.value === "cors" ? JSON.stringify(await api.bucketCors(store.browse.bucket), null, 2) : policyResource.value === "lifecycle" ? JSON.stringify(await api.bucketLifecycle(store.browse.bucket), null, 2) : JSON.stringify(await api.bucketWebsite(store.browse.bucket), null, 2); policyOriginal.value = policyText.value; objectLockInfo.value = await api.objectLock(store.browse.bucket); } catch (e) { store.toast(errorMessage(e), "error"); } }
async function openPolicy() { showPolicy.value = true; await loadPolicyResource(); }
async function savePolicy() { if (!confirm(`将覆盖当前 ${policyResource.value === "policy" ? "Bucket Policy" : policyResource.value === "cors" ? "CORS 配置" : policyResource.value === "lifecycle" ? "Lifecycle 配置" : "静态网站配置"}，是否继续？`)) return; try { if (policyResource.value === "policy") await api.saveBucketPolicy(store.browse.bucket, policyText.value, policyOriginal.value); else if (policyResource.value === "cors") await api.saveBucketCors(store.browse.bucket, JSON.parse(policyText.value), JSON.parse(policyOriginal.value)); else if (policyResource.value === "lifecycle") await api.saveBucketLifecycle(store.browse.bucket, JSON.parse(policyText.value), JSON.parse(policyOriginal.value)); else await api.saveBucketWebsite(store.browse.bucket, JSON.parse(policyText.value), JSON.parse(policyOriginal.value)); policyOriginal.value = policyText.value; store.toast("桶配置已保存"); } catch (e) { store.toast(errorMessage(e), "error"); } }
async function copyCodeExport() { try { await writeText(codeExportResult.value); store.toast("代码已复制"); } catch { store.toast("复制失败", "error"); } }
async function loadAudit() { const since = auditDays.value ? Math.floor(Date.now()/1000) - auditDays.value * 86400 : null; try { auditEntries.value = await api.getAudit(store.app.currentConnectionId, since); } catch (e) { store.toast(errorMessage(e), "error"); } }
async function exportAudit() { const path = await (await import("@tauri-apps/plugin-dialog")).save({ title: "导出操作历史", defaultPath: "easys3-history.json" }); if (!path) return; try { const since = auditDays.value ? Math.floor(Date.now()/1000) - auditDays.value * 86400 : null; await api.exportAudit(path, store.app.currentConnectionId, since); store.toast("操作历史已导出"); } catch (e) { store.toast(errorMessage(e), "error"); } }

async function exportConnectionConfig() {
  if (includeExportSecrets.value && !confirm("导出文件将包含可用凭据，请妥善保管，是否继续？")) return;
  const path = await (await import("@tauri-apps/plugin-dialog")).save({ title: "导出连接配置", defaultPath: "easyS3-connections.json" });
  if (!path) return;
  try { await api.exportConnections(path, includeExportSecrets.value, includeExportSecrets.value ? connectionTransferPassword.value : null); store.toast("连接配置已导出"); }
  catch (e) { store.toast(errorMessage(e), "error"); }
}
async function importConnectionConfig() {
  const path = await openDialog({ title: "导入连接配置", filters: [{ name: "JSON", extensions: ["json"] }] });
  if (!path || Array.isArray(path)) return;
  try { const count = await api.importConnections(path, importOverwrite.value, connectionTransferPassword.value || null); await store.refreshState(); store.toast(`已导入 ${count} 个连接`); }
  catch (e) { store.toast(errorMessage(e), "error"); }
}
async function loadAwsProfiles() {
  try { awsProfiles.value = await api.importAwsProfiles(); }
  catch (e) { store.toast(errorMessage(e), "error"); }
}
async function saveAwsProfiles() {
  let count = 0;
  try { for (const profile of awsProfiles.value) { await api.saveConnection(profile); count++; } await store.refreshState(); store.toast(`已导入 ${count} 个 AWS profile`); awsProfiles.value = []; }
  catch (e) { store.toast(errorMessage(e), "error"); }
}

async function runSearch() {
  searching.value = true; searchError.value = ""; searchResults.value = [];
  try {
    const id = await api.startSearch(searchAcrossBuckets.value ? null : store.browse.bucket, searchQuery.value);
    store.tasksStore.open = true;
    const unlisten = await listen<{ id: string; results: SearchEntry[]; error: string | null }>("search-result", (event) => {
      if (event.payload.id !== id) return;
      searchResults.value = event.payload.results; searchError.value = event.payload.error || ""; searching.value = false; unlisten();
    });
  }
  catch (e) { searchError.value = errorMessage(e); }
}
async function importInventory() { const path = await openDialog({ title: "导入 S3 Inventory CSV", filters: [{ name: "CSV", extensions: ["csv"] }] }); if (!path || Array.isArray(path)) return; try { const count = await api.importInventory(path); store.toast(`已导入 ${count} 条 Inventory 索引`); } catch (e) { store.toast(errorMessage(e), "error"); } }
async function runInventorySearch() { try { inventoryResults.value = await api.searchInventory(searchQuery.value); } catch (e) { store.toast(errorMessage(e), "error"); } }
async function jumpToSearch(entry: SearchEntry) {
  if (entry.bucket !== store.browse.bucket) await store.selectBucket(entry.bucket);
  const index = entry.key.lastIndexOf("/");
  await store.navigatePrefix(index >= 0 ? entry.key.slice(0, index + 1) : "");
  showSearch.value = false;
}

async function openSync(direction: "upload" | "download") {
  if (store.browse.mode !== "objects") return;
  const dir = await openDialog({ directory: true, title: direction === "upload" ? "选择要同步到 S3 的本地文件夹" : "选择同步到本地的文件夹" });
  if (!dir) return;
  syncDirection.value = direction; syncLocalDir.value = dir; syncPropagateDelete.value = false; syncPlan.value = null; showSyncDialog.value = true;
  await buildSyncPlan();
}
async function buildSyncPlan() {
  if (!syncLocalDir.value) return;
  syncPlanning.value = true;
  try { syncPlan.value = syncDirection.value === "upload" ? await api.planSyncUpload(store.browse.bucket, store.browse.prefix, syncLocalDir.value, syncPropagateDelete.value) : await api.planSyncDownload(store.browse.bucket, store.browse.prefix, syncLocalDir.value, syncPropagateDelete.value); }
  catch (e) { store.toast(errorMessage(e), "error"); }
  finally { syncPlanning.value = false; }
}
async function startSync() {
  if (!syncPlan.value) return;
  try {
    if (syncPlan.value.delete.length && !confirm(`同步将删除 ${syncPlan.value.delete.length} 项，操作不可恢复，是否继续？`)) return;
    const ids = syncDirection.value === "upload" ? await api.startSyncUpload(store.browse.bucket, syncPlan.value) : await api.startSyncDownload(store.browse.bucket, syncPlan.value);
    store.tasksStore.open = true; showSyncDialog.value = false; store.toast(`同步任务已开始（${ids.length} 个任务）`);
  } catch (e) { store.toast(errorMessage(e), "error"); }
}

async function createBucket() {
  const name = newBucketName.value.trim();
  if (!name) return;
  try { await api.createBucket(name); showBucketDialog.value = false; newBucketName.value = ""; await store.refresh(); store.toast("桶已创建"); }
  catch (e) { store.toast(errorMessage(e), "error"); }
}
async function manageBucket(bucket: string) {
  managedBucket.value = bucket; bucketVersionStatus.value = "加载中…"; showBucketManage.value = true;
  try { bucketVersionStatus.value = (await api.bucketVersioning(bucket)).status; }
  catch (e) { bucketVersionStatus.value = errorMessage(e); }
}
async function toggleBucketVersioning() {
  const enabled = bucketVersionStatus.value !== "Enabled";
  try { await api.setBucketVersioning(managedBucket.value, enabled); bucketVersionStatus.value = enabled ? "Enabled" : "Suspended"; store.toast(enabled ? "已开启版本控制" : "已暂停版本控制"); }
  catch (e) { store.toast(errorMessage(e), "error"); }
}
async function removeBucket() {
  if (!confirm(`确认删除桶“${managedBucket.value}”？桶必须为空，此操作不可恢复。`)) return;
  try { await api.deleteBucket(managedBucket.value); showBucketManage.value = false; await store.refresh(); store.toast("桶已删除"); }
  catch (e) { store.toast(errorMessage(e), "error"); }
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
        @manage-connections="showConnectionTransfer = true"
        :task-notifications="store.notifyOnTaskFinish.enabled"
        @set-task-notifications="store.setNotifyOnTaskFinish"
        @check-update="checkForUpdate"
        @open-audit="showAudit = true; loadAudit()"
        @open-cost="showCost = true; loadCostFallback()"
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
          <div class="bucket-title">桶列表 <button class="btn sm" @click="showBucketDialog = true">新建桶</button></div>
          <input v-model="bucketFilter" class="input bucket-filter" placeholder="筛选已加载桶名" />
          <div v-if="store.browse.loading" class="muted pad">加载中…</div>
          <template v-else>
            <div v-if="!store.browse.buckets.length" class="muted pad">
              暂无桶。若账号无 ListBuckets 权限，可在连接设置中填写限定桶。
            </div>
            <div v-else-if="!store.browse.buckets.some((bucket) => bucket.toLowerCase().includes(bucketFilter.trim().toLowerCase()))" class="muted pad">没有匹配的桶。</div>
            <a
              v-for="b in store.browse.buckets.filter((bucket) => bucket.toLowerCase().includes(bucketFilter.trim().toLowerCase()))"
              :key="b"
              class="bucket-item"
              :title="b"
              @click="store.selectBucket(b)"
            >
              <span class="bucket-icon">🪣</span>{{ b }}<button class="btn sm bucket-manage" @click.stop="manageBucket(b)">管理</button>
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
            <button class="btn" @click="openBrowserWindow">新窗口</button>
          <template v-if="store.browse.mode === 'objects'">
            <input
              class="input filter"
              placeholder="按前缀筛选"
              :value="store.browse.filter"
              @input="onFilterInput"
            />
              <button class="btn" @click="showSearch = true">全局搜索</button>
              <button class="btn" @click="toggleGridView">{{ gridView ? "表格视图" : "网格视图" }}</button>
              <button v-if="!isReadOnly" class="btn" @click="pickUpload(false)">上传文件</button>
               <button v-if="!isReadOnly" class="btn" @click="pickUpload(true)">上传文件夹</button>
               <button v-if="!isReadOnly" class="btn" @click="openSync('upload')">同步到 S3</button>
              <button class="btn" @click="openSync('download')">同步到本地</button>
              <button v-if="!isReadOnly" class="btn" @click="showFolderDialog = true">新建文件夹</button>
              <button class="btn" @click="openMultipartDialog">碎片管理</button>
              <button v-if="store.currentConnection()?.recycle_prefix" class="btn" @click="loadRecycle">回收站</button>
              <button class="btn" @click="showUsage = true; loadUsage()">统计</button>
              <button class="btn" @click="showCodeExport = true; generateCodeExport()">导出 IaC</button>
              <button v-if="!isReadOnly" class="btn" @click="openPolicy">Bucket Policy</button>
              <button class="btn" @click="openPresign('put')">生成上传链接</button>
          </template>
          <span v-if="store.selection.size" class="selected-count">
            已选 {{ store.selection.size }} 项
          </span>
          <button class="btn" :disabled="!store.selection.size" @click="downloadSelection">
            下载
          </button>
          <button class="btn" :disabled="!store.selection.size" @click="downloadZipSelection">下载 ZIP</button>
          <button class="btn" :disabled="!store.selection.size" @click="openPresign('get')">分享链接</button>
          <button class="btn" :disabled="store.selection.size !== 1" @click="objectDetail = store.browse.entries.find((entry) => store.selection.has(entry.key)) ? ({ key: store.browse.entries.find((entry) => store.selection.has(entry.key))!.key, size: 0, last_modified: null, e_tag: null, storage_class: null, content_type: null, metadata: [] }) : null; showSelect = !!objectDetail">S3 Select</button>
           <button v-if="!isReadOnly" class="btn" :disabled="!store.selection.size" @click="openCopyDialog(null, false)">
            复制
          </button>
           <button v-if="!isReadOnly" class="btn" :disabled="!store.selection.size" @click="openCopyDialog(null, true)">
            移动
          </button>
           <button v-if="!isReadOnly"
            class="btn danger"
            :disabled="!store.selection.size"
            @click="deleteSelection"
          >
            删除
           </button>
           <span v-if="isReadOnly" class="selected-count">🔒 当前连接为只读模式</span>
          <button class="btn ghost" @click="store.refresh()">刷新</button>
        </div>

        <div v-if="store.browse.error" class="error-banner">
          {{ store.browse.error }}
        </div>

        <ObjectTable
          :entries="store.browse.entries"
          :loading="store.browse.loading"
          :has-more="!!store.browse.nextToken"
          :grid="gridView"
          :bucket="store.browse.bucket"
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
       <p v-if="uploadPlan?.empty_folders.length">另将创建 {{ uploadPlan.empty_folders.length }} 个空文件夹。</p>
       <p v-if="uploadPlan?.skipped">将跳过 {{ uploadPlan.skipped }} 个大小未变化的文件。</p>
       <p class="warn-text">注意：同名对象将被覆盖。</p>
       <label class="upload-storage"><input v-model="uploadChangesOnly" type="checkbox" @change="refreshUploadPlan" /> 仅上传新增 / 变更文件</label>
       <label class="upload-storage"><input v-model="uploadVerifyAfter" type="checkbox" /> 上传后回读 ETag 校验（分片文件自动跳过）</label>
       <label class="upload-storage">存储类别
        <select v-model="uploadStorageClass" class="select">
          <option value="STANDARD">STANDARD</option>
          <option value="STANDARD_IA">STANDARD_IA</option>
          <option value="ONEZONE_IA">ONEZONE_IA</option>
          <option value="GLACIER">GLACIER</option>
          <option value="DEEP_ARCHIVE">DEEP_ARCHIVE</option>
        </select>
       </label>
       <label class="upload-storage">服务端加密<select v-model="uploadEncryption" class="select"><option value="none">不加密</option><option value="sse-s3">SSE-S3（AES-256）</option><option value="sse-kms">SSE-KMS</option><option value="sse-c">SSE-C（会话密钥）</option></select></label>
       <input v-if="uploadEncryption === 'sse-kms'" v-model="uploadKmsKeyId" class="input" placeholder="KMS Key ID（可选）" />
       <input v-if="uploadEncryption === 'sse-c'" v-model="uploadKmsKeyId" class="input" type="password" placeholder="Base64 编码的 32 字节 SSE-C 密钥（不会保存）" />
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
      <label v-if="store.currentConnection()?.recycle_prefix" class="upload-storage"><input v-model="deleteToRecycle" type="checkbox" /> 移入连接回收站（{{ store.currentConnection()?.recycle_prefix }}）</label>
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

    <PreviewModal v-model="showPreview" :name="previewName" :data="previewData" @save-text="savePreviewText" />
    <ObjectDetailModal v-model="showObjectDetail" :detail="objectDetail" :bucket="store.browse.bucket" />
    <div v-if="showSelect" class="modal-mask"><div class="modal policy-modal"><div class="modal-head">S3 Select</div><div class="modal-body copy-form"><label>输入格式 <select v-model="selectFormat" class="select"><option value="csv">CSV（首行列名）</option><option value="json">JSON Lines</option></select></label><textarea v-model="selectExpression" class="input" /><p class="copy-hint">扫描量可能产生 S3 Select 费用，请确认查询范围与 SQL 条件。</p><button class="btn" @click="runSelect">执行查询</button><p v-if="selectResult" class="muted">扫描 {{ formatBytes(selectResult.bytes_scanned) }} · 处理 {{ formatBytes(selectResult.bytes_processed) }} · 返回 {{ formatBytes(selectResult.bytes_returned) }}</p><pre v-if="selectResult" class="policy-diff">{{ selectResult.records }}</pre></div><div class="modal-foot"><button class="btn" @click="showSelect = false">关闭</button><button class="btn" :disabled="!selectResult" @click="exportSelectResult(false)">导出 CSV</button><button class="btn primary" :disabled="!selectResult" @click="exportSelectResult(true)">导出 JSON</button></div></div></div>
    <SettingsDialog v-model="showSettings" />
    <div v-if="showUsage" class="modal-mask"><div class="modal small-modal"><div class="modal-head">桶 / 前缀用量统计</div><div class="modal-body"><p class="warn-text">大桶统计会产生较多列表请求，可能耗时较长。</p><p v-if="usageLoading">正在递归扫描…</p><template v-else-if="usageStats"><p>对象数：<b>{{ usageStats.object_count }}</b></p><p>总大小：<b>{{ formatBytes(usageStats.total_bytes) }}</b></p></template></div><div class="modal-foot"><button class="btn" @click="showUsage = false">关闭</button><button class="btn" :disabled="usageLoading" @click="loadUsage">重新统计</button><button class="btn primary" :disabled="!usageStats" @click="exportUsage">导出 JSON</button></div></div></div>
    <div v-if="showRecycle" class="modal-mask"><div class="modal audit-modal"><div class="modal-head">回收站（{{ recyclePrefix }}）</div><div class="modal-body"><p class="copy-hint">回收站对象默认隐藏于正常浏览。恢复后会移除回收站副本。</p><label>恢复目标前缀 <input v-model="recycleRestorePrefix" class="input" placeholder="桶根（留空）" /></label><div class="audit-list"><label v-for="entry in recycleEntries" :key="entry.key" class="audit-row"><input v-model="recycleSelected" type="checkbox" :value="entry.key" /><span>{{ entry.key }}</span><span>{{ entry.is_dir ? '文件夹' : formatBytes(entry.size) }}</span></label><p v-if="!recycleEntries.length" class="muted">回收站为空。</p></div></div><div class="modal-foot"><button class="btn" @click="showRecycle = false">关闭</button><button class="btn" :disabled="!recycleSelected.size" @click="restoreRecycle">恢复所选</button><button class="btn danger" :disabled="!recycleEntries.length" @click="emptyRecycle">清空回收站</button></div></div></div>
    <div v-if="showCost" class="modal-mask"><div class="modal audit-modal"><div class="modal-head">成本 / 用量概览</div><div class="modal-body"><p v-if="costError" class="warn-text">Cost Explorer 不可用：{{ costError }}。以下为按桶扫描的存储字节量，不代表费用。</p><p v-if="costLoading">正在读取成本 / 统计各桶字节量…</p><div v-else class="audit-list"><div v-for="item in costPoints" :key="item.date" class="audit-row"><span>{{ item.date }}</span><span>真实费用</span><span>{{ item.amount }} {{ item.unit }}</span></div><div v-for="item in costServices" :key="item.service" class="audit-row"><span>{{ item.service }}</span><span>服务费用</span><span>{{ item.amount }} {{ item.unit }}</span></div><div v-for="item in costBuckets" :key="item.bucket" class="audit-row"><span>{{ item.bucket }}</span><span>对象 {{ item.stats.object_count }}</span><span>{{ formatBytes(item.stats.total_bytes) }}</span></div></div></div><div class="modal-foot"><button class="btn" @click="showCost = false">关闭</button><button class="btn primary" :disabled="costLoading" @click="loadCostFallback">重新统计</button></div></div></div>
    <div v-if="updateMessage" class="update-toast" @click="updateMessage = ''">{{ updateMessage }}</div>
    <div v-if="showAudit" class="modal-mask"><div class="modal audit-modal"><div class="modal-head">本机操作历史</div><div class="modal-body copy-form"><label>时间范围 <select v-model.number="auditDays" class="select" @change="loadAudit"><option :value="0">全部</option><option :value="1">最近 1 天</option><option :value="7">最近 7 天</option><option :value="30">最近 30 天</option></select></label><div class="audit-list"><div v-for="entry in auditEntries" :key="`${entry.at}-${entry.action}-${entry.detail}`" class="audit-row"><span>{{ new Date(entry.at * 1000).toLocaleString() }}</span><span>{{ entry.connection }} / {{ entry.bucket }}</span><span>{{ entry.action }} · {{ entry.status }}</span><span>{{ entry.detail || '—' }}</span></div><p v-if="!auditEntries.length" class="muted">暂无符合条件的历史记录。</p></div></div><div class="modal-foot"><button class="btn" @click="showAudit = false">关闭</button><button class="btn primary" @click="exportAudit">导出 JSON</button></div></div></div>
    <div v-if="showCodeExport" class="modal-mask"><div class="modal small-modal"><div class="modal-head">导出桶 IaC</div><div class="modal-body copy-form"><label>格式 <select v-model="codeExportFormat" class="select" @change="generateCodeExport"><option value="terraform">Terraform</option><option value="cloudformation">CloudFormation</option><option value="cdk">CDK</option></select></label><textarea class="input code-export" readonly :value="codeExportResult" /></div><div class="modal-foot"><button class="btn" @click="showCodeExport = false">关闭</button><button class="btn primary" @click="copyCodeExport">复制</button></div></div></div>
    <div v-if="showPolicy" class="modal-mask"><div class="modal policy-modal"><div class="modal-head">桶配置编辑</div><div class="modal-body copy-form"><label>资源 <select v-model="policyResource" class="select" @change="loadPolicyResource"><option value="policy">Bucket Policy</option><option value="cors">CORS</option><option value="lifecycle">Lifecycle</option><option value="website">静态网站</option></select></label><p class="copy-hint">保存前请检查变更。ACL 写入不在此功能范围内。</p><div v-if="objectLockInfo" class="object-lock">Object Lock（只读）：{{ objectLockInfo.enabled ? `已启用 · ${objectLockInfo.mode || '未设置模式'} · ${objectLockInfo.days ? `${objectLockInfo.days} 天` : objectLockInfo.years ? `${objectLockInfo.years} 年` : '未设置保留期'}` : '未启用' }}</div><textarea v-model="policyText" class="input policy-editor" /><details><summary>查看结构化文本 diff（{{ policyDiff.length }} 行）</summary><pre class="policy-diff"><span v-for="(line,index) in policyDiff" :key="index" :class="line.kind">{{ line.text }}
</span></pre></details></div><div class="modal-foot"><button class="btn" @click="showPolicy = false">关闭</button><button class="btn primary" @click="savePolicy">保存</button></div></div></div>
    <div v-if="showCommandPalette" class="modal-mask"><div class="modal command-modal"><div class="modal-head">命令面板 <span class="muted">Ctrl/Cmd+K</span></div><div class="modal-body copy-form"><input v-model="commandQuery" class="input" autofocus placeholder="输入命令名称…" /><button v-for="command in commands" :key="command.label" class="command-row" @click="command.run(); showCommandPalette = false"><span>{{ command.label }}</span><kbd>{{ command.key }}</kbd></button><p class="copy-hint">快捷键：Ctrl/Cmd+R 刷新、Ctrl/Cmd+U 上传、Ctrl/Cmd+F 搜索、Ctrl/Cmd+K 命令面板。</p></div><div class="modal-foot"><button class="btn" @click="showCommandPalette = false">关闭</button></div></div></div>
    <div v-if="showConnectionTransfer" class="modal-mask"><div class="modal transfer-modal"><div class="modal-head">导入 / 导出连接</div><div class="modal-body copy-form"><section><b>导出</b><label><input v-model="includeExportSecrets" type="checkbox" /> 包含凭据（默认脱敏，需妥善保管）</label><input v-if="includeExportSecrets" v-model="connectionTransferPassword" class="input" type="password" placeholder="含凭据导出密码（必填）" /><button class="btn" @click="exportConnectionConfig">导出到文件…</button></section><section><b>导入配置文件</b><input v-model="connectionTransferPassword" class="input" type="password" placeholder="如导入文件加密，请输入密码" /><label><input v-model="importOverwrite" type="checkbox" /> 同名连接覆盖（否则跳过）</label><button class="btn" @click="importConnectionConfig">选择文件导入…</button></section><section><b>AWS CLI profile</b><button class="btn" @click="loadAwsProfiles">读取 ~/.aws/credentials</button><div v-if="awsProfiles.length" class="aws-profiles"><div v-for="profile in awsProfiles" :key="profile.id">{{ profile.name }} · {{ profile.region }}</div><button class="btn primary" @click="saveAwsProfiles">导入全部 profile</button></div></section></div><div class="modal-foot"><button class="btn primary" @click="showConnectionTransfer = false">关闭</button></div></div></div>

    <div v-if="showSearch" class="modal-mask"><div class="modal search-modal"><div class="modal-head">全局搜索与高级过滤</div><div class="modal-body copy-form"><input v-model="searchQuery" class="input" placeholder="例如 size>10MB AND modified>30d AND class=GLACIER" @keyup.enter="runSearch" /><label><input v-model="searchAcrossBuckets" type="checkbox" /> 搜索当前连接的所有桶</label><p class="copy-hint">语法：size&gt;10MB、size&lt;1GB、modified&gt;30d、class=GLACIER；使用 AND 连接。</p><div v-if="searching" class="muted">正在分页搜索，可继续操作其他界面…</div><div v-if="searchError" class="version-error">{{ searchError }}</div><div v-if="searchResults.length" class="search-results"><button v-for="item in searchResults" :key="`${item.bucket}/${item.key}`" class="search-row" @click="jumpToSearch(item)"><b>{{ item.bucket }}</b><span>{{ item.key }}</span><span>{{ formatBytes(item.size) }}</span></button></div><div v-if="inventoryResults.length" class="search-results"><b>本地 Inventory 索引结果（零 S3 API）</b><div v-for="item in inventoryResults" :key="`inventory-${item.bucket}/${item.key}`" class="search-row"><b>{{ item.bucket }}</b><span>{{ item.key }}</span><span>{{ formatBytes(item.size) }}</span></div></div><div v-else-if="!searching && !searchError" class="muted">输入条件后搜索；空条件可列出范围内所有文件。</div></div><div class="modal-foot"><button class="btn" @click="showSearch = false">关闭</button><button class="btn" @click="importInventory">导入 Inventory CSV</button><button class="btn" @click="runInventorySearch">搜索本地索引</button><button class="btn primary" :disabled="searching" @click="runSearch">搜索 S3</button></div></div></div>

    <div v-if="showSyncDialog" class="modal-mask"><div class="modal sync-modal"><div class="modal-head">{{ syncDirection === "upload" ? "本地文件夹同步到 S3" : "S3 前缀同步到本地" }}</div><div class="modal-body copy-form"><p>本地目录：<b class="break">{{ syncLocalDir }}</b></p><p>目标：<b>{{ syncDirection === "upload" ? `${store.browse.bucket}/${store.browse.prefix}` : store.browse.prefix || `${store.browse.bucket} 根目录` }}</b></p><label><input v-model="syncPropagateDelete" type="checkbox" @change="buildSyncPlan" /> 同步删除（默认关闭，需确认后才执行）</label><div v-if="syncPlanning" class="muted">正在生成计划…</div><template v-else-if="syncPlan"><p>计划：新增 <b>{{ syncPlan.create.length }}</b>，更新 <b>{{ syncPlan.update.length }}</b>，删除 <b>{{ syncPlan.delete.length }}</b>，跳过 <b>{{ syncPlan.skipped }}</b>。</p><div class="sync-items"><div v-for="item in [...syncPlan.create, ...syncPlan.update, ...syncPlan.delete].slice(0, 20)" :key="item.source + item.target">{{ item.action }} · {{ item.source }}</div><div v-if="syncPlan.create.length + syncPlan.update.length + syncPlan.delete.length > 20" class="muted">仅显示前 20 项</div></div></template></div><div class="modal-foot"><button class="btn" @click="showSyncDialog = false">关闭</button><button class="btn" :disabled="syncPlanning" @click="buildSyncPlan">重新比较</button><button class="btn primary" :disabled="syncPlanning || !syncPlan" @click="startSync">开始同步</button></div></div></div>

    <div v-if="showBucketDialog" class="modal-mask"><div class="modal small-modal"><div class="modal-head">新建桶</div><div class="modal-body"><input v-model="newBucketName" class="input" placeholder="桶名（3–63 位小写字母、数字、点或连字符）" @keyup.enter="createBucket" /></div><div class="modal-foot"><button class="btn" @click="showBucketDialog = false">取消</button><button class="btn primary" @click="createBucket">创建</button></div></div></div>
    <div v-if="showBucketManage" class="modal-mask"><div class="modal small-modal"><div class="modal-head">桶管理：{{ managedBucket }}</div><div class="modal-body copy-form"><p>版本控制：<b>{{ bucketVersionStatus }}</b></p><p class="copy-hint">删除桶前必须清空桶内对象。</p></div><div class="modal-foot"><button class="btn" @click="showBucketManage = false">关闭</button><button class="btn" :disabled="bucketVersionStatus === '加载中…'" @click="toggleBucketVersioning">{{ bucketVersionStatus === 'Enabled' ? '暂停版本控制' : '开启版本控制' }}</button><button class="btn danger" @click="removeBucket">删除桶</button></div></div></div>

    <div v-if="showPresign" class="modal-mask">
      <div class="modal presign-modal">
        <div class="modal-head">生成{{ presignMode === "get" ? "下载" : "上传" }}链接</div>
        <div class="modal-body copy-form">
          <p v-if="presignMode === 'get'">将为已选文件生成临时下载链接。</p>
          <label v-else class="copy-row"><span class="copy-label">对象名称</span><input v-model="presignKey" class="input" placeholder="例如 incoming/file.zip" /></label>
          <label class="copy-row"><span class="copy-label">有效期（秒）</span><input v-model.number="presignSeconds" class="input" type="number" min="1" max="604800" /></label>
          <textarea v-if="presignUrls.length" class="input presign-result" readonly :value="presignUrls.join('\n')" />
        </div>
        <div class="modal-foot">
          <button class="btn" @click="showPresign = false">关闭</button>
          <button v-if="presignUrls.length" class="btn" @click="copyPresignUrls">复制</button>
          <button v-if="presignUrls.length && presignMode === 'get'" class="btn" @click="exportPresignUrls">导出清单</button>
          <button class="btn primary" @click="generatePresign">生成</button>
        </div>
      </div>
    </div>

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
            <span class="copy-label">目标桶</span>
            <select v-model="copyTargetBucket" class="select"><option v-for="bucket in store.browse.buckets" :key="bucket" :value="bucket">{{ bucket }}</option><option v-if="!store.browse.buckets.includes(store.browse.bucket)" :value="store.browse.bucket">{{ store.browse.bucket }}</option></select>
          </label>
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
.bucket-title { display: flex; align-items: center; justify-content: space-between; }
.bucket-filter { width: 100%; margin: 8px 0; }
.bucket-manage { margin-left: auto; }
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
.presign-modal { width: min(680px, 94vw); }
.presign-result { min-height: 130px; resize: vertical; font-family: ui-monospace, monospace; }
.sync-modal { width: min(720px, 94vw); }
.sync-items { max-height: 180px; overflow: auto; border-top: 1px solid var(--border); padding-top: 6px; font-size: 12px; }
.search-modal { width: min(800px, 94vw); }
.search-results { max-height: 280px; overflow: auto; border-top: 1px solid var(--border); }
.search-row { width: 100%; display: grid; grid-template-columns: 130px 1fr 100px; text-align: left; gap: 8px; padding: 7px 0; border: 0; border-bottom: 1px solid var(--border); background: transparent; color: var(--text); cursor: pointer; }
.search-row:hover { color: var(--primary); }
.search-row span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.transfer-modal { width: min(620px, 94vw); }
.transfer-modal section { display: grid; gap: 8px; padding: 10px 0; border-bottom: 1px solid var(--border); }
.command-modal { width: min(540px, 94vw); }
.command-row { display: flex; justify-content: space-between; width: 100%; padding: 9px; border: 0; background: transparent; color: var(--text); text-align: left; cursor: pointer; border-radius: 5px; }
.command-row:hover { background: var(--hover); color: var(--primary); }
kbd { color: var(--muted); font-size: 11px; }
.update-toast { position: fixed; right: 16px; top: 52px; z-index: 250; max-width: 360px; padding: 10px 14px; background: var(--panel); border: 1px solid var(--border); border-radius: 8px; box-shadow: 0 6px 20px var(--shadow); cursor: pointer; }
.audit-modal { width: min(900px, 94vw); }
.audit-list { max-height: 360px; overflow: auto; }
.audit-row { display: grid; grid-template-columns: 160px 160px 130px 1fr; gap: 8px; padding: 6px 0; border-bottom: 1px solid var(--border); font-size: 12px; }
.audit-row span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.code-export { min-height: 180px; font-family: ui-monospace, monospace; }
.policy-modal { width: min(860px, 94vw); }
.policy-editor { min-height: 360px; font-family: ui-monospace, monospace; }
.policy-diff { max-height: 180px; overflow: auto; white-space: pre-wrap; }
.policy-diff .added { color: var(--ok); }.policy-diff .removed { color: var(--danger); }
.object-lock { padding: 8px; border: 1px solid var(--border); border-radius: 6px; font-size: 12px; color: var(--muted); }
.multipart-row { display: grid; grid-template-columns: 22px 1fr 180px 48px; gap: 8px; padding: 7px 0; border-bottom: 1px solid var(--border); }
.break { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.parts-panel { margin-top: 12px; border-top: 1px solid var(--border); padding-top: 10px; max-height: 180px; overflow: auto; }
</style>
