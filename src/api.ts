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
  ObjectDetail,
  MultipartUploadInfo,
  MultipartPartInfo,
  TransferSettings,
  ObjectVersionInfo,
  ObjectTag,
  ObjectAclInfo,
  BucketVersioningInfo,
  ResumableTaskInfo,
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
export const createBucket = (bucket: string) => invoke<void>("create_bucket", { bucket });
export const deleteBucket = (bucket: string) => invoke<void>("delete_bucket", { bucket });
export const bucketVersioning = (bucket: string) => invoke<BucketVersioningInfo>("bucket_versioning", { bucket });
export const setBucketVersioning = (bucket: string, enabled: boolean) => invoke<void>("set_bucket_versioning", { bucket, enabled });
export const listObjects = (bucket: string, prefix: string, token: string | null) =>
  invoke<ListResult>("list_objects", { bucket, prefix, token });
export const countObjects = (bucket: string, prefix: string) =>
  invoke<number>("count_objects", { bucket, prefix });
export const previewObject = (bucket: string, key: string) =>
  invoke<PreviewData>("preview_object", { bucket, key });
export const isDownloadDirectory = (dest: string) =>
  invoke<boolean>("is_download_directory", { dest });
export const presignGet = (bucket: string, keys: string[], seconds: number) =>
  invoke<string[]>("presign_get", { bucket, keys, seconds });
export const presignPut = (bucket: string, key: string, seconds: number) =>
  invoke<string>("presign_put", { bucket, key, seconds });
export const exportPresignList = (path: string, urls: string[]) =>
  invoke<void>("export_presign_list", { path, urls });
export const objectDetail = (bucket: string, key: string) =>
  invoke<ObjectDetail>("object_detail", { bucket, key });
export const objectVersions = (bucket: string, key: string) => invoke<ObjectVersionInfo[]>("object_versions", { bucket, key });
export const restoreObjectVersion = (bucket: string, key: string, version: string) => invoke<void>("restore_object_version", { bucket, key, version });
export const objectTags = (bucket: string, key: string) => invoke<ObjectTag[]>("object_tags", { bucket, key });
export const saveObjectTags = (bucket: string, key: string, tags: ObjectTag[]) => invoke<void>("save_object_tags", { bucket, key, tags });
export const objectAcl = (bucket: string, key: string) => invoke<ObjectAclInfo>("object_acl", { bucket, key });
export const createFolder = (bucket: string, prefix: string, name: string) =>
  invoke<void>("create_folder", { bucket, prefix, name });
export const startCopy = (
  bucket: string,
  items: FsItem[],
  targetPrefix: string,
  conflict: string,
  newName: string | null,
  remove: boolean,
) =>
  invoke<string>("start_copy", {
    bucket,
    items,
    target_prefix: targetPrefix,
    conflict,
    new_name: newName,
    remove,
  });
export const openObject = (bucket: string, key: string) =>
  invoke<string>("open_object", { bucket, key });
export const listMultipartUploads = (bucket: string) =>
  invoke<MultipartUploadInfo[]>("list_multipart_uploads", { bucket });
export const abortMultipartUploads = (bucket: string, uploads: MultipartUploadInfo[]) =>
  invoke<MultipartUploadInfo[]>("abort_multipart_uploads", { bucket, uploads });
export const listMultipartParts = (bucket: string, key: string, uploadId: string) =>
  invoke<MultipartPartInfo[]>("list_multipart_parts", { bucket, key, upload_id: uploadId });

export const planUpload = (paths: string[], prefix: string) =>
  invoke<UploadPlan>("plan_upload", { paths, prefix });
export const startUpload = (bucket: string, prefix: string, paths: string[], storage: string | null) =>
  invoke<string>("start_upload", { bucket, prefix, paths, storage });
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
export const getResumableTasks = () => invoke<ResumableTaskInfo[]>("get_resumable_tasks");
export const discardResumableTask = (id: string) => invoke<void>("discard_resumable_task", { id });
export const resumeTask = (id: string) => invoke<void>("resume_task", { id });
export const clearFinishedTasks = () => invoke<void>("clear_finished_tasks");

export const getSettings = () => invoke<TransferSettings>("get_settings");
export const saveTransferSettings = (settings: TransferSettings) =>
  invoke<void>("save_transfer_settings", { settings });
