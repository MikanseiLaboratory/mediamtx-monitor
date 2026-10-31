//! OS 通知（Windows のトースト等）。設定の規則に従って遷移イベントを間引く。

use mtm_core::config::NotifyRules;
use mtm_core::events::{StatusEvent, StatusEventKind};
use tauri::{AppHandle, Runtime};
use tauri_plugin_notification::NotificationExt;

/// 規則に合致するイベントだけを通知する。1 周期あたり最大 5 件。
pub fn dispatch<R: Runtime>(app: &AppHandle<R>, rules: &NotifyRules, events: &[StatusEvent]) {
    if !rules.enabled {
        return;
    }
    let mut sent = 0usize;
    for event in events {
        if !wanted(rules, event) {
            continue;
        }
        if sent >= 5 {
            log::info!("通知の上限に達したため残りは省略します");
            break;
        }
        if let Err(err) = app
            .notification()
            .builder()
            .title(title_for(event.kind))
            .body(&event.message)
            .show()
        {
            log::warn!("通知の送信に失敗しました: {err}");
        }
        sent += 1;
    }
}

fn wanted(rules: &NotifyRules, event: &StatusEvent) -> bool {
    match event.kind {
        StatusEventKind::SrtConnDropped => rules.srt_conn_dropped,
        StatusEventKind::SrtConnAppeared => rules.srt_conn_appeared,
        StatusEventKind::SrtDegraded => rules.srt_degraded,
        StatusEventKind::PathDown | StatusEventKind::PathOnlineStopped => rules.path_down,
        StatusEventKind::ApiUnreachable => rules.api_unreachable,
        // 復帰・開始/停止などは画面内のタイムラインにだけ出す。
        _ => false,
    }
}

fn title_for(kind: StatusEventKind) -> &'static str {
    match kind {
        StatusEventKind::SrtConnDropped => "SRT 接続が切断されました",
        StatusEventKind::SrtConnAppeared => "SRT 接続が確立しました",
        StatusEventKind::SrtDegraded => "SRT 品質が劣化しています",
        StatusEventKind::SrtRecovered => "SRT 品質が回復しました",
        StatusEventKind::PathDown | StatusEventKind::PathOnlineStopped => "配信が停止しました",
        StatusEventKind::PathRecovered | StatusEventKind::PathOnlineStarted => "配信が復帰しました",
        StatusEventKind::ApiUnreachable => "MediaMTX に接続できません",
        StatusEventKind::ApiRestored => "MediaMTX への接続が回復しました",
        StatusEventKind::SourceChanged => "ソースが変更されました",
        StatusEventKind::MonitorStarted => "監視を開始しました",
        StatusEventKind::MonitorStopped => "監視を停止しました",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use mtm_core::events::EventSeverity;

    fn ev(kind: StatusEventKind) -> StatusEvent {
        StatusEvent {
            seq: 1,
            at: chrono::FixedOffset::east_opt(0)
                .unwrap()
                .timestamp_opt(0, 0)
                .unwrap(),
            kind,
            target: "t".into(),
            label: "t".into(),
            message: "m".into(),
            severity: EventSeverity::Warning,
        }
    }

    #[test]
    fn rules_filter_events() {
        let rules = NotifyRules {
            enabled: true,
            srt_conn_dropped: true,
            srt_conn_appeared: false,
            srt_degraded: true,
            path_down: false,
            api_unreachable: true,
        };
        assert!(wanted(&rules, &ev(StatusEventKind::SrtConnDropped)));
        assert!(!wanted(&rules, &ev(StatusEventKind::SrtConnAppeared)));
        assert!(!wanted(&rules, &ev(StatusEventKind::PathDown)));
        assert!(wanted(&rules, &ev(StatusEventKind::SrtDegraded)));
        assert!(wanted(&rules, &ev(StatusEventKind::ApiUnreachable)));
        assert!(!wanted(&rules, &ev(StatusEventKind::SrtRecovered)));
    }

    #[test]
    fn titles_are_japanese() {
        assert!(title_for(StatusEventKind::SrtConnDropped).contains("SRT"));
        assert!(!title_for(StatusEventKind::ApiUnreachable).is_empty());
    }
}
