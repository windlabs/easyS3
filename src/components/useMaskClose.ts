/**
 * 遮罩层点击关闭。
 *
 * 仅当鼠标「按下」与「抬起」都发生在遮罩本身时才触发关闭。
 * 若只在遮罩上监听 click，会出现以下误关闭：在输入框内拖选文本时，
 * 指针拖出弹窗落到遮罩上再松开，浏览器会把 click 派发到按下与抬起的
 * 最近公共祖先（即遮罩），从而误判为点击遮罩。
 */
import { ref } from "vue";
export function useMaskClose(close: () => void) {
  let startedOnMask = false;

  function onMousedown(e: MouseEvent) {
    startedOnMask = e.target === e.currentTarget;
  }

  function onClick(e: MouseEvent) {
    if (startedOnMask && e.target === e.currentTarget) close();
    startedOnMask = false;
  }

  return { onMousedown, onClick };
}

/** 为常用弹窗提供初始焦点、Esc 关闭和 Tab 焦点循环。 */
export function useDialogFocus(open: () => boolean, close: () => void) {
  const root = ref<HTMLElement | null>(null);
  let previous: HTMLElement | null = null;
  function focusables() {
    return Array.from(root.value?.querySelectorAll<HTMLElement>(
      'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])',
    ) ?? []);
  }
  function onKeydown(event: KeyboardEvent) {
    if (!open()) return;
    if (event.key === "Escape") { event.preventDefault(); close(); return; }
    if (event.key !== "Tab") return;
    const items = focusables();
    if (!items.length) return;
    const index = items.indexOf(document.activeElement as HTMLElement);
    const next = event.shiftKey ? (index <= 0 ? items.length - 1 : index - 1) : (index + 1) % items.length;
    if (index < 0 || next !== index + (event.shiftKey ? -1 : 1)) {
      event.preventDefault(); items[next]?.focus();
    }
  }
  function onOpen() {
    previous = document.activeElement as HTMLElement | null;
    requestAnimationFrame(() => focusables()[0]?.focus());
  }
  function onClose() { requestAnimationFrame(() => previous?.focus()); }
  return { root, onKeydown, onOpen, onClose };
}
