/** 与后端 DTO 对应的类型定义（字段均为 snake_case，与 Rust serde 一致） */

import { uuid } from "./utils";

export interface ProjectConfig {
  id: string;
  name: string;
  endpoint_url: string;
  region: string;
  access_key: string;
  secret_key: string;
  force_path_style: boolean;
  default_bucket?: string | null;
}

export interface StateDto {
  projects: ProjectConfig[];
  current_project_id: string | null;
}

export interface Entry {
  /** 完整 key（目录为前缀，以 / 结尾） */
  key: string;
  name: string;
  size: number;
  last_modified: string | null;
  is_dir: boolean;
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

export type TaskKind = "upload" | "download" | "delete";
export type TaskStatus = "running" | "done" | "failed" | "canceled";

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
  created_at: number;
}

export type PreviewData =
  | { kind: "text"; content: string }
  | { kind: "image"; mime: string; data_base64: string };

export function blankProject(): ProjectConfig {
  return {
    id: uuid(),
    name: "",
    endpoint_url: "",
    region: "us-east-1",
    access_key: "",
    secret_key: "",
    force_path_style: true,
    default_bucket: null,
  };
}
