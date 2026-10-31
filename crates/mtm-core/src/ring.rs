//! 固定容量の時系列リングバッファ。
//!
//! 監視対象ごとに「タイムスタンプ + 値」を保持する。値は `Option<f64>` で、未取得
//! 取得できていないサンプル（メトリクス無効、初回差分など）は `None`。
//! `None` を 0 として表示しないことが、SRT 品質評価では重要になる。

use std::collections::VecDeque;

/// 1 本の系列。
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    capacity: usize,
    /// 古い順。
    values: VecDeque<SamplePoint>,
}

/// 系列の 1 点。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SamplePoint {
    /// Unix ミリ秒（取得時刻）。
    pub at_ms: i64,
    pub value: Option<f64>,
}

impl Ring {
    /// 容量 0 は 1 として扱う。
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            values: VecDeque::with_capacity(capacity.max(1)),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn last(&self) -> Option<SamplePoint> {
        self.values.back().copied()
    }

    pub fn first(&self) -> Option<SamplePoint> {
        self.values.front().copied()
    }

    /// 追記する。容量超過時は最古を捨てる。
    pub fn push(&mut self, at_ms: i64, value: Option<f64>) {
        if self.values.len() == self.capacity {
            self.values.pop_front();
        }
        self.values.push_back(SamplePoint { at_ms, value });
    }

    /// 容量を変更する。縮小時は古い側から捨てる。
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity.max(1);
        while self.values.len() > self.capacity {
            self.values.pop_front();
        }
    }

    /// 時刻範囲（両端含む）で切り出す。
    pub fn range(&self, from_ms: Option<i64>, to_ms: Option<i64>) -> Vec<SamplePoint> {
        self.values
            .iter()
            .copied()
            .filter(|p| match (from_ms, to_ms) {
                (Some(f), Some(t)) => p.at_ms >= f && p.at_ms <= t,
                (Some(f), None) => p.at_ms >= f,
                (None, Some(t)) => p.at_ms <= t,
                (None, None) => true,
            })
            .collect()
    }

    /// 直近 `duration_ms` の点を返す。
    pub fn recent(&self, now_ms: i64, duration_ms: i64) -> Vec<SamplePoint> {
        self.range(Some(now_ms - duration_ms), None)
    }

    /// 有効値だけの平均（表示用）。
    pub fn mean(&self, window: Option<usize>) -> Option<f64> {
        let iter: Box<dyn Iterator<Item = &SamplePoint>> = match window {
            Some(n) => Box::new(self.values.iter().skip(self.values.len().saturating_sub(n))),
            None => Box::new(self.values.iter()),
        };
        let mut sum = 0.0;
        let mut count = 0usize;
        for p in iter {
            if let Some(v) = p.value.filter(|v| v.is_finite()) {
                sum += v;
                count += 1;
            }
        }
        if count == 0 {
            None
        } else {
            Some(sum / count as f64)
        }
    }

    /// 表示用に点数を間引く（`max_points` を超えないよう、等間隔に間引く）。
    /// 末尾（最新）は必ず残す。
    pub fn downsample(
        &self,
        max_points: usize,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Vec<SamplePoint> {
        let pts = self.range(from_ms, to_ms);
        let max_points = max_points.max(2);
        if pts.len() <= max_points {
            return pts;
        }
        let step = pts.len() as f64 / (max_points - 1) as f64;
        let mut out = Vec::with_capacity(max_points);
        let mut idx = 0.0_f64;
        while (idx as usize) < pts.len() {
            out.push(pts[idx as usize]);
            idx += step;
        }
        if let Some(last) = pts.last().copied() {
            if out.last().map(|p| p.at_ms) != Some(last.at_ms) {
                out.push(last);
            }
        }
        out
    }
}

/// 系列の集合（メトリクス名 → リング）。
#[derive(Debug, Clone, Default)]
pub struct SeriesSet {
    capacity: usize,
    rings: std::collections::BTreeMap<String, Ring>,
}

impl SeriesSet {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            rings: std::collections::BTreeMap::new(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn push(&mut self, metric: &str, at_ms: i64, value: Option<f64>) {
        let capacity = self.capacity;
        self.rings
            .entry(metric.to_string())
            .or_insert_with(|| Ring::new(capacity))
            .push(at_ms, value);
    }

    pub fn get(&self, metric: &str) -> Option<&Ring> {
        self.rings.get(metric)
    }

    pub fn metrics(&self) -> impl Iterator<Item = &str> {
        self.rings.keys().map(String::as_str)
    }

    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity.max(1);
        for ring in self.rings.values_mut() {
            ring.set_capacity(self.capacity);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rings.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rings.len()
    }

    /// 全メトリクスのサンプル数合計（状態表示用）。
    pub fn sample_count(&self) -> usize {
        self.rings.values().map(Ring::len).sum()
    }

    pub fn range(
        &self,
        metric: &str,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Vec<SamplePoint> {
        self.get(metric)
            .map(|r| r.range(from_ms, to_ms))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_oldest_beyond_capacity() {
        let mut r = Ring::new(3);
        for i in 0..5 {
            r.push(i * 1000, Some(i as f64));
        }
        assert_eq!(r.len(), 3);
        assert_eq!(r.first().unwrap().at_ms, 2000);
        assert_eq!(r.last().unwrap().value, Some(4.0));
    }

    #[test]
    fn keeps_none_distinct_from_zero() {
        let mut r = Ring::new(4);
        r.push(1, None);
        r.push(2, Some(0.0));
        assert_eq!(r.first().unwrap().value, None);
        assert_eq!(r.last().unwrap().value, Some(0.0));
    }

    #[test]
    fn range_is_inclusive() {
        let mut r = Ring::new(10);
        for i in 0..10 {
            r.push(i * 100, Some(i as f64));
        }
        let pts = r.range(Some(300), Some(500));
        assert_eq!(pts.len(), 3);
        assert_eq!(pts.first().unwrap().at_ms, 300);
        assert_eq!(pts.last().unwrap().at_ms, 500);
    }

    #[test]
    fn mean_ignores_non_finite() {
        let mut r = Ring::new(10);
        r.push(1, Some(1.0));
        r.push(2, None);
        r.push(3, Some(f64::NAN));
        r.push(4, Some(3.0));
        assert_eq!(r.mean(None), Some(2.0));
        assert_eq!(r.mean(Some(1)), Some(3.0));
    }

    #[test]
    fn downsample_keeps_last_and_bounds() {
        let mut r = Ring::new(1000);
        for i in 0..1000 {
            r.push(i, Some(i as f64));
        }
        let pts = r.downsample(100, None, None);
        assert!(pts.len() <= 101, "len={}", pts.len());
        assert_eq!(pts.last().unwrap().at_ms, 999);
        assert!(pts.windows(2).all(|w| w[0].at_ms < w[1].at_ms));
    }

    #[test]
    fn shrink_capacity_keeps_newest() {
        let mut r = Ring::new(10);
        for i in 0..10 {
            r.push(i, Some(i as f64));
        }
        r.set_capacity(3);
        assert_eq!(r.len(), 3);
        assert_eq!(r.last().unwrap().value, Some(9.0));
        assert_eq!(r.first().unwrap().value, Some(7.0));
    }

    #[test]
    fn series_set_tracks_all_capacity_changes() {
        let mut set = SeriesSet::new(4);
        for i in 0..10 {
            set.push("a", i, Some(i as f64));
        }
        set.set_capacity(2);
        let ring = set.get("a").unwrap();
        assert_eq!(ring.len(), 2);
        assert_eq!(set.metrics().count(), 1);
    }
}
