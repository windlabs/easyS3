<script setup lang="ts">
import { reactive, ref, watch } from "vue";
import * as api from "../api";
import type { ConnectionConfig } from "../types";
import { blankConnection } from "../types";
import { errorMessage } from "../utils";
import { toast } from "../store";
import { useMaskClose } from "./useMaskClose";

const props = defineProps<{
  modelValue: boolean;
  /** null = 新建；否则为编辑目标 */
  connection: ConnectionConfig | null;
}>();

const emit = defineEmits<{
  "update:modelValue": [v: boolean];
  saved: [];
  deleted: [];
}>();

const form = reactive<ConnectionConfig>(blankConnection());
const isEdit = ref(false);
const showSecret = ref(false);
const testing = ref(false);
const testOk = ref(false);
const testMsg = ref("");
const saving = ref(false);
const saveError = ref("");
const confirmDelete = ref(false);

watch(
  () => props.modelValue,
  (open) => {
    if (!open) return;
    Object.assign(form, props.connection ?? blankConnection());
    isEdit.value = !!props.connection;
    showSecret.value = false;
    testing.value = false;
    testMsg.value = "";
    saveError.value = "";
    confirmDelete.value = false;
  },
);

function toPayload(): ConnectionConfig {
  return {
    ...form,
    name: form.name.trim(),
    endpoint_url: form.endpoint_url.trim(),
    region: form.region.trim(),
    access_key: form.access_key.trim(),
    secret_key: form.secret_key.trim(),
    default_bucket: form.default_bucket?.trim() || null,
  };
}

async function testConnection() {
  testing.value = true;
  testMsg.value = "";
  try {
    await api.testConnection(toPayload());
    testOk.value = true;
    testMsg.value = "连接成功";
  } catch (e) {
    testOk.value = false;
    testMsg.value = errorMessage(e);
  } finally {
    testing.value = false;
  }
}

async function save() {
  saving.value = true;
  saveError.value = "";
  try {
    await api.saveConnection(toPayload());
    toast(isEdit.value ? "连接已更新" : "连接已创建");
    emit("saved");
    emit("update:modelValue", false);
  } catch (e) {
    saveError.value = errorMessage(e);
  } finally {
    saving.value = false;
  }
}

async function removeConnection() {
  // 两段式确认，避免误删
  if (!confirmDelete.value) {
    confirmDelete.value = true;
    setTimeout(() => (confirmDelete.value = false), 3000);
    return;
  }
  try {
    await api.deleteConnection(form.id);
    toast("连接已删除");
    emit("deleted");
    emit("update:modelValue", false);
  } catch (e) {
    saveError.value = errorMessage(e);
  }
}

function close() {
  emit("update:modelValue", false);
}

const { onMousedown, onClick } = useMaskClose(close);
</script>

<template>
  <div v-if="modelValue" class="modal-mask" @mousedown="onMousedown" @click="onClick">
    <div class="modal connection-modal">
      <div class="modal-head">
        {{ isEdit ? "编辑连接" : "新建连接" }}
      </div>
      <div class="modal-body">
        <div class="form-row">
          <label>名称 *</label>
          <input v-model="form.name" class="input" placeholder="如：生产环境 S3" />
        </div>
        <div class="form-row">
          <label>Endpoint URL *</label>
          <input
            v-model="form.endpoint_url"
            class="input"
            placeholder="https://s3.example.com"
          />
          <div class="form-hint">以 http:// 或 https:// 开头</div>
        </div>
        <div class="form-row two">
          <div>
            <label>Region *</label>
            <input v-model="form.region" class="input" placeholder="us-east-1" />
          </div>
          <div>
            <label>限定桶（可选）</label>
            <input
              v-model="form.default_bucket"
              class="input"
              placeholder="留空显示桶列表"
            />
          </div>
        </div>
        <div class="form-hint form-row">
          留空则进入桶列表；填写后直接进入该桶（适合 AK 无 ListBuckets 权限的账号）。
        </div>
        <div class="form-row">
          <label>Access Key *</label>
          <input v-model="form.access_key" class="input" autocomplete="off" />
        </div>
        <div class="form-row">
          <label>Secret Key *</label>
          <div class="secret-row">
            <input
              v-model="form.secret_key"
              class="input"
              :type="showSecret ? 'text' : 'password'"
              autocomplete="off"
            />
            <button class="btn ghost sm" @click="showSecret = !showSecret">
              {{ showSecret ? "隐藏" : "显示" }}
            </button>
          </div>
          <div class="form-hint">凭据仅保存在本机配置文件中，不会上传。</div>
        </div>
        <div class="form-row">
          <label class="check-label">
            <input v-model="form.force_path_style" type="checkbox" />
            路径风格访问（path-style，MinIO / Ceph 等 S3 兼容服务需保持开启）
          </label>
        </div>
        <div v-if="saveError" class="form-error">{{ saveError }}</div>
      </div>
      <div class="modal-foot">
        <button
          v-if="isEdit"
          class="btn danger"
          :class="{ solid: confirmDelete }"
          @click="removeConnection"
        >
          {{ confirmDelete ? "再次点击确认删除" : "删除连接" }}
        </button>
        <span class="spacer" />
        <button class="btn" :disabled="testing" @click="testConnection">
          {{ testing ? "测试中…" : "测试连接" }}
        </button>
        <button class="btn" @click="close">取消</button>
        <button class="btn primary" :disabled="saving" @click="save">
          {{ saving ? "保存中…" : "保存" }}
        </button>
      </div>
      <div v-if="testMsg" class="test-result" :class="testOk ? 'ok' : 'bad'">
        {{ testMsg }}
      </div>
    </div>
  </div>
</template>

<style scoped>
.connection-modal {
  width: 520px;
}
.form-row.two {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}
.secret-row {
  display: flex;
  gap: 6px;
}
.secret-row .input {
  flex: 1;
}
.check-label {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  color: var(--text);
}
.modal-foot {
  align-items: center;
}
.modal-foot .spacer {
  flex: 1;
}
.btn.danger.solid {
  background: var(--danger);
  border-color: var(--danger);
  color: #fff;
}
.test-result {
  padding: 8px 20px 12px;
  font-size: 12px;
}
.test-result.ok {
  color: var(--ok);
}
.test-result.bad {
  color: var(--danger);
}
</style>
