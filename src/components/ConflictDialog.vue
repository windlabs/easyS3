<script setup lang="ts">
import { ref, watch } from "vue";
import type { ConflictPolicy } from "../types";
import { useMaskClose, useDialogFocus } from "./useMaskClose";

const props = defineProps<{ modelValue: boolean; count: number; dest?: string; busy?: boolean }>();

const emit = defineEmits<{
  "update:modelValue": [v: boolean];
  choose: [policy: ConflictPolicy];
  "change-dest": [];
}>();

const { onMousedown, onClick } = useMaskClose(() =>
  emit("update:modelValue", false),
);
const { root, onKeydown } = useDialogFocus(() => props.modelValue, () => emit("update:modelValue", false));
watch(() => props.modelValue, (open) => { if (open) requestAnimationFrame(() => root.value?.querySelector<HTMLElement>("input:not(:disabled), button:not(:disabled)")?.focus()); });

// 默认选安全的“保留两者”（规格：三选一；不指定默认值，此为推荐项）
const policy = ref<ConflictPolicy>("rename");

watch(() => props.modelValue, (open) => {
  if (open) policy.value = "rename";
});

function setPolicy(p: ConflictPolicy) {
  policy.value = p;
}
</script>

<template>
  <div v-if="modelValue" class="modal-mask" @mousedown="onMousedown" @click="onClick">
    <div ref="root" class="modal" @keydown="onKeydown">
      <div class="modal-head">同名文件处理方式</div>
      <div class="modal-body">
        <p class="muted">
          即将下载 <b>{{ count }}</b> 个条目。若本地已存在同名文件：
        </p>
        <p v-if="dest" class="muted" :title="dest">保存到：{{ dest }}</p>
        <label class="radio-row">
          <input
            :disabled="busy"
            type="radio"
            :checked="policy === 'overwrite'"
            @change="setPolicy('overwrite')"
          />
          <span><b>覆盖</b> — 替换本地文件</span>
        </label>
        <label class="radio-row">
          <input
            :disabled="busy"
            type="radio"
            :checked="policy === 'rename'"
            @change="setPolicy('rename')"
          />
          <span><b>保留两者</b> — 新文件自动重命名为 name (1).ext</span>
        </label>
        <label class="radio-row">
          <input
            :disabled="busy"
            type="radio"
            :checked="policy === 'skip'"
            @change="setPolicy('skip')"
          />
          <span><b>跳过</b> — 不下载同名文件</span>
        </label>
      </div>
      <div class="modal-foot">
        <button class="btn" :disabled="busy" @click="emit('change-dest')">更换目录</button>
        <button class="btn" :disabled="busy" @click="emit('update:modelValue', false)">取消</button>
        <button class="btn primary" :disabled="busy" @click="emit('choose', policy)">
          {{ busy ? "处理中…" : "开始下载" }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.radio-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  margin-bottom: 8px;
  cursor: pointer;
}
.radio-row:hover {
  border-color: var(--primary);
}
</style>
