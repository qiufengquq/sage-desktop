import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { HashRouter } from "react-router";
import { App } from "./app/App";
import { useLearningStore } from "./stores/learning-store";
import "./styles/globals.css";

// 先恢复本地学习状态（没有则种子化），再渲染，避免首屏出现空数据闪烁
useLearningStore.getState().init();

const container = document.getElementById("root");
if (!container) throw new Error("找不到 #root 挂载点");

createRoot(container).render(
  <StrictMode>
    <HashRouter>
      <App />
    </HashRouter>
  </StrictMode>,
);
