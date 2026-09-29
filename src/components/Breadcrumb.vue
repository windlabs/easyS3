<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{ bucket: string; prefix: string }>();
const emit = defineEmits<{ navigate: [prefix: string] }>();

const segments = computed(() => {
  const parts = props.prefix.split("/").filter(Boolean);
  const list: { name: string; prefix: string }[] = [
    { name: props.bucket, prefix: "" },
  ];
  let acc = "";
  for (const p of parts) {
    acc += p + "/";
    list.push({ name: p, prefix: acc });
  }
  return list;
});
</script>

<template>
  <nav class="breadcrumb">
    <template v-for="(s, i) in segments" :key="s.prefix">
      <span v-if="i > 0" class="sep">/</span>
      <a
        v-if="i < segments.length - 1"
        href="#"
        @click.prevent="emit('navigate', s.prefix)"
        >{{ s.name }}</a
      >
      <span v-else class="cur">{{ s.name }}</span>
    </template>
  </nav>
</template>

<style scoped>
.breadcrumb {
  display: flex;
  align-items: center;
  gap: 2px;
  min-width: 0;
  overflow: hidden;
}
.breadcrumb a {
  color: var(--primary);
  text-decoration: none;
  padding: 2px 4px;
  border-radius: 4px;
  white-space: nowrap;
}
.breadcrumb a:hover {
  background: rgba(51, 112, 255, 0.08);
}
.breadcrumb .cur {
  font-weight: 600;
  white-space: nowrap;
}
.breadcrumb .sep {
  color: var(--muted);
}
</style>
