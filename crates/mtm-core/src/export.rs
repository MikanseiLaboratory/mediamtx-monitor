//! 外部クレートを使わず、SVG を直接組み立てる。uPlot の表示と近い見た目に
//! なるよう、色・余白・目盛りの方針をそろえてある（フロントの `theme.ts` と
//! 同じ 8 色パレット）。

use crate::ring::SamplePoint;
use std::fmt::Write as _;

/// SVG の系列 1 本。
#[derive(Debug, Clone, PartialEq)]
pub struct SvgSeries {
    pub label: String,
    pub unit: String,
    pub color: String,
    pub points: Vec<SamplePoint>,
}

/// CSV の 1 列。
#[derive(Debug, Clone, PartialEq)]
pub struct CsvColumn {
    pub metric: String,
    pub unit: String,
    pub points: Vec<SamplePoint>,
}

/// 書き出し形式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Svg,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Svg => "svg",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "csv" => Some(Self::Csv),
            "svg" => Some(Self::Svg),
            _ => None,
        }
    }
}

/// SVG に埋め込む時刻表示のタイムゾーンオフセット（分）。
fn format_time(ms: i64, offset_minutes: i32) -> String {
    use chrono::{FixedOffset, TimeZone, Utc};
    let off = FixedOffset::east_opt(offset_minutes * 60)
        .unwrap_or_else(|| FixedOffset::east_opt(0).expect("UTC"));
    let dt = Utc
        .timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().expect("epoch valid"))
        .with_timezone(&off);
    dt.format("%H:%M:%S").to_string()
}

fn format_iso(ms: i64, offset_minutes: i32) -> String {
    use chrono::{FixedOffset, TimeZone, Utc};
    let off = FixedOffset::east_opt(offset_minutes * 60)
        .unwrap_or_else(|| FixedOffset::east_opt(0).expect("UTC"));
    Utc.timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().expect("epoch valid"))
        .with_timezone(&off)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, false)
}

/// 期間指定 CSV を生成する。欠測は空欄（0 ではない）。
///
/// 列は「メトリクス名 (unit)」。行は時刻の和集合（昇順）。
pub fn to_csv(columns: &[CsvColumn], offset_minutes: i32) -> String {
    let mut out = String::new();
    out.push_str("timestamp,");
    for (i, c) in columns.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{}{}",
            csv_escape(&c.metric),
            if c.unit.is_empty() {
                String::new()
            } else {
                format!(" ({})", c.unit)
            }
        );
    }
    out.push('\n');

    let mut times: Vec<i64> = columns
        .iter()
        .flat_map(|c| c.points.iter().map(|p| p.at_ms))
        .collect();
    times.sort_unstable();
    times.dedup();

    for t in times {
        out.push_str(&format_iso(t, offset_minutes));
        for c in columns {
            out.push(',');
            if let Some(v) = c
                .points
                .iter()
                .find(|p| p.at_ms == t)
                .and_then(|p| p.value)
                .filter(|v| v.is_finite())
            {
                let _ = write!(out, "{v:.3}");
            }
        }
        out.push('\n');
    }
    out
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// SVG の寸法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SvgSize {
    pub width: u32,
    pub height: u32,
}

impl Default for SvgSize {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 360,
        }
    }
}

/// 複数系列を 1 枚の SVG に描く。`title` に期間などを入れる。
pub fn to_svg(title: &str, series: &[SvgSeries], size: SvgSize, offset_minutes: i32) -> String {
    const PAD_L: f64 = 68.0;
    const PAD_R: f64 = 24.0;
    const PAD_T: f64 = 56.0;
    const PAD_B: f64 = 52.0;

    let w = f64::from(size.width);
    let h = f64::from(size.height);
    let plot_w = (w - PAD_L - PAD_R).max(10.0);
    let plot_h = (h - PAD_T - PAD_B).max(10.0);

    // 時間範囲
    let mut t_min = i64::MAX;
    let mut t_max = i64::MIN;
    for s in series {
        for p in &s.points {
            t_min = t_min.min(p.at_ms);
            t_max = t_max.max(p.at_ms);
        }
    }
    if t_min == i64::MAX {
        t_min = 0;
        t_max = 1_000;
    }
    if t_max <= t_min {
        t_max = t_min + 1_000;
    }

    // 値範囲
    let mut v_min = f64::INFINITY;
    let mut v_max = f64::NEG_INFINITY;
    for s in series {
        for p in &s.points {
            if let Some(v) = p.value.filter(|v| v.is_finite()) {
                v_min = v_min.min(v);
                v_max = v_max.max(v);
            }
        }
    }
    if !v_min.is_finite() || !v_max.is_finite() {
        v_min = 0.0;
        v_max = 1.0;
    }
    let (v_lo, v_hi) = nice_bounds(v_min, v_max);

    let x = |t: i64| PAD_L + (t - t_min) as f64 / (t_max - t_min) as f64 * plot_w;
    let y = |v: f64| PAD_T + plot_h - (v - v_lo) / (v_hi - v_lo) * plot_h;

    let mut svg = String::with_capacity(4096);
    let _ = writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="'Segoe UI', 'Yu Gothic UI', sans-serif">"#
    );
    let _ = writeln!(
        svg,
        r##"<rect x="0" y="0" width="{w}" height="{h}" fill="#11161d"/>"##
    );
    let _ = writeln!(
        svg,
        r##"<text x="{PAD_L}" y="28" fill="#e8edf4" font-size="16" font-weight="600">{}</text>"##,
        xml_escape(title)
    );

    // Y 軸グリッド + 目盛
    let steps = 5;
    for i in 0..=steps {
        let frac = f64::from(i) / f64::from(steps);
        let v = v_lo + (v_hi - v_lo) * frac;
        let yy = y(v);
        let _ = writeln!(
            svg,
            r##"<line x1="{PAD_L}" y1="{yy:.1}" x2="{:.1}" y2="{yy:.1}" stroke="#2a3442" stroke-width="1"/>"##,
            PAD_L + plot_w
        );
        let _ = writeln!(
            svg,
            r##"<text x="{:.1}" y="{:.1}" fill="#8b98a9" font-size="11" text-anchor="end">{}</text>"##,
            PAD_L - 8.0,
            yy + 4.0,
            fmt_tick(v)
        );
    }

    // X 軸目盛
    let xticks = 6;
    for i in 0..=xticks {
        let frac = f64::from(i) / f64::from(xticks);
        let t = t_min + ((t_max - t_min) as f64 * frac) as i64;
        let xx = x(t);
        let _ = writeln!(
            svg,
            r##"<text x="{xx:.1}" y="{:.1}" fill="#8b98a9" font-size="11" text-anchor="middle">{}</text>"##,
            PAD_T + plot_h + 18.0,
            format_time(t, offset_minutes)
        );
    }
    let _ = writeln!(
        svg,
        r##"<line x1="{PAD_L}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="#3a4656" stroke-width="1"/>"##,
        PAD_T + plot_h,
        PAD_L + plot_w,
        PAD_T + plot_h
    );

    // 系列
    for s in series {
        let mut path = String::new();
        let mut pen_down = false;
        for p in &s.points {
            match p.value.filter(|v| v.is_finite()) {
                Some(v) => {
                    let cmd = if pen_down { 'L' } else { 'M' };
                    let _ = write!(path, "{cmd}{:.1},{:.1} ", x(p.at_ms), y(v));
                    pen_down = true;
                }
                None => pen_down = false,
            }
        }
        if !path.is_empty() {
            let _ = writeln!(
                svg,
                r#"<path d="{}" fill="none" stroke="{}" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round"/>"#,
                path.trim_end(),
                xml_escape(&s.color)
            );
        }
    }

    // 凡例
    let mut lx = PAD_L;
    for s in series {
        let _ = writeln!(
            svg,
            r#"<rect x="{lx:.1}" y="46" width="10" height="10" rx="2" fill="{}"/>"#,
            xml_escape(&s.color)
        );
        let _ = writeln!(
            svg,
            r##"<text x="{:.1}" y="55" fill="#c8d2df" font-size="12">{}</text>"##,
            lx + 15.0,
            xml_escape(&s.label)
        );
        lx += 15.0 + s.label.chars().count() as f64 * 12.0 + 24.0;
    }
    svg.push_str("</svg>\n");
    svg
}

fn fmt_tick(v: f64) -> String {
    let abs = v.abs();
    if abs >= 1000.0 {
        format!("{:.0}", v)
    } else if abs >= 10.0 {
        format!("{v:.1}")
    } else if abs >= 1.0 {
        format!("{v:.2}")
    } else if abs == 0.0 {
        "0".to_string()
    } else {
        format!("{v:.3}")
    }
}

/// 0 を含み、上下に少し余白を取ったキリのよい範囲。
fn nice_bounds(min: f64, max: f64) -> (f64, f64) {
    let mut lo = min.min(0.0);
    let mut hi = max.max(0.0);
    if (hi - lo).abs() < f64::EPSILON {
        hi = lo + 1.0;
    }
    let pad = (hi - lo) * 0.08;
    lo -= pad;
    hi += pad;
    (lo, hi)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(at_ms: i64, v: Option<f64>) -> SamplePoint {
        SamplePoint { at_ms, value: v }
    }

    #[test]
    fn csv_has_header_and_blank_for_missing() {
        let cols = vec![
            CsvColumn {
                metric: "srt_drop_rate".into(),
                unit: "%".into(),
                points: vec![pt(1000, None), pt(2000, Some(0.5))],
            },
            CsvColumn {
                metric: "srt_recv_mbps".into(),
                unit: "Mbps".into(),
                points: vec![pt(2000, Some(12.345))],
            },
        ];
        let csv = to_csv(&cols, 540);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "timestamp,srt_drop_rate (%),srt_recv_mbps (Mbps)");
        assert_eq!(lines.len(), 3);
        // 1 行目は欠測の空欄
        let fields: Vec<&str> = lines[1].split(',').collect();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[1], "");
        assert_eq!(fields[2], "");
        // 2 行目は両方値あり
        let fields: Vec<&str> = lines[2].split(',').collect();
        assert_eq!(fields[1], "0.500");
        assert_eq!(fields[2], "12.345");
        assert!(fields[0].contains("+09:00"), "{}", fields[0]);
    }

    #[test]
    fn csv_escapes_metric_names() {
        let cols = vec![CsvColumn {
            metric: "a,b\"c".into(),
            unit: String::new(),
            points: vec![pt(0, Some(1.0))],
        }];
        let csv = to_csv(&cols, 0);
        assert!(csv.starts_with("timestamp,\"a,b\"\"c\"\n"), "{csv}");
    }

    #[test]
    fn svg_contains_paths_and_labels() {
        let series = vec![SvgSeries {
            label: "ドロップ率".into(),
            unit: "%".into(),
            color: "#f59e0b".into(),
            points: vec![
                pt(0, Some(0.0)),
                pt(1000, Some(1.5)),
                pt(2000, None),
                pt(3000, Some(0.2)),
            ],
        }];
        let svg = to_svg("テスト", &series, SvgSize::default(), 540);
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("ドロップ率"));
        assert!(svg.contains("stroke=\"#f59e0b\""));
        assert!(svg.contains("<path d=\"M"), "path が無い");
        // 欠測で線が切れる（M コマンドが 2 本以上）
        let path_start = svg.find("<path d=\"M").expect("<path d=\"M が無い");
        let body = &svg[path_start + "<path d=\"".len()..];
        let d = &body[..body.find('"').unwrap_or(body.len())];
        assert!(
            d.matches('M').count() >= 2,
            "欠測で線が分断されていない: {d}"
        );
        assert_eq!(svg.matches("<svg").count(), 1);
        assert!(svg.ends_with("</svg>\n"));
    }

    #[test]
    fn svg_handles_empty_series() {
        let svg = to_svg("空", &[], SvgSize::default(), 0);
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn svg_escapes_title() {
        let svg = to_svg("<script>&\"", &[], SvgSize::default(), 0);
        assert!(!svg.contains("<script>"));
        assert!(svg.contains("&lt;script&gt;"));
    }

    #[test]
    fn export_format_parse() {
        assert_eq!(ExportFormat::parse("CSV"), Some(ExportFormat::Csv));
        assert_eq!(ExportFormat::parse("svg"), Some(ExportFormat::Svg));
        assert_eq!(ExportFormat::parse("png"), None);
    }
}
