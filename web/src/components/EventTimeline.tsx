/// 遷移イベントのタイムライン。

import type { StatusEvent } from "../types";
import { formatTime } from "../lib/format";

const KIND_LABEL: Record<string, string> = {
  srt_conn_appeared: "SRT 接続",
  srt_conn_dropped: "SRT 切断",
  srt_degraded: "品質劣化",
  srt_recovered: "品質回復",
  path_down: "パス消失",
  path_recovered: "パス復帰",
  path_online_stopped: "配信停止",
  path_online_started: "配信開始",
  source_changed: "ソース変更",
  api_unreachable: "API 不可",
  api_restored: "API 復帰",
  monitor_started: "監視開始",
  monitor_stopped: "監視停止",
};

export function EventTimeline({ events, limit = 40 }: { events: StatusEvent[]; limit?: number }) {
  const recent = [...events].slice(-limit).reverse();
  if (recent.length === 0) {
    return (
      <div className="empty-state" role="status">
        <div className="empty-title">まだ状態変化はありません</div>
        <div className="empty-hint">切断・劣化・復帰が起きるとここに並びます。</div>
      </div>
    );
  }
  return (
    <ol className="timeline" data-testid="event-timeline">
      {recent.map((e, i) => (
        <li key={`${e.seq}-${e.target}-${i}`} className={`timeline-item sev-${e.severity}`}>
          <span className="timeline-time">{formatTime(e.at)}</span>
          <span className={`badge sev-${e.severity}`}>{KIND_LABEL[e.kind] ?? e.kind}</span>
          <span className="timeline-message">{e.message}</span>
        </li>
      ))}
    </ol>
  );
}
