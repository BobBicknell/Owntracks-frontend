use std::collections::HashMap;

use geohash::{encode, Coord};

use crate::store::{HeatmapCell, LocationPoint};

/// Map a Leaflet zoom level to a geohash precision. Tuned coarsely:
///   zoom 0-4  (region) -> 3  (~156km)
///   zoom 5-7  (state)  -> 4  (~39km)
///   zoom 8-9  (metro)  -> 5  (~4.9km)
///   zoom 10-11 (city)  -> 6  (~1.2km)
///   zoom 12-13         -> 7  (~152m)
///   zoom 14+  (street) -> 8  (~38m)
pub fn precision_for_zoom(zoom: u8) -> u8 {
    match zoom {
        0..=4 => 3,
        5..=7 => 4,
        8..=9 => 5,
        10..=11 => 6,
        12..=13 => 7,
        _ => 8,
    }
}

/// Bucket raw points by geohash cell, returning one aggregated cell per
/// unique geohash with its weighted centroid and point count.
pub fn bucket(points: &[LocationPoint], precision: u8) -> Vec<HeatmapCell> {
    let mut buckets: HashMap<String, BucketAgg> = HashMap::new();
    let len = precision as usize;

    for p in points {
        if let Ok(hash) = encode(Coord { x: p.lon, y: p.lat }, len) {
            let agg = buckets.entry(hash).or_insert_with(|| BucketAgg {
                sum_lat: 0.0,
                sum_sin: 0.0,
                sum_cos: 0.0,
                count: 0,
            });
            let lon_rad = p.lon.to_radians();
            agg.sum_lat += p.lat;
            agg.sum_sin += lon_rad.sin();
            agg.sum_cos += lon_rad.cos();
            agg.count += 1;
        }
    }

    let mut cells: Vec<HeatmapCell> = buckets
        .into_values()
        .map(|agg| {
            let count = agg.count;
            HeatmapCell {
                lat: agg.sum_lat / count as f64,
                lon: f64::atan2(agg.sum_sin, agg.sum_cos).to_degrees(),
                count,
            }
        })
        .collect();
    cells.sort_unstable_by_key(|c| std::cmp::Reverse(c.count));
    cells
}

struct BucketAgg {
    sum_lat: f64,
    sum_sin: f64,
    sum_cos: f64,
    count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(lat: f64, lon: f64, tst: i64) -> LocationPoint {
        LocationPoint { lat, lon, tst }
    }

    #[test]
    fn precision_mapping_is_monotonic() {
        for z in 0..=19 {
            assert!((1..=12).contains(&precision_for_zoom(z)), "zoom {z}");
        }
        // Higher zoom should never produce coarser cells.
        assert!(precision_for_zoom(10) >= precision_for_zoom(5));
        assert!(precision_for_zoom(15) >= precision_for_zoom(12));
    }

    #[test]
    fn buckets_aggregate_counts_and_means() {
        // Two clusters a few km apart at precision 5 (~4.9km cells):
        // points inside a single cell collapse to one cell.
        let points = vec![
            pt(44.5853, -123.2415, 1000),
            pt(44.5855, -123.2418, 1001),
            pt(44.5856, -123.2420, 1002),
            pt(44.6000, -123.3000, 1003), // separate cell
        ];
        let cells = bucket(&points, 5);
        assert_eq!(cells.len(), 2);
        let mut total: u64 = 0;
        for c in &cells {
            total += c.count;
        }
        assert_eq!(total, 4);

        let dense = cells.iter().find(|c| c.count == 3).unwrap();
        let mean_lat = (44.5853 + 44.5855 + 44.5856) / 3.0;
        assert!((dense.lat - mean_lat).abs() < 1e-9);
    }

    #[test]
    fn coarser_precision_merges_more() {
        let points = vec![
            pt(44.5853, -123.2415, 1000),
            pt(44.6000, -123.3000, 1001),
            pt(44.6200, -123.3400, 1002),
        ];
        let coarse = bucket(&points, 3).len();
        let fine = bucket(&points, 7).len();
        assert!(fine >= coarse);
    }
}