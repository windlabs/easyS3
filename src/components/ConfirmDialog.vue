<script setup lang="ts">
import { useMaskClose } from "./useMaskClose";

defineProps<{
  modelValue: boolean;
  title: string;
  okText?: string;
  danger?: boolean;
}>();

const emit = defineEmits<{
  "update:modelValue": [v: boolean];
  confirm: [];
}>();

const { onMousedown, onClick } = useMaskClose(() =>
  emit("update:modelValue", false),
);
</script>

<template>
  <div v-if="modelValue" class="modal-mask" @mousedown="onMousedown" @click="onClick">
    <div class="modal">
      <div class="modal-head">{{ title }}</div>
      <div class="modal-body"><slot /></div>
      <div class="modal-foot">
        <button class="btn" @click="emit('update:modelValue', false)">取消</button>
        <button
          class="btn"
          :class="danger ? 'danger' : 'primary'"
          @click="emit('confirm')"
        >
          {{ okText ?? "确定" }}
        </button>
      </div>
    </div>
  </div>
</template>
