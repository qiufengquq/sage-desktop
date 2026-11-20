import { Component, type ErrorInfo, type ReactNode } from "react";
import { AlertTriangle, RotateCcw } from "lucide-react";
import { Button } from "@/components/ui/button";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/**
 * 渲染期错误兜底。
 * 第一阶段没有它，任何渲染异常都会整屏白掉；现在至少能告诉用户发生了什么，
 * 并保留"重试"与"回首页"两条恢复路径。
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("[sage] 渲染异常", error, info.componentStack);
  }

  private reset = () => {
    this.setState({ error: null });
  };

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;

    return (
      <div className="flex min-h-screen items-center justify-center bg-paper px-6">
        <div className="w-full max-w-[46ch] rounded-md border border-line bg-surface p-6">
          <div className="flex items-center gap-2 text-danger">
            <AlertTriangle className="size-4" />
            <h1 className="text-[15px] font-medium">页面渲染出错了</h1>
          </div>
          <p className="mt-2 text-[12.5px] leading-relaxed text-ink-soft">
            你的学习数据仍然保存在本机。可以先重试；如果反复出现，请刷新应用。
          </p>
          <pre className="mt-3 max-h-32 overflow-auto rounded-sm bg-sunken px-2.5 py-2 font-mono text-2xs text-ink-muted">
            {error.message}
          </pre>
          <div className="mt-4 flex items-center gap-2">
            <Button onClick={this.reset}>
              <RotateCcw />
              重试
            </Button>
            <Button
              variant="outline"
              onClick={() => {
                window.location.hash = "#/";
                window.location.reload();
              }}
            >
              回首页并刷新
            </Button>
          </div>
        </div>
      </div>
    );
  }
}
