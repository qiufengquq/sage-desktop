import { useEffect } from "react";

/** 注册全局快捷键，例如 ⌘K / Ctrl+K */
export function useHotkey(
  combo: { key: string; meta?: boolean; ctrl?: boolean; shift?: boolean },
  handler: (event: KeyboardEvent) => void,
) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key.toLowerCase() !== combo.key.toLowerCase()) return;
      const metaOk = combo.meta ? event.metaKey : true;
      const ctrlOk = combo.ctrl ? event.ctrlKey : true;
      const shiftOk = combo.shift ? event.shiftKey : true;
      if (!metaOk || !ctrlOk || !shiftOk) return;
      handler(event);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [combo.key, combo.meta, combo.ctrl, combo.shift, handler]);
}

export function isMac() {
  return typeof navigator !== "undefined" && /mac/i.test(navigator.platform);
}

/** 统一显示平台对应的修饰键 */
export function modKeyLabel() {
  return isMac() ? "⌘" : "Ctrl";
}
