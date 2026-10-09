<script setup lang="ts">
import { ref } from "vue";
import type { ConflictPolicy } from "../types";
import { useMaskClose } from "./useMaskClose";

defineProps<{ modelValue: boolean; count: number; dest?: string }>();

const emit = defineEmits<{
  "update:modelValue": [v: boolean];
  choose: [policy: ConflictPolicy];
  "change-dest": [];
}>();

const { onMousedown, onClick } = useMaskClose(() =>
  emit("update:modelValue", false),
);

// 默认选安全的“保留两者”（规格：三选一；不指定默认值，此为推荐项）
const policy = ref<ConflictPolicy>("rename");

function setPolicy(p: ConflictPolicy) {
  policy.value = p;
}
</script>

<template>
  <div v-if="modelValue" class="modal-mask" @mousedown="onMousedown" @click="onClick">
    <div class="modal">
      <div class="modal-head">同名文件处理方式</div>
      <div class="modal-body">
        <p class="muted">
          即将下载 <b>{{ count }}</b> 个条目。若本地已存在同名文件：
        </p>
        <p v-if="dest" class="muted" :title="dest">保存到：{{ dest }}</p>
        <label class="radio-row">
          <input
            type="radio"
            :checked="policy === 'overwrite'"
            @change="setPolicy('overwrite')"
          />
          <span><b>覆盖</b> — 替换本地文件</span>
        </label>
        <label class="radio-row">
          <input
            type="radio"
            :checked="policy === 'rename'"
            @change="setPolicy('rename')"
          />
          <span><b>保留两者</b> — 新文件自动重命名为 name (1).ext</span>
        </label>
        <label class="radio-row">
          <input
            type="radio"
            :checked="policy === 'skip'"
            @change="setPolicy('skip')"
          />
          <span><b>跳过</b> — 不下载同名文件</span>
        </label>
      </div>
      <div class="modal-foot">
        <button class="btn" @click="emit('change-dest')">更换目录</button>
        <button class="btn" @click="emit('update:modelValue', false)">取消</button>
        <button class="btn primary" @click="emit('choose', policy)">
          开始下载
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
