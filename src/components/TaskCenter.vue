<script setup lang="ts">
import { computed } from "vue";
import { cancelTask, clearFinished, retryTask, tasksStore } from "../store";
import type { TaskInfo } from "../types";
import { formatBytes } from "../utils";

const kindLabel: Record<TaskInfo["kind"], string> = {
  upload: "上传",
  download: "下载",
  delete: "删除",
};
const statusLabel: Record<TaskInfo["status"], string> = {
  running: "进行中",
  done: "已完成",
  failed: "失败",
  canceled: "已取消",
};

const runningCount = computed(
  () => tasksStore.tasks.filter((t) => t.status === "running").length,
);
const hasFinished = computed(() =>
  tasksStore.tasks.some((t) => t.status !== "running"),
);

function percent(t: TaskInfo): number | null {
  if (t.kind === "delete") {
    return t.files_total > 0
      ? Math.round((t.files_done / t.files_total) * 100)
      : null;
  }
  if (t.bytes_total > 0) {
    return Math.min(100, Math.round((t.bytes_done / t.bytes_total) * 100));
  }
  return null;
}

function progressText(t: TaskInfo): string {
  if (t.kind === "delete") return `${t.files_done} / ${t.files_total} 个`;
  return `${formatBytes(t.bytes_done)} / ${formatBytes(t.bytes_total)}`;
}

function fileStats(t: TaskInfo): string {
  const parts = [`文件 ${t.files_done}/${t.files_total}`];
  if (t.files_skipped > 0) parts.push(`跳过 ${t.files_skipped}`);
  if (t.files_failed > 0) parts.push(`失败 ${t.files_failed}`);
  return parts.join("，");
}
</script>

<template>
  <div v-if="tasksStore.tasks.length" class="task-center">
    <div class="task-head" @click="tasksStore.open = !tasksStore.open">
      <span class="title">传输任务</span>
      <span v-if="runningCount" class="badge">{{ runningCount }}</span>
      <span class="spacer" />
      <button
        v-if="hasFinished && tasksStore.open"
        class="btn sm ghost"
        @click.stop="clearFinished"
      >
        清除已完成
      </button>
      <span class="toggle">{{ tasksStore.open ? "▾" : "▴" }}</span>
    </div>
    <div v-if="tasksStore.open" class="task-list">
      <div
        v-for="t in tasksStore.tasks"
        :key="t.id"
        class="task-card"
        :data-status="t.status"
      >
        <div class="task-line">
          <span class="kind">{{ kindLabel[t.kind] }}</span>
          <span class="bucket muted" :title="t.bucket">{{ t.bucket }}</span>
          <span class="status" :class="t.status">{{ statusLabel[t.status] }}</span>
          <span class="spacer" />
          <button
            v-if="t.status === 'running'"
            class="btn sm"
            @click="cancelTask(t.id)"
          >
            取消
          </button>
          <button
            v-if="t.status !== 'running' && t.failures.length"
            class="btn sm"
            @click="retryTask(t.id)"
          >
            重试失败项
          </button>
        </div>
        <div class="task-line stats muted">
          <span>{{ fileStats(t) }}</span>
          <span>· {{ progressText(t) }}</span>
        </div>
        <div v-if="t.status === 'running'" class="progress">
          <div
            v-if="percent(t) !== null"
            class="bar"
            :style="{ width: (percent(t) ?? 0) + '%' }"
          />
          <div v-else class="bar indeterminate" />
        </div>
        <div
          v-if="t.current_file && t.status === 'running'"
          class="task-line current muted"
          :title="t.current_file"
        >
          {{ t.current_file }}
        </div>
        <div v-if="t.failures.length" class="failures">
          <div
            v-for="f in t.failures.slice(0, 3)"
            :key="f.key"
            class="task-line failure"
          >
            <span class="fkey" :title="f.key">{{ f.key }}</span>
            <span class="ferr" :title="f.error">{{ f.error }}</span>
          </div>
          <div v-if="t.failures.length > 3" class="muted more">
            …等 {{ t.failures.length }} 个失败项
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.task-center {
  position: fixed;
  right: 16px;
  bottom: 16px;
  width: 400px;
  max-width: calc(100vw - 32px);
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 10px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.14);
  z-index: 200;
  overflow: hidden;
}
.task-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  cursor: pointer;
  background: #fafbfc;
  border-bottom: 1px solid var(--border);
}
.task-head .title {
  font-weight: 600;
}
.badge {
  background: var(--primary);
  color: #fff;
  border-radius: 10px;
  padding: 0 7px;
  font-size: 12px;
  line-height: 18px;
}
.task-list {
  max-height: 42vh;
  overflow: auto;
}
.task-card {
  padding: 10px 14px;
  border-bottom: 1px solid var(--border);
}
.task-card:last-child {
  border-bottom: none;
}
.task-line {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.task-line .spacer {
  flex: 1;
}
.kind {
  font-weight: 600;
  flex-shrink: 0;
}
.bucket {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.status {
  flex-shrink: 0;
  font-size: 12px;
  border-radius: 4px;
  padding: 0 6px;
  line-height: 18px;
}
.status.running {
  color: var(--primary);
  background: rgba(51, 112, 255, 0.1);
}
.status.done {
  color: var(--ok);
  background: rgba(52, 199, 36, 0.12);
}
.status.failed {
  color: var(--danger);
  background: rgba(229, 69, 69, 0.1);
}
.status.canceled {
  color: var(--muted);
  background: rgba(0, 0, 0, 0.05);
}
.stats {
  margin-top: 4px;
  gap: 4px;
}
.current {
  margin-top: 4px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}
.progress {
  margin-top: 6px;
  height: 4px;
  border-radius: 2px;
  background: #edf0f4;
  overflow: hidden;
}
.progress .bar {
  height: 100%;
  background: var(--primary);
  border-radius: 2px;
  transition: width 0.2s ease;
}
.progress .bar.indeterminate {
  width: 30%;
  animation: slide 1.2s infinite ease-in-out;
}
@keyframes slide {
  0% {
    margin-left: -30%;
  }
  100% {
    margin-left: 100%;
  }
}
.failures {
  margin-top: 6px;
  background: #fdf5f5;
  border-radius: 6px;
  padding: 6px 8px;
}
.failure {
  font-size: 12px;
  color: var(--danger);
  gap: 6px;
}
.fkey {
  max-width: 45%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ferr {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.more {
  font-size: 12px;
  margin-top: 2px;
}
</style>
