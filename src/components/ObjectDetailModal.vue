<script setup lang="ts">
import type { ObjectDetail } from "../types";
import { formatBytes, formatTime } from "../utils";
import { useMaskClose } from "./useMaskClose";

const props = defineProps<{ modelValue: boolean; detail: ObjectDetail | null }>();
const emit = defineEmits<{ "update:modelValue": [value: boolean] }>();
const { onMousedown, onClick } = useMaskClose(() => emit("update:modelValue", false));
</script>

<template>
  <div v-if="modelValue" class="modal-mask" @mousedown="onMousedown" @click="onClick">
    <div class="modal detail-modal">
      <div class="modal-head">对象详情</div>
      <div v-if="detail" class="modal-body">
        <dl>
          <dt>Key</dt><dd class="break">{{ detail.key }}</dd>
          <dt>大小</dt><dd>{{ formatBytes(detail.size) }}</dd>
          <dt>修改时间</dt><dd>{{ formatTime(detail.last_modified) }}</dd>
          <dt>ETag</dt><dd class="break">{{ detail.e_tag || "—" }}</dd>
          <dt>存储类别</dt><dd>{{ detail.storage_class || "STANDARD" }}</dd>
          <dt>Content-Type</dt><dd>{{ detail.content_type || "—" }}</dd>
        </dl>
        <div v-if="detail.metadata.length" class="meta-title">自定义元数据</div>
        <dl v-if="detail.metadata.length">
          <template v-for="item in detail.metadata" :key="item.key">
            <dt>{{ item.key }}</dt><dd class="break">{{ item.value }}</dd>
          </template>
        </dl>
      </div>
      <div class="modal-foot"><button class="btn primary" @click="emit('update:modelValue', false)">关闭</button></div>
    </div>
  </div>
</template>

<style scoped>
.detail-modal { width: min(640px, 92vw); }
dl { display: grid; grid-template-columns: 120px 1fr; margin: 0; gap: 8px 12px; }
dt { color: var(--muted); } dd { margin: 0; } .break { word-break: break-all; }
.meta-title { margin: 18px 0 10px; font-weight: 600; }
</style>
