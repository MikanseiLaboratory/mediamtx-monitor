/// パスカード。死活監視の一覧表示用。

import type { PathSample } from "../types";
import { formatDuration, formatMbps, formatNumber, formatTime } from "../lib/format";
import { PathBadge } from "./Badges";

export function PathCard({
  path,
  onSelect,
  active = false,
}: {
  path: PathSample;
  onSelect?: (name: string) => void;
  active?: boolean;
}) {
  return (
    <button
      type="button"
      className={`path-card ${active ? "active" : ""}`}
      data-testid={`path-card-${path.name}`}
      onClick={() => onSelect?.(path.name)}
    >
      <div className="path-card-head">
        <span className="path-name">{path.name}</span>
        <PathBadge severity={path.severity} />
      </div>
      <dl className="path-card-grid">
        <div>
          <dt>受信</dt>
          <dd>{formatMbps(path.inboundMbps)}</dd>
        </div>
        <div>
          <dt>送信</dt>
          <dd>{formatMbps(path.outboundMbps)}</dd>
        </div>
        <div>
          <dt>視聴者</dt>
          <dd>{formatNumber(path.readerCount, 0)}</dd>
        </div>
        <div>
          <dt>E2E 遅延（未算出）</dt>
          <dd>—</dd>
        </div>
      </dl>
      <div className="path-card-foot">
        <span>{path.sourceType || "ソースなし"}</span>
        <span>・</span>
        <span>{path.codecs.length ? path.codecs.join(" / ") : "トラックなし"}</span>
      </div>
      <div className="path-card-foot dim">
        <span>稼働: {formatDuration(path.onlineSince)}</span>
        <span>更新: {formatTime(path.onlineSince)}</span>
      </div>
    </button>
  );
}
