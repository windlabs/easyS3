<script setup lang="ts">
import { toasts } from "../store";
</script>

<template>
  <div class="toast-wrap">
    <div
      v-for="t in toasts.list"
      :key="t.id"
      class="toast"
      :class="t.kind"
    >
      {{ t.text }}
    </div>
  </div>
</template>
