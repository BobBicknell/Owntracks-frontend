/// <reference types="vite/client" />
import { invoke } from "@tauri-apps/api/core";
import L from "leaflet";
import "leaflet/dist/leaflet.css";
import "leaflet.heat";
import "./styles.css";
import { attachDatePicker } from "./datepicker";

interface UserEntry {
  user: string;
  devices: string[];
}

interface Waypoint {
  user: string;
  desc: string;
  lat: number;
  lon: number;
  rad: number;
}

interface HeatmapCell {
  lat: number;
  lon: number;
  count: number;
}

interface BucketsResult {
  cells: HeatmapCell[];
  total_points: number;
}

interface LocationPoint {
  lat: number;
  lon: number;
  tst: number;
}

const DEFAULT_ZOOM = 12;

// ---- Map setup -------------------------------------------------------------

const map = L.map("map", {
  center: [44.59, -123.26],
  zoom: DEFAULT_ZOOM,
  zoomControl: true,
  attributionControl: true,
});

L.tileLayer("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png", {
  maxZoom: 19,
  attribution: "&copy; OpenStreetMap contributors",
}).addTo(map);

let heatLayer: L.HeatLayer | null = null;
let trackLayer: L.Polyline | null = null;
let waypointLayer: L.LayerGroup | null = null;

// ---- Current query state ---------------------------------------------------

let users: UserEntry[] = [];
let waypoints: Waypoint[] = [];
let trackPoints: LocationPoint[] | null = null;
let currentRange: { user: string; device: string; from: number; to: number } | null = null;
let fetchSeq = 0;

// ---- DOM refs --------------------------------------------------------------

const userSelect = document.querySelector<HTMLSelectElement>("#user-select")!;
const deviceSelect = document.querySelector<HTMLSelectElement>("#device-select")!;
const fromDate = document.querySelector<HTMLInputElement>("#from-date")!;
const toDate = document.querySelector<HTMLInputElement>("#to-date")!;
const loadBtn = document.querySelector<HTMLButtonElement>("#load-btn")!;
const heatToggle = document.querySelector<HTMLInputElement>("#heat-toggle")!;
const trackToggle = document.querySelector<HTMLInputElement>("#track-toggle")!;
const waypointToggle = document.querySelector<HTMLInputElement>("#waypoint-toggle")!;
const statusEl = document.querySelector<HTMLSpanElement>("#status")!;

attachDatePicker(fromDate);
attachDatePicker(toDate);

// ---- Helpers ---------------------------------------------------------------

function setStatus(text: string) {
  statusEl.textContent = text;
}

function dateToEpoch(d: string): number {
  // Treat the picker value as a local calendar day, 00:00 local time.
  const [y, m, day] = d.split("-").map(Number);
  return Math.floor(new Date(y, m - 1, day).getTime() / 1000);
}

function trimRange(): { from: number; to: number } | null {
  if (!fromDate.value || !toDate.value) return null;
  const from = dateToEpoch(fromDate.value);
  const to = dateToEpoch(toDate.value) + 86399; // include the whole "to" day
  if (from > to) return null;
  return { from, to };
}

function boundsFromCells(cells: HeatmapCell[]): L.LatLngBounds | null {
  if (cells.length === 0) return null;
  let minLat = Infinity;
  let minLon = Infinity;
  let maxLat = -Infinity;
  let maxLon = -Infinity;
  for (const c of cells) {
    minLat = Math.min(minLat, c.lat);
    minLon = Math.min(minLon, c.lon);
    maxLat = Math.max(maxLat, c.lat);
    maxLon = Math.max(maxLon, c.lon);
  }
  return L.latLngBounds([minLat, minLon], [maxLat, maxLon]);
}

// ---- Controls --------------------------------------------------------------

function populateUsers() {
  userSelect.innerHTML = "";
  for (const u of users) {
    const opt = document.createElement("option");
    opt.value = u.user;
    opt.textContent = u.user;
    userSelect.appendChild(opt);
  }
  if (users.length > 0) populateDevices();
}

function populateDevices() {
  const current = userSelect.value;
  const entry = users.find((u) => u.user === current);
  deviceSelect.innerHTML = "";
  for (const d of entry?.devices ?? []) {
    const opt = document.createElement("option");
    opt.value = d;
    opt.textContent = d;
    deviceSelect.appendChild(opt);
  }
}

async function loadWaypoints(user: string) {
  try {
    waypoints = await invoke<Waypoint[]>("get_waypoints", { user });
  } catch (e) {
    waypoints = [];
    console.warn("waypoints unavailable:", e);
  }
  renderWaypoints();
}

function renderWaypoints() {
  waypointLayer?.clearLayers();
  if (!waypointToggle.checked) return;
  for (const w of waypoints) {
    L.circle([w.lat, w.lon], {
      radius: w.rad,
      color: "#f38ba8",
      weight: 2,
      fillColor: "#f38ba8",
      fillOpacity: 0.15,
    })
      .bindTooltip(w.desc)
      .addTo(waypointLayer!);
  }
}

// ---- Data loading ----------------------------------------------------------

async function loadData() {
  const range = trimRange();
  if (!range) {
    setStatus("Pick a valid date range.");
    return;
  }
  const user = userSelect.value;
  const device = deviceSelect.value;
  if (!user || !device) {
    setStatus("Pick a user and device.");
    return;
  }

  currentRange = { user, device, ...range };
  trackPoints = null;
  const seq = ++fetchSeq;
  loadBtn.disabled = true;
  setStatus("Loading…");

  try {
    const zoom = map.getZoom();
    const buckets = await invoke<BucketsResult>("get_heatmap_buckets", {
      user,
      device,
      from: range.from,
      to: range.to,
      zoom,
    });
    if (seq !== fetchSeq) return;

    const bounds = boundsFromCells(buckets.cells);
    if (bounds) map.fitBounds(bounds, { maxZoom: 15, padding: [24, 24] });
    drawHeat(buckets.cells);
    drawTrack();
    renderWaypoints();

    setStatus(
      `${buckets.total_points.toLocaleString()} points, ` +
        `${buckets.cells.length.toLocaleString()} heat cells (zoom ${zoom})`,
    );
  } catch (e) {
    if (seq === fetchSeq) setStatus(`Error: ${e}`);
  } finally {
    if (seq === fetchSeq) loadBtn.disabled = false;
  }
}

function drawHeat(cells: HeatmapCell[]) {
  heatLayer?.remove();
  heatLayer = null;
  if (!heatToggle.checked) return;

  // Normalize against the 90th percentile count (cells are sorted desc) so
  // the ~10% densest cells saturate while the rest span the full gradient.
  // Max-based scaling washes everything out when one cell (home, work) dwarfs
  // the rest.
  const latlngs: Array<[number, number, number]> = [];
  const hot = cells.reduce((m, c) => Math.max(m, c.count), 1);
  const p90 =
    cells.length > 1
      ? Math.max(1, cells[Math.floor(cells.length * 0.1)].count)
      : hot;
  const scale = Math.log(p90 + 1) || 1;
  for (const c of cells) {
    const t = Math.min(1, Math.log(c.count + 1) / scale);
    latlngs.push([c.lat, c.lon, 0.2 + 0.8 * t]);
  }
  if (latlngs.length === 0) return;

  const zoom = map.getZoom();
  const radius = Math.max(16, 46 - zoom * 1.5);
  heatLayer = L.heatLayer(latlngs, {
    radius,
    blur: radius * 0.55,
    minOpacity: 0.15,
    gradient: {
      0.0: "#0000cd",
      0.3: "#00bfff",
      0.5: "#00ff7f",
      0.7: "#ffff00",
      1.0: "#ff0000",
    },
  }).addTo(map);
}

async function ensureTrack(): Promise<LocationPoint[] | null> {
  if (!currentRange) return null;
  if (!trackPoints) {
    try {
      trackPoints = await invoke<LocationPoint[]>("get_track", {
        user: currentRange.user,
        device: currentRange.device,
        from: currentRange.from,
        to: currentRange.to,
      });
    } catch (e) {
      setStatus(`Error: ${e}`);
      return null;
    }
  }
  return trackPoints;
}

// Approximate ground distance in meters (equirectangular, fine for this scale).
function metersBetween(
  a: { lat: number; lon: number },
  b: { lat: number; lon: number },
): number {
  const latMid = ((a.lat + b.lat) / 2) * (Math.PI / 180);
  const dx = (b.lon - a.lon) * Math.cos(latMid) * 111_320;
  const dy = (b.lat - a.lat) * 110_540;
  return Math.hypot(dx, dy);
}

// Decimate points closer than `minMeters` apart, then apply two passes of a
// centered moving average. Removes GPS jitter (the "jagged" look) while
// keeping the overall path faithful.
function smoothTrack(raw: LocationPoint[]): Array<{ lat: number; lon: number }> {
  const minMeters = 12;
  const maxPoints = 6000;

  let pts: Array<{ lat: number; lon: number }> = [];
  for (const p of raw) {
    if (pts.length === 0 || metersBetween(pts[pts.length - 1], p) >= minMeters) {
      pts.push({ lat: p.lat, lon: p.lon });
    }
  }
  if (pts.length > maxPoints) {
    const step = Math.ceil(pts.length / maxPoints);
    pts = pts.filter((_, i) => i % step === 0);
  }

  for (let pass = 0; pass < 2; pass++) {
    const smoothed = pts.map((p, i) => {
      if (i === 0 || i === pts.length - 1) return p;
      const a = pts[i - 1];
      const c = pts[i + 1];
      return { lat: (a.lat + p.lat * 2 + c.lat) / 4, lon: (a.lon + p.lon * 2 + c.lon) / 4 };
    });
    pts = smoothed;
  }
  return pts;
}

async function drawTrack() {
  trackLayer?.remove();
  trackLayer = null;
  if (!trackToggle.checked) return;

  const raw = await ensureTrack();
  if (!raw || raw.length < 2) return;

  const pts = smoothTrack(raw);
  const latlngs = pts.map((p) => L.latLng(p.lat, p.lon));
  trackLayer = L.polyline(latlngs, {
    color: "#89b4fa",
    weight: 3,
    opacity: 0.9,
    lineJoin: "round",
    lineCap: "round",
  }).addTo(map);
}

// ---- Event wiring ----------------------------------------------------------

let rebucketTimer: number | undefined;
map.on("zoomend", () => {
  if (!currentRange) return;
  window.clearTimeout(rebucketTimer);
  rebucketTimer = window.setTimeout(async () => {
    try {
      const result = await invoke<BucketsResult>("get_heatmap_buckets", {
        user: currentRange!.user,
        device: currentRange!.device,
        from: currentRange!.from,
        to: currentRange!.to,
        zoom: map.getZoom(),
      });
      drawHeat(result.cells);
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  }, 250);
});

userSelect.addEventListener("change", () => {
  populateDevices();
  loadWaypoints(userSelect.value);
});
deviceSelect.addEventListener("change", () => {
  trackPoints = null;
  waypointLayer?.clearLayers();
});

heatToggle.addEventListener("change", () => {
  if (heatToggle.checked && currentRange) drawHeatState();
  else heatLayer?.remove();
});
trackToggle.addEventListener("change", () => {
  if (trackToggle.checked && currentRange) {
    drawHeatState();
    drawTrack();
  } else {
    trackLayer?.remove();
  }
});
waypointToggle.addEventListener("change", renderWaypoints);
loadBtn.addEventListener("click", loadData);

// Redraw the existing heat from cached cells on zoombuckets already handled;
// this only helps re-adding the layer after a toggle.
async function drawHeatState() {
  if (!currentRange) return;
  try {
    const result = await invoke<BucketsResult>("get_heatmap_buckets", {
      user: currentRange.user,
      device: currentRange.device,
      from: currentRange.from,
      to: currentRange.to,
      zoom: map.getZoom(),
    });
    drawHeat(result.cells);
  } catch (e) {
    setStatus(`Error: ${e}`);
  }
}

// ---- Init -------------------------------------------------------------------

async function init() {
  waypointLayer = L.layerGroup().addTo(map);
  map.attributionControl.setPosition("bottomright");

  const now = new Date();
  const ago = new Date(now.getTime() - 30 * 24 * 3600 * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  toDate.value = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  fromDate.value = `${ago.getFullYear()}-${pad(ago.getMonth() + 1)}-${pad(ago.getDate())}`;

  try {
    users = await invoke<UserEntry[]>("list_users");
  } catch (e) {
    setStatus(`Could not read recorder store: ${e}`);
    return;
  }
  if (users.length === 0) {
    setStatus("No recorded data found in the store.");
    return;
  }
  populateUsers();
  loadWaypoints(userSelect.value);
  setStatus("Ready — load data for the default range…");
  loadData();
}

init();