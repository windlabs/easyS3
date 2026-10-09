<script setup lang="ts">
import { computed } from "vue";
import {
  cancelTask,
  clearFinished,
  retryTask,
  setTasksHover,
  tasksStore,
} from "../store";
import type { TaskInfo } from "../types";
import { formatBytes } from "../utils";

const kindLabel: Record<TaskInfo["kind"], string> = {
  upload: "上传",
  download: "下载",
  delete: "删除",
  copy: "复制/移动",
};
const statusLabel: Record<TaskInfo["status"], string> = {
  queued: "排队中",
  running: "进行中",
  done: "已完成",
  failed: "失败",
  canceled: "已取消",
};

const runningCount = computed(
  () => tasksStore.tasks.filter((t) => t.status === "running").length,
);
const hasFinished = computed(() =>
  tasksStore.tasks.some((t) => t.status !== "running" && t.status !== "queued"),
);

// 复制/移动与删除按对象计数展示进度（无本地字节流）
const countKind = (t: TaskInfo) => t.kind === "delete" || t.kind === "copy";

function percent(t: TaskInfo): number | null {
  if (countKind(t)) {
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
  if (countKind(t)) return `${t.files_done} / ${t.files_total} 个`;
  return `${formatBytes(t.bytes_done)} / ${formatBytes(t.bytes_total)}`;
}

function fileStats(t: TaskInfo): string {
  const parts = [`文件 ${t.files_done}/${t.files_total}`];
  if (t.files_skipped > 0) parts.push(`跳过 ${t.files_skipped}`);
  if (t.files_failed > 0) parts.push(`失败 ${t.files_failed}`);
  if (t.retry_count > 0) parts.push(`自动重试 ${t.retry_count} 次`);
  return parts.join("，");
}

// 传输速度：按 task-update 事件增量估算（≥500ms 窗口均值），纯前端计算
const speedSamples = new Map<string, { bytes: number; ts: number; speed: number }>();

function sampleSpeed(t: TaskInfo): number | null {
  if (t.status !== "running" || countKind(t) || t.bytes_total <= 0) {
    return null;
  }
  const now = Date.now();
  const prev = speedSamples.get(t.id);
  if (!prev) {
    speedSamples.set(t.id, { bytes: t.bytes_done, ts: now, speed: 0 });
    return null;
  }
  const dt = (now - prev.ts) / 1000;
  if (dt < 0.5) return prev.speed > 0 ? prev.speed : null;
  const speed = Math.max(0, t.bytes_done - prev.bytes) / dt;
  speedSamples.set(t.id, { bytes: t.bytes_done, ts: now, speed });
  return speed;
}

const speeds = computed(() => {
  const m = new Map<string, number | null>();
  for (const t of tasksStore.tasks) m.set(t.id, sampleSpeed(t));
  return m;
});
</script>

<template>
  <div
    v-if="tasksStore.tasks.length"
    class="task-center"
    @mouseenter="setTasksHover(true)"
    @mouseleave="setTasksHover(false)"
  >
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
            v-if="t.status === 'running' || t.status === 'queued'"
            class="btn sm"
            @click="cancelTask(t.id)"
          >
            取消
          </button>
          <button
            v-if="t.status !== 'running' && t.status !== 'queued' && t.failures.length"
            class="btn sm"
            @click="retryTask(t.id)"
          >
            重试失败项
          </button>
        </div>
        <div class="task-line stats muted">
          <span>{{ fileStats(t) }}</span>
          <span>· {{ progressText(t) }}</span>
          <span v-if="speeds.get(t.id) != null" class="speed">
            · {{ formatBytes(speeds.get(t.id) ?? 0) }}/s
          </span>
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
  box-shadow: 0 8px 32px var(--shadow);
  z-index: 200;
  overflow: hidden;
}
.task-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  cursor: pointer;
  background: var(--header-bg);
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
.stats .speed {
  white-space: nowrap;
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
  background: var(--progress-bg);
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
  background: var(--failure-bg);
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
