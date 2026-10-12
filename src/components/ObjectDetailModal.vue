<script setup lang="ts">
import { ref, watch } from "vue";
import * as api from "../api";
import type { ObjectAclInfo, ObjectDetail, ObjectTag, ObjectVersionInfo } from "../types";
import { formatBytes, formatTime } from "../utils";
import { useMaskClose } from "./useMaskClose";

const emit = defineEmits<{ "update:modelValue": [value: boolean] }>();
const { onMousedown, onClick } = useMaskClose(() => emit("update:modelValue", false));
const props = defineProps<{ modelValue: boolean; detail: ObjectDetail | null; bucket: string }>();
const versions = ref<ObjectVersionInfo[]>([]);
const versionsLoading = ref(false);
const versionsError = ref("");
const tags = ref<ObjectTag[]>([]);
const acl = ref<ObjectAclInfo | null>(null);
const accessError = ref("");
const accessLoaded = ref(false);
const editingMetadata = ref(false);
const editContentType = ref("");
const editMetadata = ref<ObjectTag[]>([]);
const restoreDays = ref(7);
const restoreTier = ref("Standard");

watch(() => [props.modelValue, props.detail?.key] as const, ([open]) => {
  if (!open) { versions.value = []; versionsError.value = ""; }
});

async function loadVersions() {
  if (!props.detail) return;
  versionsLoading.value = true; versionsError.value = "";
  try { versions.value = await api.objectVersions(props.bucket, props.detail.key); }
  catch (e) { versionsError.value = e instanceof Error ? e.message : String(e); }
  finally { versionsLoading.value = false; }
}

async function restore(version: ObjectVersionInfo) {
  if (!props.detail || version.is_delete_marker) return;
  if (!confirm(`将恢复此历史版本为当前对象，是否继续？`)) return;
  try { await api.restoreObjectVersion(props.bucket, props.detail.key, version.version_id); await loadVersions(); }
  catch (e) { versionsError.value = e instanceof Error ? e.message : String(e); }
}

async function loadAccess() {
  if (!props.detail) return;
  accessError.value = ""; accessLoaded.value = true;
  try { tags.value = await api.objectTags(props.bucket, props.detail.key); }
  catch (e) { accessError.value = `标签：${e instanceof Error ? e.message : String(e)}`; }
  try { acl.value = await api.objectAcl(props.bucket, props.detail.key); }
  catch (e) { accessError.value += `${accessError.value ? "；" : ""}ACL：${e instanceof Error ? e.message : String(e)}`; }
}
function addTag() { tags.value.push({ key: "", value: "" }); }
function removeTag(index: number) { tags.value.splice(index, 1); }
async function saveTags() {
  if (!props.detail) return;
  try { await api.saveObjectTags(props.bucket, props.detail.key, tags.value); await loadAccess(); }
  catch (e) { accessError.value = e instanceof Error ? e.message : String(e); }
}
function beginMetadataEdit() { if (!props.detail) return; editContentType.value = props.detail.content_type || "application/octet-stream"; editMetadata.value = props.detail.metadata.map((item) => ({ key: item.key, value: item.value })); editingMetadata.value = true; }
async function saveMetadata() { if (!props.detail) return; try { await api.replaceObjectMetadata(props.bucket, props.detail.key, editContentType.value, editMetadata.value); editingMetadata.value = false; emit("update:modelValue", false); } catch (e) { versionsError.value = e instanceof Error ? e.message : String(e); } }
function archival() { return props.detail?.storage_class === "GLACIER" || props.detail?.storage_class === "DEEP_ARCHIVE"; }
async function restoreArchive() { if (!props.detail || !confirm("将发起归档对象恢复请求，是否继续？")) return; try { await api.restoreArchivedObject(props.bucket, props.detail.key, restoreDays.value, restoreTier.value); } catch (e) { versionsError.value = e instanceof Error ? e.message : String(e); } }
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
          <dt>服务端加密</dt><dd>{{ detail.server_side_encryption || "未启用" }}</dd>
        </dl>
        <div v-if="archival()" class="meta-title">归档恢复</div>
        <div v-if="archival()" class="access-panel"><p>状态：{{ detail.restore_status || "尚未恢复" }}</p><label class="tag-row"><span>保留天数</span><input v-model.number="restoreDays" class="input" type="number" min="1" max="365" /></label><label class="tag-row"><span>恢复速度</span><select v-model="restoreTier" class="select"><option value="Expedited">快速</option><option value="Standard">标准</option><option value="Bulk">批量</option></select></label><button class="btn" @click="restoreArchive">发起恢复</button></div>
        <div class="meta-title version-head"><span>对象元数据</span><button class="btn sm" @click="beginMetadataEdit">编辑</button></div>
        <div v-if="editingMetadata" class="access-panel"><label class="tag-row"><span>Content-Type</span><input v-model="editContentType" class="input" /></label><div v-for="(item, index) in editMetadata" :key="index" class="tag-row"><input v-model="item.key" class="input" placeholder="x-amz-meta-* 名称" /><input v-model="item.value" class="input" placeholder="值" /><button class="btn sm" @click="editMetadata.splice(index, 1)">删除</button></div><div class="tag-actions"><button class="btn sm" @click="editMetadata.push({ key: '', value: '' })">新增元数据</button><button class="btn sm primary" @click="saveMetadata">保存</button></div></div>
        <div v-if="detail.metadata.length" class="meta-title">自定义元数据</div>
        <dl v-if="detail.metadata.length">
          <template v-for="item in detail.metadata" :key="item.key">
            <dt>{{ item.key }}</dt><dd class="break">{{ item.value }}</dd>
          </template>
        </dl>
        <div class="meta-title version-head"><span>版本</span><button class="btn sm" :disabled="versionsLoading" @click="loadVersions">{{ versionsLoading ? "加载中…" : "查看版本" }}</button></div>
        <div v-if="versionsError" class="version-error">{{ versionsError }}</div>
        <div v-if="versions.length" class="versions">
          <div v-for="version in versions" :key="version.version_id" class="version-row">
            <span>{{ version.is_delete_marker ? "删除标记" : formatBytes(version.size) }}</span>
            <span>{{ formatTime(version.last_modified) }}</span>
            <span :title="version.version_id">{{ version.is_latest ? "当前版本" : version.version_id }}</span>
            <button v-if="!version.is_delete_marker && !version.is_latest" class="btn sm" @click="restore(version)">恢复</button>
          </div>
        </div>
        <div class="meta-title version-head"><span>标签与访问控制</span><button class="btn sm" @click="loadAccess">查看</button></div>
        <div v-if="accessError" class="version-error">{{ accessError }}</div>
        <div v-if="accessLoaded" class="access-panel">
          <div v-for="(tag, index) in tags" :key="index" class="tag-row"><input v-model="tag.key" class="input" placeholder="标签名" /><input v-model="tag.value" class="input" placeholder="标签值" /><button class="btn sm" @click="removeTag(index)">删除</button></div>
          <div class="tag-actions"><button class="btn sm" @click="addTag">新增标签</button><button class="btn sm primary" @click="saveTags">保存标签</button></div>
          <div v-if="acl" class="acl"><b>ACL（只读）</b><div>所有者：{{ acl.owner || "—" }}</div><div v-for="grant in acl.grants" :key="grant">{{ grant }}</div></div>
        </div>
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
.version-head { display: flex; align-items: center; justify-content: space-between; }
.versions { border-top: 1px solid var(--border); }
.version-row { display: grid; grid-template-columns: 90px 1fr minmax(100px, 1fr) auto; gap: 8px; align-items: center; padding: 7px 0; border-bottom: 1px solid var(--border); font-size: 12px; }
.version-row span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.version-error { color: var(--danger); font-size: 12px; }
.access-panel { display: grid; gap: 7px; }
.tag-row { display: grid; grid-template-columns: 1fr 1fr auto; gap: 6px; }
.tag-actions { display: flex; gap: 6px; }
.acl { margin-top: 8px; padding-top: 8px; border-top: 1px solid var(--border); font-size: 12px; display: grid; gap: 4px; }
</style>
