import { useEffect, useRef, useState } from "react";
import { Mic, Square, Trash2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { formatDuration } from "@/lib/format";
import { Button } from "@/components/ui/button";
import { canRecord } from "@/lib/speech";

interface VoiceRecorderProps {
  onRecorded?: (info: { durationMs: number; url: string | null }) => void;
  onClear?: () => void;
  className?: string;
}

type RecorderState = "idle" | "recording" | "recorded" | "unsupported" | "denied";

export function VoiceRecorder({ onRecorded, onClear, className }: VoiceRecorderProps) {
  const [state, setState] = useState<RecorderState>(() =>
    canRecord() ? "idle" : "unsupported",
  );
  const [elapsed, setElapsed] = useState(0);
  const [url, setUrl] = useState<string | null>(null);
  const recorderRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const streamRef = useRef<MediaStream | null>(null);
  const startedAtRef = useRef(0);

  useEffect(() => {
    if (state !== "recording") return undefined;
    const timer = setInterval(() => {
      setElapsed(Date.now() - startedAtRef.current);
    }, 100);
    return () => clearInterval(timer);
  }, [state]);

  useEffect(
    () => () => {
      streamRef.current?.getTracks().forEach((track) => track.stop());
      if (url) URL.revokeObjectURL(url);
    },
    [url],
  );

  const start = async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      streamRef.current = stream;
      chunksRef.current = [];
      const recorder = new MediaRecorder(stream);
      recorder.ondataavailable = (event) => {
        if (event.data.size > 0) chunksRef.current.push(event.data);
      };
      recorder.onstop = () => {
        const blob = new Blob(chunksRef.current, { type: "audio/webm" });
        const objectUrl = URL.createObjectURL(blob);
        setUrl(objectUrl);
        setState("recorded");
        onRecorded?.({ durationMs: Date.now() - startedAtRef.current, url: objectUrl });
        stream.getTracks().forEach((track) => track.stop());
      };
      recorderRef.current = recorder;
      startedAtRef.current = Date.now();
      recorder.start();
      setElapsed(0);
      setState("recording");
    } catch {
      setState("denied");
    }
  };

  const stop = () => {
    recorderRef.current?.stop();
    recorderRef.current = null;
  };

  const reset = () => {
    if (url) URL.revokeObjectURL(url);
    setUrl(null);
    setElapsed(0);
    setState(canRecord() ? "idle" : "unsupported");
    onClear?.();
  };

  if (state === "unsupported" || state === "denied") {
    return (
      <div
        className={cn(
          "rounded-sm border border-dashed border-line px-3 py-2.5 text-2xs text-ink-muted",
          className,
        )}
      >
        {state === "denied"
          ? "麦克风权限被拒绝。可以在系统设置中允许访问，或继续用文字完成本次练习。"
          : "当前环境不支持录音。可以先在下方输入你的表达，接口已为真实语音预留。"}
      </div>
    );
  }

  return (
    <div className={cn("flex items-center gap-2.5", className)}>
      {state === "idle" && (
        <Button size="sm" variant="outline" onClick={start}>
          <Mic />
          开始录音
        </Button>
      )}

      {state === "recording" && (
        <>
          <Button size="sm" variant="destructive" onClick={stop}>
            <Square />
            停止
          </Button>
          <span className="flex items-center gap-1.5">
            <span className="flex items-end gap-0.5" aria-hidden>
              {[0, 1, 2, 3, 4].map((i) => (
                <span
                  key={i}
                  className="w-0.5 animate-pulse-soft rounded-full bg-danger"
                  style={{ height: 4 + ((i * 5) % 12), animationDelay: `${i * 120}ms` }}
                />
              ))}
            </span>
            <span className="font-mono text-2xs text-danger">{formatDuration(elapsed)}</span>
          </span>
        </>
      )}

      {state === "recorded" && (
        <>
          <span className="font-mono text-2xs text-success">
            已录制 {formatDuration(elapsed)}
          </span>
          {url && (
            <audio controls src={url} className="h-7 max-w-44">
              <track kind="captions" />
            </audio>
          )}
          <Button size="icon-sm" variant="ghost" onClick={reset} aria-label="删除录音">
            <Trash2 />
          </Button>
        </>
      )}
    </div>
  );
}
