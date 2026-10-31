use std::collections::HashMap;

/// 1 本のサンプル。
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub name: String,
    pub labels: HashMap<String, String>,
    pub value: f64,
}

impl Sample {
    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels.get(key).map(String::as_str)
    }
}

/// `/metrics` 全体。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metrics {
    by_name: HashMap<String, Vec<Sample>>,
}

impl Metrics {
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
    pub fn sample_count(&self) -> usize {
        self.by_name.values().map(Vec::len).sum()
    }
    pub fn get(&self, name: &str) -> &[Sample] {
        self.by_name.get(name).map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn value(&self, name: &str, labels: &[(&str, &str)]) -> Option<f64> {
        self.get(name)
            .iter()
            .find(|s| {
                labels
                    .iter()
                    .all(|(k, v)| s.labels.get(*k).map(String::as_str) == Some(*v))
            })
            .map(|s| s.value)
    }
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.by_name.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }
    pub fn srt_conn_series(&self) -> HashMap<String, HashMap<String, f64>> {
        let mut out: HashMap<String, HashMap<String, f64>> = HashMap::new();
        for (name, samples) in &self.by_name {
            let Some(suffix) = name.strip_prefix("srt_conns_") else {
                continue;
            };
            for s in samples {
                if let Some(id) = s.label("id").filter(|id| !id.is_empty()) {
                    out.entry(id.to_string())
                        .or_default()
                        .insert(suffix.to_string(), s.value);
                }
            }
        }
        out
    }
}

pub fn parse(input: &str) -> Metrics {
    let mut by_name: HashMap<String, Vec<Sample>> = HashMap::new();
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(sample) = parse_line(line) {
            by_name.entry(sample.name.clone()).or_default().push(sample);
        }
    }
    Metrics { by_name }
}

fn parse_line(line: &str) -> Option<Sample> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && !matches!(bytes[i], b'{' | b' ' | b'\t') {
        i += 1;
    }
    let name = &line[..i];
    if name.is_empty() {
        return None;
    }
    let mut labels = HashMap::new();
    if i < bytes.len() && bytes[i] == b'{' {
        let (parsed, next) = parse_labels(line, i)?;
        labels = parsed;
        i = next;
    }
    let rest = line[i..].trim_start();
    let value_token = rest.split_whitespace().next()?;
    let value = parse_value(value_token)?;
    Some(Sample {
        name: name.to_string(),
        labels,
        value,
    })
}

fn parse_value(token: &str) -> Option<f64> {
    match token {
        "+Inf" | "Inf" => Some(f64::INFINITY),
        "-Inf" => Some(f64::NEG_INFINITY),
        "NaN" => Some(f64::NAN),
        _ => token.parse::<f64>().ok(),
    }
}

fn parse_labels(line: &str, open: usize) -> Option<(HashMap<String, String>, usize)> {
    let mut labels = HashMap::new();
    let bytes = line.as_bytes();
    let mut i = open + 1;
    loop {
        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b',') {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        if bytes[i] == b'}' {
            return Some((labels, i + 1));
        }
        let key_start = i;
        while i < bytes.len() && bytes[i] != b'=' {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let key = line[key_start..i].trim().to_string();
        i += 1;
        if i >= bytes.len() || bytes[i] != b'"' {
            return None;
        }
        i += 1;
        let mut value = String::new();
        while i < bytes.len() {
            match bytes[i] {
                b'\\' if i + 1 < bytes.len() => {
                    let next = bytes[i + 1];
                    match next {
                        b'n' => value.push('\n'),
                        b'"' => value.push('"'),
                        b'\\' => value.push('\\'),
                        other => {
                            value.push('\\');
                            value.push(other as char);
                        }
                    }
                    i += 2;
                }
                b'"' => {
                    i += 1;
                    labels.insert(key, value);
                    break;
                }
                _ => {
                    let ch = line[i..].chars().next()?;
                    value.push(ch);
                    i += ch.len_utf8();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# Paths
paths{name="cam1",state="ready"} 1
paths_readers{name="cam1",state="ready",readerType="hlsSession"} 2
paths_inbound_bytes{name="cam1",state="ready"} 123456789
# The remoteAddr label is deprecated.
srt_conns{id="6f1d",state="read",path="cam1",remoteAddr="192.168.0.10:51000"} 1
srt_conns_ms_rtt{id="6f1d",state="read",path="cam1",remoteAddr="192.168.0.10:51000"} 7.25e0
srt_conns_packets_received_retrans{id="6f1d",state="read",path="cam1",remoteAddr="192.168.0.10:51000"} 42
srt_conns_packets_send_loss{id="",state="",path="",remoteAddr=""} 0
"#;

    #[test]
    fn parses_mediamtx_shape() {
        let m = parse(SAMPLE);
        assert_eq!(m.value("paths", &[("name", "cam1")]), Some(1.0));
        assert_eq!(
            m.value(
                "paths_readers",
                &[("name", "cam1"), ("readerType", "hlsSession")]
            ),
            Some(2.0)
        );
        assert_eq!(m.value("srt_conns_ms_rtt", &[("id", "6f1d")]), Some(7.25));
    }

    #[test]
    fn srt_series_groups_by_id() {
        let m = parse(SAMPLE);
        let series = m.srt_conn_series();
        let conn = series.get("6f1d").expect("conn");
        assert_eq!(conn.get("ms_rtt"), Some(&7.25));
        assert_eq!(conn.get("packets_received_retrans"), Some(&42.0));
        assert!(!series.contains_key(""));
    }

    #[test]
    fn handles_escaped_labels() {
        let m = parse(r#"weird{a="he said \"hi\"",b="back\\slash",c="line\nbreak"} 3"#);
        let s = &m.get("weird")[0];
        assert_eq!(s.label("a"), Some(r#"he said "hi""#));
        assert_eq!(s.label("b"), Some(r"back\slash"));
        assert_eq!(s.label("c"), Some("line\nbreak"));
    }

    #[test]
    fn handles_unicode_and_no_labels() {
        let m = parse("uptime_seconds 3600.5\nカメラ{path=\"カメラ1\"} 2\n");
        assert_eq!(m.value("uptime_seconds", &[]), Some(3600.5));
        assert_eq!(m.value("カメラ", &[("path", "カメラ1")]), Some(2.0));
    }

    #[test]
    fn skips_garbage_lines() {
        let m = parse("not a metric line\n\n{broken=true} 1\nok 1\n#comment\n");
        assert_eq!(m.get("ok").len(), 1);
        assert_eq!(m.sample_count(), 1);
    }

    #[test]
    fn special_values() {
        let m = parse("a +Inf\nb -Inf\nc NaN\nd 1 1698765432000\n");
        assert!(m.value("a", &[]).unwrap().is_infinite());
        assert!(m.value("b", &[]).unwrap() < 0.0);
        assert!(m.value("c", &[]).unwrap().is_nan());
        assert_eq!(m.value("d", &[]), Some(1.0));
    }

    #[test]
    fn label_order_independent() {
        let m = parse("x{b=\"2\",a=\"1\"} 1\n");
        assert_eq!(m.value("x", &[("a", "1"), ("b", "2")]), Some(1.0));
    }
}
