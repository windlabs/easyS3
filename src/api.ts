import { invoke } from "@tauri-apps/api/core";
import type {
  ConflictPolicy,
  FsItem,
  ListResult,
  PreviewData,
  ConnectionConfig,
  StateDto,
  TaskInfo,
  UploadPlan,
} from "./types";

// 命令参数一律使用与 Rust 参数名完全一致的键（均为单词，规避大小写转换问题）

export const getState = () => invoke<StateDto>("get_state");
export const saveConnection = (connection: ConnectionConfig) =>
  invoke<void>("save_connection", { connection });
export const deleteConnection = (id: string) => invoke<void>("delete_connection", { id });
export const setCurrentConnection = (id: string) =>
  invoke<void>("set_current_connection", { id });
export const testConnection = (connection: ConnectionConfig) =>
  invoke<void>("test_connection", { connection });

export const listBuckets = () => invoke<string[]>("list_buckets");
export const listObjects = (bucket: string, prefix: string, token: string | null) =>
  invoke<ListResult>("list_objects", { bucket, prefix, token });
export const countObjects = (bucket: string, prefix: string) =>
  invoke<number>("count_objects", { bucket, prefix });
export const previewObject = (bucket: string, key: string) =>
  invoke<PreviewData>("preview_object", { bucket, key });

export const planUpload = (paths: string[], prefix: string) =>
  invoke<UploadPlan>("plan_upload", { paths, prefix });
export const startUpload = (bucket: string, prefix: string, paths: string[]) =>
  invoke<string>("start_upload", { bucket, prefix, paths });
export const startDownload = (
  bucket: string,
  items: FsItem[],
  dest: string,
  conflict: ConflictPolicy,
) => invoke<string>("start_download", { bucket, items, dest, conflict });
export const startDelete = (bucket: string, items: FsItem[]) =>
  invoke<string>("start_delete", { bucket, items });

export const cancelTask = (id: string) => invoke<void>("cancel_task", { id });
export const retryTask = (id: string) => invoke<void>("retry_task", { id });
export const getTasks = () => invoke<TaskInfo[]>("get_tasks");
export const clearFinishedTasks = () => invoke<void>("clear_finished_tasks");
