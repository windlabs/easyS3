import { reactive } from "vue";
import { listen } from "@tauri-apps/api/event";
import * as api from "./api";
import type { Entry, ConnectionConfig, TaskInfo } from "./types";
import { errorMessage } from "./utils";

// ---------- 全局应用状态 ----------

export const app = reactive({
  connections: [] as ConnectionConfig[],
  currentConnectionId: null as string | null,
  loaded: false,
});

export function currentConnection(): ConnectionConfig | null {
  return app.connections.find((p) => p.id === app.currentConnectionId) ?? null;
}

// ---------- 浏览状态 ----------

export type BrowseMode = "empty" | "buckets" | "objects";

export const browse = reactive({
  mode: "empty" as BrowseMode,
  buckets: [] as string[],
  bucket: "",
  prefix: "",
  filter: "",
  entries: [] as Entry[],
  nextToken: null as string | null,
  loading: false,
  error: "",
});

export const selection = reactive(new Set<string>());

// ---------- 传输任务 ----------

export const tasksStore = reactive({
  tasks: [] as TaskInfo[],
  open: false,
  hovered: false,
});

// 全部任务成功结束后，任务中心自动收起的延时（毫秒）
const TASKS_AUTO_CLOSE_MS = 5000;
let tasksAutoCloseTimer: ReturnType<typeof setTimeout> | null = null;

function clearTasksAutoClose() {
  if (tasksAutoCloseTimer !== null) {
    clearTimeout(tasksAutoCloseTimer);
    tasksAutoCloseTimer = null;
  }
}

/**
 * 自动收起判定：仅当「存在任务、全部终态、且没有任何失败项、鼠标未悬停」时启动计时。
 * 有任务进行中或有失败项时不关闭（失败任务需保留重试入口）。
 */
function maybeScheduleTasksAutoClose() {
  clearTasksAutoClose();
  const tasks = tasksStore.tasks;
  if (!tasks.length) return;
  if (tasks.some((t) => t.status === "running" || t.status === "queued")) return;
  if (tasks.some((t) => t.failures.length > 0)) return;
  if (tasksStore.hovered) return;
  tasksAutoCloseTimer = setTimeout(() => {
    tasksAutoCloseTimer = null;
    void clearFinished();
  }, TASKS_AUTO_CLOSE_MS);
}

/** 鼠标悬停任务中心时暂停自动收起，移出后重新计时。 */
export function setTasksHover(hovered: boolean) {
  tasksStore.hovered = hovered;
  if (hovered) clearTasksAutoClose();
  else maybeScheduleTasksAutoClose();
}

// ---------- 轻提示 ----------

export const toasts = reactive({
  list: [] as { id: number; text: string; kind: "info" | "error" }[],
});
let toastSeq = 0;
export function toast(text: string, kind: "info" | "error" = "info") {
  const id = ++toastSeq;
  toasts.list.push({ id, text, kind });
  // 错误提示停留更久（规格），便于阅读与排查
  setTimeout(
    () => {
      const i = toasts.list.findIndex((t) => t.id === id);
      if (i >= 0) toasts.list.splice(i, 1);
    },
    kind === "error" ? 8000 : 4000,
  );
}

// ---------- 动作 ----------

export async function refreshState() {
  const s = await api.getState();
  app.connections = s.connections;
  app.currentConnectionId = s.current_connection_id;
  app.loaded = true;
}

export async function switchConnection(id: string) {
  await api.setCurrentConnection(id);
  await refreshState();
  await enterConnection();
}

export async function enterConnection() {
  const p = currentConnection();
  selection.clear();
  browse.filter = "";
  if (!p) {
    browse.mode = "empty";
    return;
  }
  if (p.default_bucket) {
    // 限定单桶连接：跳过桶列表直接进入该桶（规格）
    browse.bucket = p.default_bucket;
    browse.prefix = "";
    await loadFirstPage();
  } else {
    await loadBuckets();
  }
}

export async function backToBuckets() {
  const p = currentConnection();
  // 限定单桶连接无桶列表，不提供返回入口（规格）
  if (!p || p.default_bucket) return;
  selection.clear();
  browse.filter = "";
  await loadBuckets();
}

async function loadBuckets() {
  browse.mode = "buckets";
  browse.loading = true;
  browse.error = "";
  try {
    browse.buckets = await api.listBuckets();
  } catch (e) {
    browse.error = errorMessage(e);
  } finally {
    browse.loading = false;
  }
}

export async function selectBucket(bucket: string) {
  browse.bucket = bucket;
  browse.prefix = "";
  await loadFirstPage();
}

export async function openEntry(entry: Entry) {
  if (!entry.is_dir) return;
  browse.prefix = entry.key;
  browse.filter = "";
  await loadFirstPage();
}

export async function navigatePrefix(prefix: string) {
  browse.prefix = prefix;
  browse.filter = "";
  await loadFirstPage();
}

async function loadFirstPage() {
  browse.mode = "objects";
  browse.entries = [];
  browse.nextToken = null;
  selection.clear();
  await loadMore();
}

export async function loadMore() {
  if (browse.loading) return;
  browse.loading = true;
  browse.error = "";
  try {
    // 前缀过滤：直接以服务端 prefix 参数过滤（规格：非子串搜索）
    const prefix = browse.prefix + browse.filter;
    const r = await api.listObjects(browse.bucket, prefix, browse.nextToken);
    browse.entries.push(...r.entries);
    browse.nextToken = r.next_token;
  } catch (e) {
    browse.error = errorMessage(e);
  } finally {
    browse.loading = false;
  }
}

export async function setFilter(filter: string) {
  browse.filter = filter;
  await loadFirstPage();
}

export async function refresh() {
  if (browse.mode === "objects") await loadFirstPage();
  else if (browse.mode === "buckets") await loadBuckets();
}

// ---------- 任务事件 ----------

export async function initTasks() {
  tasksStore.tasks = await api.getTasks();
  maybeScheduleTasksAutoClose();
  await listen<TaskInfo>("task-update", (ev) => {
    const t = ev.payload;
    const i = tasksStore.tasks.findIndex((x) => x.id === t.id);
    const isNew = i < 0;
    if (i >= 0) tasksStore.tasks[i] = t;
    else tasksStore.tasks.push(t);
    // 新任务开始（含排队中）：取消自动收起计时并展开浮窗
    if (isNew && (t.status === "running" || t.status === "queued")) {
      clearTasksAutoClose();
      tasksStore.open = true;
    }
    // 上传/删除任务结束后刷新当前列表（下载不影响服务端列表）
    if (
      t.status !== "running" &&
      t.status !== "queued" &&
      (t.kind === "upload" || t.kind === "delete")
    ) {
      void refresh();
    }
    maybeScheduleTasksAutoClose();
  });
}

export async function cancelTask(id: string) {
  try {
    await api.cancelTask(id);
  } catch (e) {
    toast(errorMessage(e), "error");
  }
}

export async function retryTask(id: string) {
  try {
    await api.retryTask(id);
  } catch (e) {
    toast(errorMessage(e), "error");
  }
}

export async function clearFinished() {
  clearTasksAutoClose();
  await api.clearFinishedTasks();
  tasksStore.tasks = tasksStore.tasks.filter(
    (t) => t.status === "running" || t.status === "queued",
  );
}
