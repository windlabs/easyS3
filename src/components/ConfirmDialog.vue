<script setup lang="ts">
import { useMaskClose, useDialogFocus } from "./useMaskClose";

const props = defineProps<{
  modelValue: boolean;
  title: string;
  okText?: string;
  danger?: boolean;
  busy?: boolean;
}>();

const emit = defineEmits<{
  "update:modelValue": [v: boolean];
  confirm: [];
}>();

const { onMousedown, onClick } = useMaskClose(() =>
  { if (!props.busy) emit("update:modelValue", false); },
);
const { root, onKeydown } = useDialogFocus(() => props.modelValue, () => { if (!props.busy) emit("update:modelValue", false); });
void root;
</script>

<template>
  <div v-if="modelValue" class="modal-mask" @mousedown="onMousedown" @click="onClick">
    <div ref="root" class="modal" tabindex="-1" @keydown="onKeydown">
      <div class="modal-head">{{ title }}</div>
      <div class="modal-body"><slot /></div>
      <div class="modal-foot">
        <button class="btn" :disabled="busy" @click="emit('update:modelValue', false)">取消</button>
        <button
          class="btn"
          :class="danger ? 'danger' : 'primary'"
          :disabled="busy"
          @click="emit('confirm')"
        >
          {{ busy ? "处理中…" : (okText ?? "确定") }}
        </button>
      </div>
    </div>
  </div>
</template>
