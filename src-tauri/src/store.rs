use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Full set of files we expect inside the recorder store root:
///   rec/<user>/<device>/<YYYY-MM>.rec     per-month TSV location logs
///   waypoints/<user>/<dir>/<file>.otrw    waypoint exports
pub const REC_SUBDIR: &str = "rec";
pub const WAYPOINTS_SUBDIR: &str = "waypoints";

#[derive(Debug, Clone, Serialize)]
pub struct UserEntry {
    pub user: String,
    pub devices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationPoint {
    pub lat: f64,
    pub lon: f64,
    pub tst: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Waypoint {
    pub user: String,
    pub desc: String,
    pub lat: f64,
    pub lon: f64,
    pub rad: f64,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct PointKey {
    pub user: String,
    pub device: String,
    pub from: i64,
    pub to: i64,
}

#[derive(Serialize)]
pub struct BucketsResult {
    pub cells: Vec<HeatmapCell>,
    pub total_points: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeatmapCell {
    pub lat: f64,
    pub lon: f64,
    pub count: u64,
}

/// OwnTracks recorder store access, with a small raw-point cache.
pub struct Store {
    pub root: PathBuf,
    cache: Mutex<HashMap<PointKey, Arc<Vec<LocationPoint>>>>,
    cache_order: Mutex<Vec<PointKey>>,
}

impl Store {
    /// Resolve the recorder store root from `OWNFE_STORE` env, else
    /// `$HOME/owntracks/recorder-store`.
    pub fn open() -> Result<Self, String> {
        let root = match std::env::var_os("OWNFE_STORE") {
            Some(v) => PathBuf::from(v),
            None => {
                let home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
                    .ok_or_else(|| "HOME environment variable not set".to_string())?;
                home.join("owntracks").join("recorder-store")
            }
        };
        let root = root.canonicalize().map_err(|e| {
            format!(
                "cannot open recorder store at {}: {e} (set OWNFE_STORE to the recorder-store dir)",
                root.display()
            )
        })?;
        Ok(Self {
            root,
            cache: Mutex::new(HashMap::new()),
            cache_order: Mutex::new(Vec::new()),
        })
    }

    fn rec_dir(&self) -> PathBuf {
        self.root.join(REC_SUBDIR)
    }

    /// Users with at least one month of recorded data.
    pub fn list_users(&self) -> Result<Vec<UserEntry>, String> {
        let rec = self.rec_dir();
        let entries = fs::read_dir(&rec)
            .map_err(|e| format!("cannot read {}: {e}", rec.display()))?;

        let mut users = Vec::new();
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let user = match entry.file_name().into_string() {
                Ok(u) => u,
                Err(_) => continue,
            };
            let mut devices = Vec::new();
            let device_dir = entry.path();
            if let Ok(devs) = fs::read_dir(&device_dir) {
                for d in devs.flatten() {
                    if !d.path().is_dir() {
                        continue;
                    }
                    if let Some(name) = d.file_name().to_str() {
                        let has_data = fs::read_dir(d.path())
                            .map(|months| {
                                months.flatten().any(|f| {
                                    f.path().is_file()
                                        && f.path()
                                            .extension()
                                            .is_some_and(|e| e == "rec")
                                })
                            })
                            .unwrap_or(false);
                        if has_data {
                            devices.push(name.to_string());
                        }
                    }
                }
            }
            devices.sort();
            if !devices.is_empty() {
                users.push(UserEntry { user, devices });
            }
        }
        users.sort_unstable_by_key(|u| u.user.clone());
        Ok(users)
    }

    /// Waypoint sets stored per user.
    pub fn waypoints_for_user(&self, user: &str) -> Result<Vec<Waypoint>, String> {
        let dir = self.root.join(WAYPOINTS_SUBDIR).join(user);
        let mut out = Vec::new();
        let mut stack = vec![dir];
        while let Some(d) = stack.pop() {
            let Ok(read) = fs::read_dir(&d) else { continue };
            for entry in read.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|e| e == "otrw") {
                    out.extend(parse_waypoint_file(&p, user));
                }
            }
        }
        Ok(out)
    }

    /// Fetch raw location points for a range, filtering through the cache.
    pub fn points(
        &self,
        user: &str,
        device: &str,
        from: i64,
        to: i64,
    ) -> Result<Arc<Vec<LocationPoint>>, String> {
        let key = PointKey {
            user: user.to_string(),
            device: device.to_string(),
            from,
            to,
        };

        {
            let cache = self.cache.lock().unwrap();
            if let Some(p) = cache.get(&key) {
                return Ok(p.clone());
            }
        }

        let points = Arc::new(read_monthly(&self.rec_dir(), user, device, from, to)?);

        let mut cache = self.cache.lock().unwrap();
        let mut order = self.cache_order.lock().unwrap();
        if cache.len() >= 16 {
            if let Some(oldest) = order.first().cloned() {
                cache.remove(&oldest);
                order.remove(0);
            }
        }
        cache.insert(key.clone(), points.clone());
        order.push(key);
        Ok(points)
    }
}

/// Read every month file overlapping [from, to] and keep location rows.
fn read_monthly(
    rec_dir: &Path,
    user: &str,
    device: &str,
    from: i64,
    to: i64,
) -> Result<Vec<LocationPoint>, String> {
    let dir = rec_dir.join(user).join(device);
    let month_files = month_files(&dir)?;

    let mut points = Vec::new();
    for (path, first_e, last_e) in month_files {
        if last_e.unwrap_or(i64::MAX) < from || first_e.unwrap_or(0) > to {
            continue; // month fully outside the requested window
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(p) = parse_rec_line(line, from, to) {
                points.push(p);
            }
        }
    }
    points.sort_unstable_by_key(|p| p.tst);
    Ok(points)
}

/// A `.rec` month file plus the unix-second range its month covers.
type MonthlyFile = (PathBuf, Option<i64>, Option<i64>);

/// List `.rec` files in a device dir, each with the unix second range its
/// month covers (approx, first/last day of month in local time is not
/// needed — filter happens per-point anyway; this just prunes whole files).
fn month_files(dir: &Path) -> Result<Vec<MonthlyFile>, String> {
    let read = fs::read_dir(dir)
        .map_err(|e| format!("cannot read device directory {}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for entry in read.flatten() {
        let p = entry.path();
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if p.extension().is_some_and(|e| e == "rec") {
            // YYYY-MM.rec
            if let Some((y, m)) = stem.split_once('-') {
                if let (Ok(y), Ok(m)) = (y.parse::<i64>(), m.parse::<u32>()) {
                    if (1..=12).contains(&m) {
                        let first = days_from_civil(y, m, 1) * 86_400;
                        let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
                        let next = days_from_civil(ny, nm, 1) * 86_400;
                        out.push((p, Some(first), Some(next)));
                        continue;
                    }
                }
            }
            out.push((p, None, None));
        }
    }
    out.sort_unstable_by_key(|m| m.0.clone());
    Ok(out)
}

/// Parse one line from a `.rec` file: `ISO-TS \t padding \t {json}`.
/// Only `_type: "location"` rows with lat/lon/tst are kept.
fn parse_rec_line(line: &str, from: i64, to: i64) -> Option<LocationPoint> {
    let json = line.rsplit_once('\t').map(|(_, j)| j)?;
    let v: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
    if v.get("_type").and_then(|t| t.as_str()) != Some("location") {
        return None;
    }
    let lat = v.get("lat")?.as_f64()?;
    let lon = v.get("lon")?.as_f64()?;
    let tst = v.get("tst")?.as_i64()?;
    if tst < from || tst > to {
        return None;
    }
    Some(LocationPoint { lat, lon, tst })
}

fn parse_waypoint_file(path: &Path, user: &str) -> Vec<Waypoint> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    // The .otrw files we see are a single `{"_type":"waypoints","waypoints":[...]}`
    // object, but be tolerant of a bare array too.
    let items: Vec<&serde_json::Value> = match v.get("waypoints") {
        Some(arr) => arr.as_array().map(|a| a.iter().collect()).unwrap_or_default(),
        None => v
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default(),
    };
    let mut out = Vec::new();
    for w in items {
        let Some(lat) = w.get("lat").and_then(|l| l.as_f64()) else {
            continue;
        };
        let Some(lon) = w.get("lon").and_then(|l| l.as_f64()) else {
            continue;
        };
        out.push(Waypoint {
            user: user.to_string(),
            desc: w
                .get("desc")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string(),
            lat,
            lon,
            rad: w.get("rad").and_then(|r| r.as_f64()).unwrap_or(0.0),
        });
    }
    let _ = &path;
    out
}

/// Days since Unix epoch for civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) as i64 + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_month_rec(dir: &Path, stem: &str, lines: &[&str]) {
        fs::create_dir_all(dir).unwrap();
        let p = dir.join(format!("{stem}.rec"));
        fs::write(&p, lines.join("\n")).unwrap();
    }

    #[test]
    fn parses_only_location_rows_within_range() {
        assert!(parse_rec_line(
            &format!(
                "2026-09-01T00:00:43Z\t*\t{}",
                serde_json::json!({
                    "_type": "location",
                    "lat": 44.585,
                    "lon": -123.24,
                    "tst": 1000,
                })
            ),
            0,
            2000,
        )
        .is_some());

        // Out of the requested window -> skipped.
        assert!(parse_rec_line(
            &format!(
                "2026-09-01T00:00:43Z\t*\t{}",
                serde_json::json!({
                    "_type": "location",
                    "lat": 44.585,
                    "lon": -123.24,
                    "tst": 9999,
                })
            ),
            0,
            2000,
        )
        .is_none());

        // Non-location rows -> skipped.
        assert!(parse_rec_line(
            &format!(
                "2026-09-01T00:00:43Z\t*\t{}",
                serde_json::json!({ "_type": "transition", "tst": 1000 })
            ),
            0,
            2000,
        )
        .is_none());
    }

    #[test]
    fn reads_devices_with_months() {
        let tmp = std::env::temp_dir().join(format!("ownt-fe-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let store = Store {
            root: tmp.clone(),
            cache: Mutex::new(HashMap::new()),
            cache_order: Mutex::new(Vec::new()),
        };

        write_month_rec(
            &tmp.join("rec").join("alice").join("phone"),
            "1970-01",
            &["1970-01-01T00:00:01Z\t*\t{\"_type\":\"location\",\"lat\":1.0,\"lon\":2.0,\"tst\":10}"],
        );
        write_month_rec(
            &tmp.join("rec").join("alice").join("tablet"),
            "1969-12",
            &["1969-12-01T00:00:01Z\t*\t{\"_type\":\"location\",\"lat\":1.0,\"lon\":2.0,\"tst\":9}"],
        );
        fs::create_dir_all(tmp.join("rec").join("alice").join("broken")).unwrap();
        fs::create_dir_all(tmp.join("rec").join("no-device")).unwrap();

        let users = store.list_users().unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].user, "alice");
        assert_eq!(users[0].devices, vec!["phone", "tablet"]);

        // Query that misses the month entirely -> no points, no error.
        let empty = store.points("alice", "phone", 0, 5).unwrap();
        assert!(empty.is_empty());

        let got = store.points("alice", "phone", 5, 100).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].tst, 10);

        // Second fetch comes from cache (no re-read).
        let again = store.points("alice", "phone", 5, 100).unwrap();
        assert_eq!(again[0].tst, 10);

        let _ = fs::remove_dir_all(&tmp);
    }

    /// End-to-end smoke test against the real recorder store. Requires
    /// `OWNFE_STORE` to point at a recorder-store, or the default
    /// `~/owntracks/recorder-store` to exist. Run with:
    ///   OWNFE_STORE=~/owntracks/recorder-store cargo test -- --ignored smoke_real_store
    #[test]
    #[ignore]
    fn smoke_real_store() {
        use crate::heatmap;
        let store = Store::open().unwrap();
        eprintln!("store root: {}", store.root.display());
        let users = store.list_users().unwrap();
        assert!(!users.is_empty(), "expected at least one user");
        for u in &users {
            eprintln!("  user={} devices={:?}", u.user, u.devices);
            for d in &u.devices {
                let n = store.points(&u.user, d, 0, i64::MAX).unwrap().len();
                eprintln!("    device={d} points={n}");
            }
        }
        // Replicate the frontend defaults: 30-day window ending now, zoom 12.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let from = now - 30 * 86400;
        if let Some(first) = users.first() {
            if let Some(dev) = first.devices.first() {
                let pts = store.points(&first.user, dev, from, now).unwrap();
                let cells = heatmap::bucket(&pts, heatmap::precision_for_zoom(12));
                eprintln!(
                    "  range check user={} device={dev} from={from} to={now} raw={} cells={}",
                    first.user,
                    pts.len(),
                    cells.len()
                );
                assert!(!pts.is_empty(), "expected raw points in range window");
                assert!(!cells.is_empty(), "expected heat cells from range window");
            }
        }
    }
}