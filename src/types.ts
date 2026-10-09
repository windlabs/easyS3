/** 与后端 DTO 对应的类型定义（字段均为 snake_case，与 Rust serde 一致） */

import { uuid } from "./utils";

export interface ConnectionConfig {
  id: string;
  name: string;
  endpoint_url: string;
  region: string;
  access_key: string;
  secret_key: string;
  force_path_style: boolean;
  default_bucket?: string | null;
  /** 可选；服务用自签证书时指定 CA 证书（PEM 编码）文件路径 */
  ca_cert_path?: string | null;
  /** 最近一次「测试连接」结果（后端随配置持久化；连接字段变更后由后端清空） */
  last_test?: LastTest | null;
}

/** 最近一次「测试连接」结果 */
export interface LastTest {
  ok: boolean;
  msg: string;
  /** Unix 时间戳（秒） */
  at: number;
}

export interface StateDto {
  connections: ConnectionConfig[];
  current_connection_id: string | null;
}

export interface Entry {
  /** 完整 key（目录为前缀，以 / 结尾） */
  key: string;
  name: string;
  size: number;
  last_modified: string | null;
  storage_class: string | null;
  is_dir: boolean;
}

export interface ObjectDetail {
  key: string;
  size: number;
  last_modified: string | null;
  e_tag: string | null;
  storage_class: string | null;
  content_type: string | null;
  metadata: { key: string; value: string }[];
}

export interface MultipartUploadInfo {
  key: string;
  upload_id: string;
  initiated: string | null;
  storage_class: string | null;
}

export interface MultipartPartInfo {
  part_number: number;
  size: number;
  last_modified: string | null;
  e_tag: string | null;
}

export interface ListResult {
  entries: Entry[];
  next_token: string | null;
}

export interface PlannedFile {
  path: string;
  key: string;
  size: number;
}

export interface UploadPlan {
  files: PlannedFile[];
  total_bytes: number;
}

export interface FsItem {
  key: string;
  is_dir: boolean;
}

export type ConflictPolicy = "overwrite" | "rename" | "skip";

export interface Failure {
  key: string;
  error: string;
}

export type TaskKind = "upload" | "download" | "delete" | "copy";
export type TaskStatus = "queued" | "running" | "done" | "failed" | "canceled";

export interface TaskInfo {
  id: string;
  kind: TaskKind;
  bucket: string;
  status: TaskStatus;
  files_total: number;
  files_done: number;
  files_failed: number;
  files_skipped: number;
  bytes_total: number;
  bytes_done: number;
  current_file: string | null;
  failures: Failure[];
  /** 瞬时错误自动重试累计次数 */
  retry_count: number;
  created_at: number;
}

export interface TransferSettings {
  auto_retry_count: number;
  upload_limit_kbps: number;
  download_limit_kbps: number;
  file_concurrency: number;
  part_size_mb: number;
  max_concurrent_tasks: number;
  preview_limit_mb: number;
}

export type PreviewData =
  | { kind: "text"; content: string }
  | { kind: "image"; mime: string; data_base64: string }
  | { kind: "media"; mime: string; data_base64: string };

export function blankConnection(): ConnectionConfig {
  return {
    id: uuid(),
    name: "",
    endpoint_url: "",
    region: "us-east-1",
    access_key: "",
    secret_key: "",
    force_path_style: true,
    default_bucket: null,
    ca_cert_path: null,
    last_test: null,
  };
}
