// Custom date picker that replaces <input type="date">, whose native picker
// freeze in WebKitGTK. Keeps the input value in YYYY-MM-DD so callers read it
// exactly as before.

const MONTHS = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];
const WEEKDAYS = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

function iso(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(
    d.getDate(),
  ).padStart(2, "0")}`;
}

function parseIso(s: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s);
  return m ? new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3])) : null;
}

function calendarIcon(): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", "14");
  svg.setAttribute("height", "14");
  svg.setAttribute("aria-hidden", "true");
  svg.innerHTML = `
    <rect x="3" y="5" width="18" height="16" rx="2" fill="none" stroke="currentColor" stroke-width="2"/>
    <path d="M3 9h18M8 3v4M16 3v4" fill="none" stroke="currentColor" stroke-width="2"/>
  `;
  return svg;
}

export function attachDatePicker(input: HTMLInputElement): void {
  input.type = "text";
  input.readOnly = true;
  input.autocomplete = "off";
  input.classList.add("date-input");

  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "date-btn";
  btn.title = "Pick a date";
  btn.appendChild(calendarIcon());

  const popup = document.createElement("div");
  popup.className = "datepicker-popup";
  popup.hidden = true;
  popup.appendChild(document.createElement("div")); // header placeholder
  popup.appendChild(document.createElement("div")); // grid placeholder
  popup.appendChild(document.createElement("div")); // footer placeholder

  input.after(btn, popup);

  let view = new Date();
  view.setDate(1);

  function render() {
    const header = popup.childNodes[0] as HTMLDivElement;
    header.innerHTML = "";
    header.className = "datepicker-head";
    const prev = document.createElement("button");
    prev.type = "button";
    prev.textContent = "\u2039";
    const label = document.createElement("span");
    label.textContent = `${MONTHS[view.getMonth()]} ${view.getFullYear()}`;
    const next = document.createElement("button");
    next.type = "button";
    next.textContent = "\u203a";
    header.append(prev, label, next);
    prev.onclick = () => {
      view = new Date(view.getFullYear(), view.getMonth() - 1, 1);
      render();
    };
    next.onclick = () => {
      view = new Date(view.getFullYear(), view.getMonth() + 1, 1);
      render();
    };

    const grid = popup.childNodes[1] as HTMLDivElement;
    grid.innerHTML = "";
    grid.className = "datepicker-grid";
    for (const wd of WEEKDAYS) {
      const h = document.createElement("span");
      h.className = "datepicker-wd";
      h.textContent = wd;
      grid.appendChild(h);
    }
    const first = new Date(view.getFullYear(), view.getMonth(), 1);
    const days = new Date(view.getFullYear(), view.getMonth() + 1, 0).getDate();
    const selected = parseIso(input.value);
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    for (let i = 0; i < first.getDay(); i++) {
      grid.appendChild(document.createElement("span"));
    }
    for (let day = 1; day <= days; day++) {
      const cellDate = new Date(view.getFullYear(), view.getMonth(), day);
      const b = document.createElement("button");
      b.type = "button";
      b.className = "datepicker-day";
      b.textContent = String(day);
      if (selected && iso(cellDate) === iso(selected)) b.classList.add("selected");
      if (iso(cellDate) === iso(today)) b.classList.add("today");
      b.onclick = () => select(parseIso(iso(cellDate)));
      grid.appendChild(b);
    }

    const footer = popup.childNodes[2] as HTMLDivElement;
    footer.innerHTML = "";
    footer.className = "datepicker-foot";
    const clear = document.createElement("button");
    clear.type = "button";
    clear.textContent = "Clear";
    const now = document.createElement("button");
    now.type = "button";
    now.textContent = "Today";
    footer.append(clear, now);
    clear.onclick = () => {
      input.value = "";
      input.dispatchEvent(new Event("change", { bubbles: true }));
      close();
    };
    now.onclick = () => select(new Date());
  }

  function select(d: Date | null) {
    if (d) input.value = iso(d);
    input.dispatchEvent(new Event("change", { bubbles: true }));
    close();
  }

  function position() {
    const rect = input.getBoundingClientRect();
    let left = rect.left;
    let top = rect.bottom + 4;
    popup.hidden = false;
    const width = popup.offsetWidth;
    const height = popup.offsetHeight;
    popup.hidden = true;
    if (left + width > window.innerWidth - 8) left = Math.max(8, window.innerWidth - width - 8);
    if (top + height > window.innerHeight - 8) top = Math.max(8, rect.top - height - 4);
    popup.style.left = `${left}px`;
    popup.style.top = `${top}px`;
  }

  function onOutside(e: Event) {
    const t = e.target as Node;
    if (popup.contains(t)) return;
    if (t === input || t === btn) return;
    close();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }

  function open() {
    const opened = parseIso(input.value) || new Date();
    view = new Date(opened.getFullYear(), opened.getMonth(), 1);
    render();
    position();
    popup.hidden = false;
    document.addEventListener("mousedown", onOutside, true);
    document.addEventListener("keydown", onKey, true);
  }

  function close() {
    if (popup.hidden) return;
    popup.hidden = true;
    document.removeEventListener("mousedown", onOutside, true);
    document.removeEventListener("keydown", onKey, true);
  }

  btn.onclick = () => (popup.hidden ? open() : close());
  input.onfocus = () => open();
}