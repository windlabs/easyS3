<script setup lang="ts">
import { reactive, ref, watch } from "vue";
import { getSettings, saveTransferSettings } from "../api";
import type { TransferSettings } from "../types";
import { useMaskClose } from "./useMaskClose";

const props = defineProps<{ modelValue: boolean }>();
const emit = defineEmits<{ "update:modelValue": [value: boolean] }>();
const { onMousedown, onClick } = useMaskClose(() => emit("update:modelValue", false));

const form = reactive<TransferSettings>({
  auto_retry_count: 3,
  upload_limit_kbps: 0,
  download_limit_kbps: 0,
  file_concurrency: 2,
  part_size_mb: 8,
  max_concurrent_tasks: 3,
  preview_limit_mb: 20,
});
const saving = ref(false);
const error = ref("");

watch(
  () => props.modelValue,
  async (open) => {
    if (!open) return;
    error.value = "";
    try {
      Object.assign(form, await getSettings());
    } catch (e) {
      error.value = String(e);
    }
  },
);

async function save() {
  // 数字输入框的字符串值统一转数字；非法值交由 Rust 侧 sanitize 收敛
  const settings: TransferSettings = {
    auto_retry_count: Number(form.auto_retry_count),
    upload_limit_kbps: Number(form.upload_limit_kbps),
    download_limit_kbps: Number(form.download_limit_kbps),
    file_concurrency: Number(form.file_concurrency),
    part_size_mb: Number(form.part_size_mb),
    max_concurrent_tasks: Number(form.max_concurrent_tasks),
    preview_limit_mb: Number(form.preview_limit_mb),
  };
  saving.value = true;
  try {
    await saveTransferSettings(settings);
    emit("update:modelValue", false);
  } catch (e) {
    error.value = String(e);
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <div v-if="modelValue" class="modal-mask" @mousedown="onMousedown" @click="onClick">
    <div class="modal settings-modal">
      <div class="modal-head">传输设置</div>
      <div class="modal-body">
        <p v-if="error" class="error-text">{{ error }}</p>

        <label class="form-row">
          <span class="form-label">自动重试次数</span>
          <input v-model.number="form.auto_retry_count" class="input" type="number" min="0" max="10" />
          <span class="form-hint-inline">瞬时错误自动退避重试，0 = 关闭</span>
        </label>

        <label class="form-row">
          <span class="form-label">上传限速</span>
          <input v-model.number="form.upload_limit_kbps" class="input" type="number" min="0" />
          <span class="form-hint-inline">KB/s，0 = 不限速</span>
        </label>

        <label class="form-row">
          <span class="form-label">下载限速</span>
          <input v-model.number="form.download_limit_kbps" class="input" type="number" min="0" />
          <span class="form-hint-inline">KB/s，0 = 不限速</span>
        </label>

        <label class="form-row">
          <span class="form-label">文件并发数</span>
          <input v-model.number="form.file_concurrency" class="input" type="number" min="1" max="8" />
          <span class="form-hint-inline">单任务内同时传输的文件数</span>
        </label>

        <label class="form-row">
          <span class="form-label">分片大小</span>
          <input v-model.number="form.part_size_mb" class="input" type="number" min="5" max="64" />
          <span class="form-hint-inline">MB，大文件分片上传，新任务生效</span>
        </label>

        <label class="form-row">
          <span class="form-label">最大并发任务</span>
          <input v-model.number="form.max_concurrent_tasks" class="input" type="number" min="1" max="16" />
          <span class="form-hint-inline">超出排队等待</span>
        </label>

        <label class="form-row">
          <span class="form-label">预览大小上限</span>
          <input v-model.number="form.preview_limit_mb" class="input" type="number" min="1" max="100" />
          <span class="form-hint-inline">MB，PDF / 音视频预览上限</span>
        </label>

        <p class="form-hint">
          限速调整对进行中任务即时生效；分片大小只影响新任务。设置保存在本机。
        </p>
      </div>
      <div class="modal-foot">
        <button class="btn" @click="emit('update:modelValue', false)">取消</button>
        <button class="btn primary" :disabled="saving" @click="save">保存</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings-modal {
  width: min(520px, 94vw);
}
.form-row {
  display: grid;
  grid-template-columns: 96px 120px 1fr;
  align-items: center;
  gap: 10px;
  margin-bottom: 12px;
}
.form-label {
  color: var(--muted);
}
.form-hint-inline {
  font-size: 12px;
  color: var(--muted);
}
.error-text {
  color: var(--danger);
  margin: 0 0 10px;
}
</style>
