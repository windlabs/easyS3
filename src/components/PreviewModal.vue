<script setup lang="ts">
import { computed } from "vue";
import type { PreviewData } from "../types";
import { useMaskClose } from "./useMaskClose";

const props = defineProps<{
  modelValue: boolean;
  name: string;
  data: PreviewData | null;
}>();

const emit = defineEmits<{ "update:modelValue": [v: boolean] }>();

const { onMousedown, onClick } = useMaskClose(() =>
  emit("update:modelValue", false),
);

const imgSrc = computed(() =>
  props.data?.kind === "image"
    ? `data:${props.data.mime};base64,${props.data.data_base64}`
    : "",
);
</script>

<template>
  <div v-if="modelValue" class="modal-mask" @mousedown="onMousedown" @click="onClick">
    <div class="modal preview-modal">
      <div class="modal-head preview-head">
        <span class="name" :title="name">{{ name }}</span>
        <button class="btn ghost sm" @click="emit('update:modelValue', false)">
          关闭
        </button>
      </div>
      <div class="modal-body preview-body">
        <img v-if="data?.kind === 'image'" :src="imgSrc" :alt="name" />
        <pre v-else-if="data?.kind === 'text'">{{ data.content }}</pre>
      </div>
    </div>
  </div>
</template>

<style scoped>
.preview-modal {
  width: min(860px, 90vw);
}
.preview-head {
  display: flex;
  align-items: center;
  gap: 12px;
}
.preview-head .name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.preview-body {
  display: flex;
  align-items: center;
  justify-content: center;
  background: #f5f6f8;
}
.preview-body img {
  max-width: 100%;
  max-height: 65vh;
  object-fit: contain;
}
.preview-body pre {
  align-self: stretch;
  margin: 0;
  padding: 12px;
  white-space: pre-wrap;
  word-break: break-all;
  font-family: "JetBrains Mono", Consolas, monospace;
  font-size: 12px;
  line-height: 1.6;
  max-height: 65vh;
  overflow: auto;
}
</style>
