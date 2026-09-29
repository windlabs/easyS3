/**
 * 遮罩层点击关闭。
 *
 * 仅当鼠标「按下」与「抬起」都发生在遮罩本身时才触发关闭。
 * 若只在遮罩上监听 click，会出现以下误关闭：在输入框内拖选文本时，
 * 指针拖出弹窗落到遮罩上再松开，浏览器会把 click 派发到按下与抬起的
 * 最近公共祖先（即遮罩），从而误判为点击遮罩。
 */
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
