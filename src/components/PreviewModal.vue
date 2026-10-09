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

// ---------- 图片 / PDF / 音视频：base64 data URL 内嵌，不经临时文件 ----------

const dataUrl = computed(() => {
  if (props.data?.kind === "image" || props.data?.kind === "media") {
    return `data:${props.data.mime};base64,${props.data.data_base64}`;
  }
  return "";
});

const isPdf = computed(() => props.data?.kind === "media" && props.data.mime === "application/pdf");
const isVideo = computed(
  () => props.data?.kind === "media" && props.data.mime.startsWith("video/"),
);
const isAudio = computed(
  () => props.data?.kind === "media" && props.data.mime.startsWith("audio/"),
);

// ---------- 文本：代码高亮 / Markdown 只读视图（全部文本节点，禁止 HTML 注入） ----------

/** 大文本渲染保护：超过该行数截断并提示（预览不是编辑器） */
const MAX_RENDER_LINES = 5000;

const isMarkdown = computed(() => /\.(md|markdown)$/i.test(props.name));

interface CodeToken {
  text: string;
  cls: string;
}

const TOKEN_RE =
  /("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|`(?:[^`\\]|\\.)*`)|(\/\/[^\n]*|#[^\n]*)|\b(\d+(?:\.\d+)?)\b|\b(const|let|var|function|return|if|else|for|while|switch|case|break|continue|import|export|from|class|interface|extends|async|await|try|catch|throw|def|lambda|print|func|go|package|type|struct|impl|pub|use|fn|mut|match|trait|new|delete|select|insert|update|where|true|false|null|nil|None|True|False|undefined)\b/g;

/** 单行分词：字符串 / 注释 / 数字 / 关键词着色，其余原样（纯文本节点渲染） */
function tokenizeLine(line: string): CodeToken[] {
  const tokens: CodeToken[] = [];
  let last = 0;
  for (const m of line.matchAll(TOKEN_RE)) {
    const idx = m.index ?? 0;
    if (idx > last) tokens.push({ text: line.slice(last, idx), cls: "" });
    const cls = m[1] ? "tk-str" : m[2] ? "tk-cmt" : m[3] ? "tk-num" : "tk-kw";
    tokens.push({ text: m[0], cls });
    last = idx + m[0].length;
  }
  if (last < line.length) tokens.push({ text: line.slice(last), cls: "" });
  return tokens;
}

const codeLines = computed(() => {
  if (props.data?.kind !== "text" || isMarkdown.value) return [];
  return props.data.content.slice(0, 200_000).split("\n").map(tokenizeLine);
});
const codeTruncated = computed(() => codeLines.value.length > MAX_RENDER_LINES);
const codeRenderLines = computed(() => codeLines.value.slice(0, MAX_RENDER_LINES));

// ---------- Markdown：解析为安全块结构，行内 **粗体** / *斜体* / `代码` 着色 ----------

type MdBlock =
  | { type: "heading"; level: number; tokens: CodeToken[] }
  | { type: "code"; lines: string[] }
  | { type: "quote"; tokens: CodeToken[] }
  | { type: "list"; items: CodeToken[][] }
  | { type: "para"; tokens: CodeToken[] };

const INLINE_RE = /(\*\*[^*]+\*\*|\*[^*]+\*|`[^`]+`)/g;

/** 行内标记分词：**粗体** / *斜体* / `代码`（文本节点 + class，无 HTML） */
function inlineTokens(text: string): CodeToken[] {
  const tokens: CodeToken[] = [];
  let last = 0;
  for (const m of text.matchAll(INLINE_RE)) {
    const idx = m.index ?? 0;
    if (idx > last) tokens.push({ text: text.slice(last, idx), cls: "" });
    const raw = m[0];
    if (raw.startsWith("**")) tokens.push({ text: raw.slice(2, -2), cls: "md-b" });
    else if (raw.startsWith("`")) tokens.push({ text: raw.slice(1, -1), cls: "md-code" });
    else tokens.push({ text: raw.slice(1, -1), cls: "md-i" });
    last = idx + raw.length;
  }
  if (last < text.length) tokens.push({ text: text.slice(last), cls: "" });
  return tokens;
}

function parseMarkdown(src: string): MdBlock[] {
  const lines = src.split("\n");
  const blocks: MdBlock[] = [];
  let inCode = false;
  let codeBuf: string[] = [];
  for (const line of lines) {
    if (line.trimStart().startsWith("```")) {
      if (inCode) {
        blocks.push({ type: "code", lines: codeBuf });
        codeBuf = [];
      }
      inCode = !inCode;
      continue;
    }
    if (inCode) {
      codeBuf.push(line);
      continue;
    }
    const heading = line.match(/^(#{1,4})\s+(.*)$/);
    if (heading) {
      blocks.push({
        type: "heading",
        level: heading[1].length,
        tokens: inlineTokens(heading[2]),
      });
      continue;
    }
    if (/^(\*|-|\+)\s+/.test(line)) {
      const tokens = inlineTokens(line.replace(/^(\*|-|\+)\s+/, ""));
      const prev = blocks[blocks.length - 1];
      if (prev?.type === "list") prev.items.push(tokens);
      else blocks.push({ type: "list", items: [tokens] });
      continue;
    }
    if (/^>\s?/.test(line)) {
      blocks.push({ type: "quote", tokens: inlineTokens(line.replace(/^>\s?/, "")) });
      continue;
    }
    if (line.trim() === "") continue;
    blocks.push({ type: "para", tokens: inlineTokens(line) });
  }
  if (inCode && codeBuf.length) blocks.push({ type: "code", lines: codeBuf });
  return blocks;
}

const mdBlocks = computed(() =>
  props.data?.kind === "text" && isMarkdown.value
    ? parseMarkdown(props.data.content.slice(0, 200_000))
    : [],
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
        <img v-if="data?.kind === 'image'" :src="dataUrl" :alt="name" />
        <iframe
          v-else-if="isPdf"
          :src="dataUrl"
          :title="name"
          class="pdf-frame"
        />
        <video v-else-if="isVideo" :src="dataUrl" controls />
        <audio v-else-if="isAudio" :src="dataUrl" controls />

        <!-- Markdown 只读视图（无 HTML 渲染） -->
        <div v-else-if="mdBlocks.length" class="md-view">
          <template v-for="(block, i) in mdBlocks" :key="i">
            <h3 v-if="block.type === 'heading' && block.level <= 2"><span v-for="(t, j) in block.tokens" :key="j" :class="t.cls">{{ t.text }}</span></h3>
            <h4 v-else-if="block.type === 'heading'"><span v-for="(t, j) in block.tokens" :key="j" :class="t.cls">{{ t.text }}</span></h4>
            <pre v-else-if="block.type === 'code'" class="md-codeblock">{{ block.lines.join("\n") }}</pre>
            <blockquote v-else-if="block.type === 'quote'"><span v-for="(t, j) in block.tokens" :key="j" :class="t.cls">{{ t.text }}</span></blockquote>
            <ul v-else-if="block.type === 'list'">
              <li v-for="(item, j) in block.items" :key="j"><span v-for="(t, k) in item" :key="k" :class="t.cls">{{ t.text }}</span></li>
            </ul>
            <p v-else><span v-for="(t, j) in block.tokens" :key="j" :class="t.cls">{{ t.text }}</span></p>
          </template>
        </div>

        <!-- 代码 / JSON / 普通文本：纯文本高亮（文本节点渲染） -->
        <pre v-else-if="data?.kind === 'text'" class="code-view"><span v-for="(tokens, i) in codeRenderLines" :key="i" class="code-line"><span v-for="(t, j) in tokens" :key="j" :class="t.cls">{{ t.text }}</span>
</span><span v-if="codeTruncated" class="truncated-tip">… 内容过长，仅显示前 {{ MAX_RENDER_LINES }} 行，请下载查看完整内容</span></pre>
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
  background: var(--preview-bg);
}
.preview-body img {
  max-width: 100%;
  max-height: 65vh;
  object-fit: contain;
}
.pdf-frame {
  width: 100%;
  height: 70vh;
  border: 1px solid var(--border);
  background: #fff;
}
.preview-body video {
  max-width: 100%;
  max-height: 65vh;
}
.preview-body audio {
  width: 80%;
}
.code-view,
.md-view {
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
  width: 100%;
}
.code-view {
  display: block;
}
.code-line {
  display: block;
}
.tk-str {
  color: #b3592b;
}
.tk-cmt {
  color: var(--muted);
  font-style: italic;
}
.tk-num {
  color: #8f5bb3;
}
.tk-kw {
  color: #2563a8;
  font-weight: 600;
}
.truncated-tip {
  display: block;
  margin-top: 10px;
  color: var(--warn);
}
.md-view {
  font-family: inherit;
  font-size: 13px;
}
.md-view h3,
.md-view h4 {
  margin: 14px 0 8px;
}
.md-view p {
  margin: 8px 0;
}
.md-view ul {
  margin: 8px 0;
  padding-left: 22px;
}
.md-view blockquote {
  margin: 8px 0;
  padding: 4px 12px;
  border-left: 3px solid var(--border);
  color: var(--muted);
}
.md-b {
  font-weight: 700;
}
.md-i {
  font-style: italic;
}
.md-code {
  font-family: "JetBrains Mono", Consolas, monospace;
  background: var(--hover);
  padding: 1px 4px;
  border-radius: 3px;
}
.md-codeblock {
  margin: 8px 0;
  padding: 10px;
  background: var(--hover);
  border-radius: 4px;
  white-space: pre-wrap;
}
</style>
