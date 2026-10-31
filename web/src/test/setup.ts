// jsdom には ResizeObserver が無いので、何もしない実装を差し込む。
class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

if (!("ResizeObserver" in globalThis)) {
  (globalThis as unknown as { ResizeObserver: typeof ResizeObserverStub }).ResizeObserver =
    ResizeObserverStub;
}

// HTMLCanvasElement#getContext / toDataURL は jsdom では未実装。
if (typeof HTMLCanvasElement !== "undefined") {
  HTMLCanvasElement.prototype.getContext = (() =>
    null) as unknown as typeof HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.toDataURL = () => "data:image/png;base64,UE5H";
}

// モックブリッジを使う（Tauri API は jsdom に無い）。
if (typeof window !== "undefined") {
  window.history.replaceState({}, "", "/?mock=1");
}
