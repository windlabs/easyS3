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
  SyncPlan,
  UsageStats,
  ResumableTaskInfo,
} from "./types";

// 命令参数一律使用与 Rust 参数名完全一致的键（均为单词，规避大小写转换问题）

export const getState = () => invoke<StateDto>("get_state");
export const testAssumeRole = (roleArn: string, externalId: string | null) => invoke<void>("test_assume_role", { role_arn: roleArn, external_id: externalId });
export const activateAssumeRole = () => invoke<void>("activate_assume_role");
export const getAudit = (connection: string | null, since: number | null) => invoke<import("./types").AuditEntry[]>("get_audit", { connection, since });
export const exportAudit = (path: string, connection: string | null, since: number | null) => invoke<void>("export_audit", { path, connection, since });
export const recordAudit = (bucket: string, action: string, status: string, detail: string) => invoke<void>("record_audit", { bucket, action, status, detail });
export const costOverview = () => invoke<{ date: string; amount: string; unit: string }[]>("cost_overview");
export const generateIac = (bucket: string, format: string) => invoke<string>("generate_iac", { bucket, format });
export const generateSdkSnippet = (bucket: string, key: string, language: string) => invoke<string>("generate_sdk_snippet", { bucket, key, language });
export const openBrowserWindow = () => invoke<void>("open_browser_window");
export const exportConnections = (path: string, secrets: boolean, password: string | null) => invoke<void>("export_connections", { path, secrets, password });
export const importConnections = (path: string, overwrite: boolean, password: string | null) => invoke<number>("import_connections", { path, overwrite, password });
export const importAwsProfiles = () => invoke<ConnectionConfig[]>("import_aws_profiles");
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
export const bucketPolicy = (bucket: string) => invoke<string>("bucket_policy", { bucket });
export const saveBucketPolicy = (bucket: string, policy: string) => invoke<void>("save_bucket_policy", { bucket, policy });
export const bucketCors = (bucket: string) => invoke<import("./types").CorsRule[]>("bucket_cors", { bucket });
export const saveBucketCors = (bucket: string, rules: import("./types").CorsRule[]) => invoke<void>("save_bucket_cors", { bucket, rules });
export const bucketLifecycle = (bucket: string) => invoke<import("./types").LifecycleRule[]>("bucket_lifecycle", { bucket });
export const saveBucketLifecycle = (bucket: string, rules: import("./types").LifecycleRule[]) => invoke<void>("save_bucket_lifecycle", { bucket, rules });
export const bucketWebsite = (bucket: string) => invoke<import("./types").WebsiteConfig>("bucket_website", { bucket });
export const saveBucketWebsite = (bucket: string, website: import("./types").WebsiteConfig) => invoke<void>("save_bucket_website", { bucket, website });
export const objectLock = (bucket: string) => invoke<import("./types").ObjectLockInfo>("object_lock", { bucket });
export const selectCsvObject = (bucket: string, key: string, expression: string) => invoke<import("./types").SelectResult>("select_csv_object", { bucket, key, expression });
export const startSelectCsvObject = (bucket: string, key: string, expression: string) => invoke<string>("start_select_csv_object", { bucket, key, expression });
export const listObjects = (bucket: string, prefix: string, token: string | null) =>
  invoke<ListResult>("list_objects", { bucket, prefix, token });
export const countObjects = (bucket: string, prefix: string) =>
  invoke<number>("count_objects", { bucket, prefix });
export const prefixUsage = (bucket: string, prefix: string) => invoke<UsageStats>("prefix_usage", { bucket, prefix });
export const startPrefixUsage = (bucket: string, prefix: string) => invoke<string>("start_prefix_usage", { bucket, prefix });
export const planSyncUpload = (bucket: string, prefix: string, local: string, propagate: boolean) => invoke<SyncPlan>("plan_sync_upload", { bucket, prefix, local, propagate });
export const planSyncDownload = (bucket: string, prefix: string, local: string, propagate: boolean) => invoke<SyncPlan>("plan_sync_download", { bucket, prefix, local, propagate });
export const startSyncUpload = (bucket: string, plan: SyncPlan) => invoke<string[]>("start_sync_upload", { bucket, plan });
export const startSyncDownload = (bucket: string, plan: SyncPlan) => invoke<string[]>("start_sync_download", { bucket, plan });
export const startZipDownload = (bucket: string, items: FsItem[], dest: string) => invoke<string>("start_zip_download", { bucket, items, dest });
export const startSearch = (bucket: string | null, query: string) => invoke<string>("start_search", { bucket, query });
export const previewObject = (bucket: string, key: string) =>
  invoke<PreviewData>("preview_object", { bucket, key });
export const thumbnailObject = (bucket: string, key: string) => invoke<PreviewData>("thumbnail_object", { bucket, key });
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
export const restoreArchivedObject = (bucket: string, key: string, days: number, tier: string) =>
  invoke<void>("restore_archived_object", { bucket, key, days, tier });
export const objectVersions = (bucket: string, key: string) => invoke<ObjectVersionInfo[]>("object_versions", { bucket, key });
export const restoreObjectVersion = (bucket: string, key: string, version: string) => invoke<void>("restore_object_version", { bucket, key, version });
export const replaceObjectMetadata = (bucket: string, key: string, contentType: string, metadata: ObjectTag[]) => invoke<void>("replace_object_metadata", { bucket, key, content_type: contentType, metadata });
export const saveTextObject = (bucket: string, key: string, content: string) => invoke<void>("save_text_object", { bucket, key, content });
export const objectTags = (bucket: string, key: string) => invoke<ObjectTag[]>("object_tags", { bucket, key });
export const saveObjectTags = (bucket: string, key: string, tags: ObjectTag[]) => invoke<void>("save_object_tags", { bucket, key, tags });
export const objectAcl = (bucket: string, key: string) => invoke<ObjectAclInfo>("object_acl", { bucket, key });
export const createFolder = (bucket: string, prefix: string, name: string) =>
  invoke<void>("create_folder", { bucket, prefix, name });
export const startCopy = (
  bucket: string,
  items: FsItem[],
  targetPrefix: string,
  targetBucket: string,
  conflict: string,
  newName: string | null,
  remove: boolean,
) =>
  invoke<string>("start_copy", {
    bucket,
    items,
    target_prefix: targetPrefix,
    target_bucket: targetBucket,
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
export const pasteClipboardImage = () => invoke<string | null>("paste_clipboard_image");
export const planUploadChanges = (bucket: string, paths: string[], prefix: string) =>
  invoke<UploadPlan>("plan_upload_changes", { bucket, paths, prefix });
export const startUpload = (bucket: string, prefix: string, paths: string[], storage: string | null, verify: boolean, encryption: string, kmsKeyId: string | null) =>
  invoke<string>("start_upload", { bucket, prefix, paths, storage, verify, encryption, kms_key_id: kmsKeyId });
export const startUploadPlan = (bucket: string, plan: UploadPlan, storage: string | null, verify: boolean, encryption: string, kmsKeyId: string | null) =>
  invoke<string>("start_upload_plan", { bucket, plan, storage, verify, encryption, kms_key_id: kmsKeyId });
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
