/**
 * 语音能力的最小封装。
 * 第一阶段使用浏览器内置 SpeechSynthesis 做示范朗读，
 * 未来接入 Rust 侧 TTS / ASR 时只需替换这里的实现。
 */

export function canSpeak() {
  return typeof window !== "undefined" && "speechSynthesis" in window;
}

export function speak(text: string, options: { rate?: number; lang?: string } = {}) {
  if (!canSpeak()) return false;
  const synth = window.speechSynthesis;
  synth.cancel();
  const utterance = new SpeechSynthesisUtterance(text);
  utterance.lang = options.lang ?? "en-US";
  utterance.rate = options.rate ?? 0.92;
  const voices = synth.getVoices();
  const preferred = voices.find((v) => /en-US|en-GB/.test(v.lang));
  if (preferred) utterance.voice = preferred;
  synth.speak(utterance);
  return true;
}

export function stopSpeaking() {
  if (canSpeak()) window.speechSynthesis.cancel();
}

export function canRecord() {
  return (
    typeof navigator !== "undefined" &&
    Boolean(navigator.mediaDevices?.getUserMedia) &&
    typeof MediaRecorder !== "undefined"
  );
}
