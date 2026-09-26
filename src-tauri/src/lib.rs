mod heatmap;
mod store;

use std::sync::Arc;

use store::{BucketsResult, Store, UserEntry, Waypoint};

#[tauri::command]
fn list_users(store: tauri::State<Arc<Store>>) -> Result<Vec<UserEntry>, String> {
    store.list_users()
}

#[tauri::command]
fn get_waypoints(
    store: tauri::State<Arc<Store>>,
    user: String,
) -> Result<Vec<Waypoint>, String> {
    store.waypoints_for_user(&user)
}

/// Load raw points (through the store's cache) and bucket them into
/// heatmap cells for the given map zoom level.
#[tauri::command]
fn get_heatmap_buckets(
    store: tauri::State<Arc<Store>>,
    user: String,
    device: String,
    from: i64,
    to: i64,
    zoom: u8,
) -> Result<BucketsResult, String> {
    let points: Arc<Vec<store::LocationPoint>> =
        store.points(&user, &device, from, to)?;
    let precision = heatmap::precision_for_zoom(zoom);
    let result = BucketsResult {
        cells: heatmap::bucket(&points, precision),
        total_points: points.len() as u64,
    };
    Ok(result)
}

/// Raw points for track drawing (stride-thinned by the frontend as needed).
#[tauri::command]
fn get_track(
    store: tauri::State<Arc<Store>>,
    user: String,
    device: String,
    from: i64,
    to: i64,
) -> Result<Vec<store::LocationPoint>, String> {
    let points = store.points(&user, &device, from, to)?;
    Ok(points.as_ref().clone())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let store = match Store::open() {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .manage(store)
        .invoke_handler(tauri::generate_handler![
            list_users,
            get_waypoints,
            get_heatmap_buckets,
            get_track,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}