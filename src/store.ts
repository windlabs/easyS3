import { reactive } from "vue";
import { listen } from "@tauri-apps/api/event";
import * as api from "./api";
import type { Entry, ProjectConfig, TaskInfo } from "./types";
import { errorMessage } from "./utils";

// ---------- 全局应用状态 ----------

export const app = reactive({
  projects: [] as ProjectConfig[],
  currentProjectId: null as string | null,
  loaded: false,
});

export function currentProject(): ProjectConfig | null {
  return app.projects.find((p) => p.id === app.currentProjectId) ?? null;
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
});

// ---------- 轻提示 ----------

export const toasts = reactive({
  list: [] as { id: number; text: string; kind: "info" | "error" }[],
});
let toastSeq = 0;
export function toast(text: string, kind: "info" | "error" = "info") {
  const id = ++toastSeq;
  toasts.list.push({ id, text, kind });
  setTimeout(() => {
    const i = toasts.list.findIndex((t) => t.id === id);
    if (i >= 0) toasts.list.splice(i, 1);
  }, 4000);
}

// ---------- 动作 ----------

export async function refreshState() {
  const s = await api.getState();
  app.projects = s.projects;
  app.currentProjectId = s.current_project_id;
  app.loaded = true;
}

export async function switchProject(id: string) {
  await api.setCurrentProject(id);
  await refreshState();
  await enterProject();
}

export async function enterProject() {
  const p = currentProject();
  selection.clear();
  browse.filter = "";
  if (!p) {
    browse.mode = "empty";
    return;
  }
  if (p.default_bucket) {
    // 限定单桶项目：跳过桶列表直接进入该桶（规格）
    browse.bucket = p.default_bucket;
    browse.prefix = "";
    await loadFirstPage();
  } else {
    await loadBuckets();
  }
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
  await listen<TaskInfo>("task-update", (ev) => {
    const t = ev.payload;
    const i = tasksStore.tasks.findIndex((x) => x.id === t.id);
    if (i >= 0) tasksStore.tasks[i] = t;
    else tasksStore.tasks.push(t);
    // 上传/删除任务结束后刷新当前列表（下载不影响服务端列表）
    if (
      t.status !== "running" &&
      (t.kind === "upload" || t.kind === "delete")
    ) {
      void refresh();
    }
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
  await api.clearFinishedTasks();
  tasksStore.tasks = tasksStore.tasks.filter((t) => t.status === "running");
}
