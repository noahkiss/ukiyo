# /// script
# requires-python = ">=3.11"
# dependencies = ["beautifulsoup4>=4.12"]
# ///
"""Build catalog/menus/<model>.json from Fujifilm's HTML owner's manuals.

Usage (from tools/catalog/):
    uv run -q menus_fetch.py [MODEL ...] [--offline] [--refetch]

MODEL is a manual slug such as x-t5 (default: every body in BODIES).
Pages are cached in local/manuals/<slug>/; a rerun reads the cache and
fetches only what is missing. --offline never touches the network;
--refetch ignores the cache.

The output holds structured facts only: menu, page and item names, option
labels, defaults and ranges. It never copies the manual's prose or images.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

from bs4 import BeautifulSoup, NavigableString, Tag

ROOT = Path(__file__).resolve().parents[2]
CACHE = ROOT / "local" / "manuals"
OUT = ROOT / "catalog" / "menus"
BASE = "https://fujifilm-dsc.com/en/manual/"
UA = (
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 "
    "(KHTML, like Gecko) Version/17.0 Safari/605.1.15"
)
DELAY = 1.0  # seconds between requests

# slug -> (model as the camera reports it, sensor generation, sensor, processor)
BODIES: dict[str, tuple[str, str, str, str]] = {
    "x-t5": ("X-T5", "X-Trans V", "X-Trans CMOS 5 HR", "X-Processor 5"),
    "x-t50": ("X-T50", "X-Trans V", "X-Trans CMOS 5 HR", "X-Processor 5"),
    "x-h2": ("X-H2", "X-Trans V", "X-Trans CMOS 5 HR", "X-Processor 5"),
    "x-h2s": ("X-H2S", "X-Trans V", "X-Trans CMOS 5 HS", "X-Processor 5"),
    "x-s20": ("X-S20", "X-Trans IV + X-Processor 5", "X-Trans CMOS 4", "X-Processor 5"),
    "x-e5": ("X-E5", "X-Trans V", "X-Trans CMOS 5 HR", "X-Processor 5"),
    "x-m5": ("X-M5", "X-Trans IV + X-Processor 5", "X-Trans CMOS 4", "X-Processor 5"),
    "x100vi": ("X100VI", "X-Trans V", "X-Trans CMOS 5 HR", "X-Processor 5"),
    "x-t4": ("X-T4", "X-Trans IV", "X-Trans CMOS 4", "X-Processor 4"),
    "x-s10": ("X-S10", "X-Trans IV", "X-Trans CMOS 4", "X-Processor 4"),
    "x-e4": ("X-E4", "X-Trans IV", "X-Trans CMOS 4", "X-Processor 4"),
    "x-pro3": ("X-Pro3", "X-Trans IV", "X-Trans CMOS 4", "X-Processor 4"),
    "x100v": ("X100V", "X-Trans IV", "X-Trans CMOS 4", "X-Processor 4"),
    "x-t30-2": ("X-T30 II", "X-Trans IV", "X-Trans CMOS 4", "X-Processor 4"),
    "x-t3": ("X-T3", "X-Trans IV", "X-Trans CMOS 4", "X-Processor 4"),
    "x-t30": ("X-T30", "X-Trans IV", "X-Trans CMOS 4", "X-Processor 4"),
    "x-h1": ("X-H1", "X-Trans III", "X-Trans CMOS III", "X-Processor Pro"),
    "x-pro2": ("X-Pro2", "X-Trans III", "X-Trans CMOS III", "X-Processor Pro"),
    "x-t2": ("X-T2", "X-Trans III", "X-Trans CMOS III", "X-Processor Pro"),
    "x-e3": ("X-E3", "X-Trans III", "X-Trans CMOS III", "X-Processor Pro"),
    "x100f": ("X100F", "X-Trans III", "X-Trans CMOS III", "X-Processor Pro"),
}

# Manual paths (relative to the model root) that hold menu facts.
MENU_PREFIXES = (
    "introduction/menu_list/",
    "menu_shooting/",
    "menu_setup/",
    "menu_playback/",
    "playback/menu_playback/",
    "connections/network_usb_menu/",
)


# --------------------------------------------------------------------------- fetch


class Fetcher:
    def __init__(self, offline: bool, refetch: bool) -> None:
        self.offline = offline
        self.refetch = refetch
        self.last = 0.0
        self.fetched = 0

    def get(self, url: str, dest: Path) -> bytes | None:
        if dest.exists() and not self.refetch:
            return dest.read_bytes()
        if self.offline:
            return None
        wait = DELAY - (time.monotonic() - self.last)
        if wait > 0:
            time.sleep(wait)
        req = urllib.request.Request(url, headers={"User-Agent": UA})
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                data = r.read()
        except urllib.error.HTTPError as e:
            print(f"  {e.code} {url}", file=sys.stderr)
            data = None
        except urllib.error.URLError as e:
            print(f"  error {url}: {e.reason}", file=sys.stderr)
            data = None
        self.last = time.monotonic()
        self.fetched += 1
        if data is None:
            return None
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
        return data


def rel_path(model_root: str, href: str, page_url: str) -> str | None:
    """Turn a link into a path relative to the model root, or None."""
    url = urllib.parse.urljoin(page_url, href).split("#")[0].split("?")[0]
    if not url.startswith(model_root):
        return None
    rel = url[len(model_root):]
    if rel.endswith("index.html"):
        rel = rel[: -len("index.html")]
    if rel and not rel.endswith("/") and not rel.endswith(".pdf"):
        return None
    return rel


def cache_name(rel: str) -> str:
    if rel.endswith(".pdf"):
        return rel.rsplit("/", 1)[-1]
    return (rel.strip("/").replace("/", "__") or "index") + ".html"


def crawl(slug: str, f: Fetcher) -> dict[str, Path]:
    """Fetch the index, every menu page, and the New Features Guide PDF.

    Returns {relative path: cached file}.
    """
    root = BASE + slug + "/"
    cdir = CACHE / slug
    pages: dict[str, Path] = {}
    queue = [""]
    seen = set(queue)
    while queue:
        rel = queue.pop(0)
        dest = cdir / cache_name(rel)
        data = f.get(root + rel, dest)
        if data is None:
            continue
        pages[rel] = dest
        if rel.endswith(".pdf"):
            continue
        soup = BeautifulSoup(data, "html.parser")
        # Section index pages and the index itself link to menu pages.
        if rel and not (rel in ("menu_shooting/", "menu_setup/", "menu_playback/")):
            continue
        for a in soup.find_all("a", href=True):
            r = rel_path(root, a["href"], root + rel)
            if r is None or r in seen:
                continue
            if r.endswith(".pdf"):
                if "_nfg_" not in r:
                    continue
            elif not r.startswith(MENU_PREFIXES):
                continue
            seen.add(r)
            queue.append(r)
    return pages


# --------------------------------------------------------------------------- text

# Icon-font glyphs the manual uses as menu markers: (class, glyph) -> mode.
MODE_ICONS = {("fficn63", "x"): "stills", ("fficn10", "F"): "movie"}
# Items whose label is only an icon in the menu list.
ICON_LABELS = {"fficn61:Q fficn70:a": "LANG."}
# Glyphs that stand for a word: the L, M and S image sizes.
ICON_TEXT = {("fficn31", "O"): "L", ("fficn31", "P"): "M", ("fficn31", "Q"): "S"}
NUM = re.compile(r"^[+\-−±]?\d+(\.\d+)?$")


def norm(s: str) -> str:
    """Key for matching names across pages: upper case, letters and digits only."""
    return re.sub(r"[^A-Z0-9]", "", s.upper())


def icons(tag: Tag) -> list[tuple[str, str]]:
    out = []
    for sp in tag.find_all("span", class_=re.compile(r"^fficn")):
        cls = next(c for c in sp.get("class", []) if c.startswith("fficn"))
        out.append((cls, sp.get_text()))
    return out


ICON_LEARNED: dict[tuple[str, str], str] = {}


def icon_key(el: Tag) -> tuple[str, str] | None:
    """(font class, glyph) for an icon span, or for the matching icon image in older manuals."""
    if el.name == "span":
        cls = next((c for c in el.get("class", []) if c.startswith("fficn")), None)
        return (cls, el.get_text()) if cls else None
    m = re.search(r"icn_ff(\d+)-([a-z])(2?)_", el.get("src", ""))
    if not m:
        return None
    return ("fficn" + m.group(1), m.group(2).upper() if m.group(3) else m.group(2))


def clean(tag: Tag | NavigableString | None, fill_icons: bool = True) -> str:
    """Visible label text without icon glyphs, images or footnote marks.

    A label that is only icons (older manuals print WB presets as icons) is
    spelled out from the icon names learned from newer manuals.
    """
    if tag is None:
        return ""
    if isinstance(tag, NavigableString):
        return tidy(str(tag))
    text = tidy(_text(tag, False))
    if fill_icons and not re.search(r"[A-Za-z0-9]", text):
        filled = tidy(_text(tag, True))
        if re.search(r"[A-Za-z0-9]", filled) and "\x00" not in filled:
            return filled
    return text


def _text(tag: Tag, learned: bool) -> str:
    t = BeautifulSoup(str(tag), "html.parser")
    for el in t.find_all(["span", "img"]):
        if el.name == "span" and not any(c.startswith("fficn") for c in el.get("class", [])):
            continue
        key = icon_key(el)
        word = ICON_TEXT.get(key) if key else None
        if word is None and learned and key and key not in MODE_ICONS:
            word = ICON_LEARNED.get(key, "\x00")
        if word:
            el.replace_with(word + " ")
        elif el.name == "span" or el.name == "img":
            el.decompose()
    for x in t.find_all(["sup", "script"]):
        x.decompose()
    for br in t.find_all("br"):
        br.replace_with(" ")
    return t.get_text(" ")


def tidy(text: str) -> str:
    text = text.replace("\xa0", " ").replace("−", "-").replace("％", "%")
    text = re.sub(r"\s+", " ", text).strip()
    text = re.sub(r"\s*([/])\s*", r"\1", text) if re.fullmatch(r"[A-Z0-9 ./&+-]+", text or "-") else text
    text = re.sub(r"\(\s+", "(", re.sub(r"\s+\)", ")", text))
    if re.fullmatch(r"\([^()]+\)", text):
        text = text[1:-1].strip()
    return text.lstrip(":; ").strip()


def learn_icons(files: list[Path]) -> dict[tuple[str, str], str]:
    """Icon -> label, from cells that show one icon followed by its label."""
    votes: dict[tuple[str, str], dict[str, int]] = {}
    for f in files:
        soup = BeautifulSoup(f.read_bytes(), "html.parser")
        for cell in soup.find_all(["th", "td", "strong", "dt"]):
            spans = [s for s in cell.find_all("span") if any(c.startswith("fficn") for c in s.get("class", []))]
            if len(spans) != 1 or cell.find(["table", "p", "li"]):
                continue
            key = icon_key(spans[0])
            if key is None or key in MODE_ICONS or key in ICON_TEXT:
                continue
            label = clean(cell, fill_icons=False)
            if not label or len(label) > 30 or not menu_label(label) and not re.search(r"[A-Z]{2}", label):
                continue
            votes.setdefault(key, {}).setdefault(label, 0)
            votes[key][label] += 1
    out = {}
    for key, v in votes.items():
        best, n = max(v.items(), key=lambda kv: kv[1])
        if n / sum(v.values()) >= 0.75:
            out[key] = best
    return out


def skip_container(tag: Tag) -> bool:
    """True inside notes, cautions and side columns, which hold prose, not options."""
    for p in tag.parents:
        cls = p.get("class") or []
        if any(c in ("note", "column", "alert", "footnote", "caution", "memo") for c in cls):
            return True
    return False


def main_content(soup: BeautifulSoup) -> Tag:
    for s in soup.find_all(["script", "style", "noscript"]):
        s.decompose()
    for s in soup.select("#sidebar, .breadcrumb, header, nav, .google-search, .search-note, "
                         "ul.topic-list, .chapter_title, .pure-drawer"):
        s.decompose()
    main = soup.find(attrs={"role": "main"})
    if main:
        return main
    h1 = soup.find("h1")
    if h1:
        for p in h1.parents:
            cls = " ".join(p.get("class") or [])
            if p.name == "div" and re.search(r"col-(xs|sm|md)-(9|12)", cls) and "page-header" not in cls:
                return p
    return soup.body or soup


# --------------------------------------------------------------------------- options


SPAN = re.compile(r"^([+\-]?\d+)\s*[—–~]+\s*([+\-]?\d+)$")


def as_range(opts: list[str]) -> dict | None:
    """Numeric options as min/max/step; "+9 — +1" stands for every step between."""
    vals: list[float] = []
    spans = False
    for o in opts:
        m = SPAN.match(o)
        if m:
            a, b = sorted((int(m.group(1)), int(m.group(2))))
            vals += [float(v) for v in range(a, b + 1)]
            spans = True
        elif NUM.match(o):
            vals.append(float(o.replace("±", "")))
        else:
            return None
    if len(vals) < 3 or (len(opts) < 3 and not spans):
        return None
    vals = sorted(set(vals))
    vals_sorted = sorted(vals)
    steps = {round(b - a, 3) for a, b in zip(vals_sorted, vals_sorted[1:])}
    if len(steps) != 1:
        return None
    step = steps.pop()
    n = lambda v: int(v) if float(v).is_integer() else v  # noqa: E731
    return {"min": n(vals_sorted[0]), "max": n(vals_sorted[-1]), "step": n(step)}


OPTION_HEADS = ("option", "setting", "mode", "item", "value")


def grid_rows(rows: list[Tag]) -> list[list[tuple[int, Tag]]]:
    """Each row's cells with the column they start in, resolving rowspan and colspan."""
    taken: dict[tuple[int, int], bool] = {}
    out = []
    for r, tr in enumerate(rows):
        col = 0
        cells = []
        for cell in tr.find_all(["th", "td"], recursive=False):
            while taken.get((r, col)):
                col += 1
            rs = int(cell.get("rowspan", 1) or 1)
            cs = int(cell.get("colspan", 1) or 1)
            for dr in range(rs):
                for dc in range(cs):
                    taken[(r + dr, col + dc)] = True
            cells.append((col, cell))
            col += cs
        out.append(cells)
    return out


RANGE_CELL = re.compile(r"^([+\-]?\d+(?:\.\d+)?)\s*(?:to|-|–)\s*([+\-]?\d+(?:\.\d+)?)\s*([A-Za-z%]+)?$")
PROSE_RANGE = re.compile(
    r"(?:values? (?:of )?|range )?from (?:values of )?(?:from )?([+\-−]?\d[\d,]*(?:\.\d+)?)\s*(K|EV|%|sec\.|mm)?\b"
    r" (?:to|and) ([+\-−]?\d[\d,]*(?:\.\d+)?)\s*(K|EV|%|sec\.|mm)?\b"
)


def num(v: str) -> int | float:
    f = float(v.replace(",", "").replace("−", "-"))
    return int(f) if f.is_integer() else f


def table_options(table: Tag) -> tuple[list[str], str | None]:
    """Option labels from an option table, and the header of the option column.

    Rows whose description is only a range ("-2 to +4") are recorded in
    table.ukiyo_ranges as {option: range}.
    """
    ranges: dict[str, dict] = {}
    table.ukiyo_ranges = ranges
    rows = [tr for tr in table.find_all("tr") if tr.find_parent("table") is table]
    if not rows:
        return [], None
    thead = table.find("thead")
    if thead is not None:
        head_rows = [r for r in rows if r.find_parent("thead") is thead]
    elif len(rows) > 1 and all(c.name == "th" for c in rows[0].find_all(["th", "td"], recursive=False)):
        head_rows = rows[:1]
    else:
        return [], None
    body = [r for r in rows if r not in head_rows]
    if not head_rows or not body:
        return [], None
    head_cells = grid_rows(head_rows[:1])[0]
    labels = []
    for col, cell in head_cells:
        cs = int(cell.get("colspan", 1) or 1)
        labels.append((col, cs, clean(cell)))
    opt_cols: set[int] = set()
    for col, cs, lab in labels:
        if lab.lower().startswith(OPTION_HEADS):
            opt_cols.update(range(col, col + cs))
    head_label = next((lab for _, _, lab in labels if lab.lower().startswith(OPTION_HEADS)), None)
    if not opt_cols:
        return [], None
    out: list[str] = []
    for cells in grid_rows(body):
        row_opts = []
        for col, cell in cells:
            if col in opt_cols:
                strong = cell.find("strong")
                label = clean(cell)
                if strong and clean(strong) and len(label) > 40:
                    label = clean(strong)
                icon_only = label and not re.search(r"[A-Za-z0-9]", clean(cell, fill_icons=False))
                parts = [ICON_TEXT.get(k) or ICON_LEARNED.get(k) for k in map(icon_key, cell.find_all(["img", "span"])) if k] if icon_only else []
                segs = [x for x in re.split(r"<br\s*/?>", cell.decode_contents()) if x.strip()]
                lines = [clean(BeautifulSoup(x, "html.parser"), fill_icons=False) for x in segs]
                lines = [x for x in lines if x]
                each_icon = len(segs) == len(lines) and all("fficn" in x or "<img" in x for x in segs)
                if len(lines) > 1 and each_icon and all(len(x) <= 30 for x in lines) and not cell.find(["p", "li"]):
                    row_opts += lines  # one option per line: CUSTOM 1<br>CUSTOM 2
                elif icon_only and len(parts) > 1 and all(parts):
                    row_opts += parts  # one icon per option: CUSTOM 1, 2, 3
                elif label:
                    row_opts.append(label)
        if not row_opts and all(not clean(c) for _, c in cells):
            out.append("")  # group separator
            continue
        out += row_opts
        if len(row_opts) == 1:
            for col, cell in cells:
                m = RANGE_CELL.match(clean(cell)) if col not in opt_cols else None
                if m:
                    rng = {"min": num(m.group(1)), "max": num(m.group(2))}
                    if m.group(3):
                        rng["unit"] = m.group(3)
                    ranges[row_opts[0]] = rng
        # Variants listed in the description (ACROS+Ye FILTER under ACROS).
        if len(row_opts) == 1:
            for col, cell in cells:
                if col in opt_cols:
                    continue
                for li in cell.find_all("li"):
                    st = li.find("strong")
                    v = clean(st) if st else ""
                    if v and norm(v).startswith(norm(row_opts[0])) and v != row_opts[0]:
                        out.append(v)
    return out, head_label


def dl_options(dl: Tag) -> list[str]:
    out = []
    for dt_ in dl.find_all("dt"):
        label = clean(dt_)
        if not label:
            continue
        if not re.search(r"[A-Za-z0-9]", clean(dt_, fill_icons=False)):
            parts = [ICON_TEXT.get(k) or ICON_LEARNED.get(k) for k in map(icon_key, dt_.find_all(["img", "span"])) if k]
            if len(parts) > 1 and all(parts):
                out += parts  # one icon per option: CUSTOM 1, 2, 3
                continue
        out.append(label)
        dd = dt_.find_next_sibling("dd")
        if dd and label in ("ACROS", "MONOCHROME"):
            filters = [clean(s) for s in dd.find_all("strong")]
            if {"Ye", "R", "G"} <= set(filters):
                out += [f"{label}+{f} FILTER" for f in ("Ye", "R", "G")]
    return out


def finish_options(item: dict, opts: list[str], head: str | None, ranges: dict | None = None) -> None:
    if ranges and set(ranges) == {o for o in opts if o}:
        # Each "option" is a slider of its own (TONE CURVE: HIGHLIGHTS, SHADOWS).
        for o in opts:
            if o and not any(norm(s["name"]) == norm(o) for s in item["sub"]):
                item["sub"].append({"name": o, "level": 9, "sub": [], "range": ranges[o]})
        return
    groups: list[list[str]] = [[]]
    for o in opts:
        if o == "":
            if groups[-1]:
                groups.append([])
        else:
            groups[-1].append(o)
    groups = [g for g in groups if g]
    if not groups:
        return
    flat = [o for g in groups for o in g]
    seen = []
    for o in flat:
        if o not in seen:
            seen.append(o)
    item["options"] = seen
    if head and head.lower() not in ("option", "options", ""):
        item["option_header"] = head
    rng = as_range(seen)
    if rng:
        item["range"] = rng


# --------------------------------------------------------------------------- pages


def parse_detail(path: Path, rel: str) -> dict:
    """One manual page: its title, mode, and the items it describes."""
    soup = BeautifulSoup(path.read_bytes(), "html.parser")
    title_tag = soup.find("title")
    title = clean(title_tag) if title_tag else ""
    main = main_content(soup)
    mode = None
    if "(Movie" in title:
        mode = "movie"
    elif "(Still" in title:
        mode = "stills"
    h1 = main.find("h1")
    page_name = re.sub(r"\s*\((Still|Movie)[^)]*\)\s*$", "", clean(h1) if h1 else title)
    root = {"name": page_name, "level": 1, "sub": []}
    stack = [root]
    taken: set[int] = set()
    for el in main.find_all(["h1", "h2", "h3", "h4", "table", "dl", "p"]):
        if el.name in ("h1", "h2", "h3", "h4"):
            if el.name == "h1" or skip_container(el):
                continue
            level = int(el.name[1])
            name = clean(el)
            if not name:
                continue
            node = {"name": name, "level": level, "sub": [], "anchor": el.get("id")}
            while stack[-1]["level"] >= level:
                stack.pop()
            stack[-1]["sub"].append(node)
            stack.append(node)
            continue
        if el.name == "p" and "text-heading" not in (el.get("class") or []):
            if not el.find_parent("table"):
                # Notes included: ranges are often stated there ("from 2500 to 10000 K").
                stack[-1].setdefault("text", []).append(clean(el))
            continue
        if skip_container(el) or "table-menu" in (el.get("class") or []):
            continue
        if el.find_parent(["table", "dl"]):
            continue
        if el.name == "p" and "text-heading" in (el.get("class") or []) and not skip_container(el):
            name = clean(el)  # a paragraph styled as a sub-heading (X-Pro3: ROUGHNESS, SIZE)
            if name and menu_label(name):
                level = 3 if any(n["level"] == 2 for n in stack) else 2
                node = {"name": name, "level": level, "sub": [], "anchor": None}
                while stack[-1]["level"] >= level:
                    stack.pop()
                stack[-1]["sub"].append(node)
                stack.append(node)
                continue
        node = stack[-1]
        ranges = None
        if el.name == "table":
            opts, head = table_options(el)
            ranges = getattr(el, "ukiyo_ranges", None)
        else:
            opts, head = dl_options(el), None
        if not opts:
            continue
        if id(node) in taken:
            # A list split over side-by-side tables (IMAGE SIZE): same header, same item.
            if el.name == "table" and head and node.get("_head") == head and "options" in node and not ranges:
                more = [o for o in opts if o and o not in node["options"]]
                node["options"] += more
            continue
        finish_options(node, opts, head, ranges)
        node["_head"] = head
        taken.add(id(node))
    filter_variants(root)
    prose_ranges(root)
    strongs = [clean(x) for x in main.find_all("strong")]
    return {"rel": rel, "title": title, "name": page_name, "mode": mode, "root": root, "strong": strongs}


def filter_variants(node: dict) -> None:
    """ACROS/MONOCHROME +Ye/R/G FILTER when the manual names the filters only in a footnote."""
    opts = node.get("options") or []
    text = " ".join(node.get("text", []))
    if norm(node["name"]).startswith("FILMSIMULATION") and "yellow" in text.lower() and "(Ye)" in text:
        for base in ("ACROS", "MONOCHROME"):
            if base in opts and not any(o.startswith(base + "+") for o in opts):
                i = opts.index(base) + 1
                opts[i:i] = [f"{base}+{f} FILTER" for f in ("Ye", "R", "G")]
    for s in node["sub"]:
        filter_variants(s)


def prose_ranges(node: dict) -> None:
    """A range stated in the text ("from 2500 to 10000 K") for an item without options."""
    text = " ".join(node.pop("text", []))
    if "options" not in node and "range" not in node and text:
        m = PROSE_RANGE.search(text)
        if m:
            rng = {"min": num(m.group(1)), "max": num(m.group(3))}
            unit = m.group(2) or m.group(4)
            if unit:
                rng["unit"] = unit
            st = re.search(r"increments of (\d+(?:\.\d+)?)\s*(K|EV)?\b", text)
            if st:
                rng["step"] = num(st.group(1))
            if rng["min"] < rng["max"]:
                node["range"] = rng
                node["range_from_text"] = True
    for s in node["sub"]:
        prose_ranges(s)


def parse_defaults(path: Path) -> dict[str, str]:
    """Factory-defaults table (older manuals): {norm(section)+'>'+norm(item path): default}."""
    soup = BeautifulSoup(path.read_bytes(), "html.parser")
    out: dict[str, str] = {}
    section = ""
    item = ""
    for table in soup.find_all("table", class_="table-menu"):
        for tr in table.find_all("tr"):
            cells = tr.find_all(["th", "td"], recursive=False)
            ths = [c for c in cells if c.name == "th"]
            if len(cells) == 1 and "parent" in (cells[0].get("class") or []):
                section = clean(cells[0])
                item = ""
                continue
            names = [clean(c) for c in ths if "parent-continue" not in (c.get("class") or []) and clean(c)]
            tds = [c for c in cells if c.name == "td"]
            if names:
                if len(names) == 1 and int(ths[-1].get("colspan", 1)) >= 2:
                    item = names[0]
                    path_ = [item]
                elif len(names) == 1:
                    path_ = [item, names[0]] if item else names
                else:
                    item = names[0]
                    path_ = names
            else:
                continue
            if not tds:
                continue
            ps = tds[-1].find_all("p")
            value = "; ".join(clean(p) for p in ps if clean(p)) if ps else clean(tds[-1])
            if value and not value.lower().startswith("see "):
                out[norm(section) + ">" + ">".join(norm(p) for p in path_)] = value
    return out


def parse_menu_list(path: Path) -> list[dict]:
    """Menu List page (newer manuals): menus, tabs, pages and items in camera order."""
    soup = BeautifulSoup(path.read_bytes(), "html.parser")
    main = main_content(soup)
    tabs: list[dict] = []
    menu = None
    mode = None
    tab = None
    page_no: int | None = None
    for el in main.find_all(["h2", "h3", "p", "table", "ul"]):
        if el.name == "h2":
            menu, mode, tab = clean(el), None, None
        elif el.name == "h3":
            m = re.match(r"^\s*(\d+)\s*/\s*(\d+)\s*$", el.get_text(""))
            if m:
                page_no = int(m.group(1))  # X-Pro3 style: "1/3" heading, then a list
                continue
            t = clean(el).lower()
            mode = "stills" if "photo" in t or "still" in t else "movie" if "movie" in t else None
        elif el.name == "ul" and tab is not None and not skip_container(el) and not el.find_parent(["ul", "table"]):
            items = []
            for li in el.find_all("li", recursive=False):
                ic = icons(li)
                name = clean(li) or ICON_LABELS.get(" ".join(f"{c}:{g}" for c, g in ic), "")
                if name:
                    items.append({"name": name, "modes": sorted({MODE_ICONS[i] for i in ic if i in MODE_ICONS})})
            if items:
                tab["pages"].append({"page": page_no, "items": items})
                page_no = None
        elif el.name == "p" and "menu-header" in (el.get("class") or []):
            tab = {"menu": menu, "mode": mode, "name": clean(el), "pages": []}
            tabs.append(tab)
            page_no = None
        elif el.name == "table" and not skip_container(el):
            if tab is None:
                if menu is None:
                    continue
                tab = {"menu": menu, "mode": mode, "name": re.sub(r"^The ", "", menu).upper(), "pages": []}
                tabs.append(tab)
            hdr = el.find("th", class_="bg-tableheader")
            page_no = None
            if hdr:
                m = re.match(r"\s*(\d+)\s*/\s*(\d+)", hdr.get_text(""))
                if m:
                    page_no = int(m.group(1))
            items = []
            for td in el.find_all("td"):
                ic = icons(td)
                name = clean(td)
                if not name:
                    key = " ".join(f"{c}:{g}" for c, g in ic)
                    name = ICON_LABELS.get(key, "")
                    if not name:
                        continue
                modes = sorted({MODE_ICONS[i] for i in ic if i in MODE_ICONS})
                items.append({"name": name, "modes": modes})
            if items:
                tab["pages"].append({"page": page_no, "items": items})
    return tabs


# --------------------------------------------------------------------------- build

MENU_KEYS = [("network", "network"), ("usb", "network"), ("connection", "network"),
             ("playback", "playback"), ("play back", "playback"), ("setup", "setup"),
             ("set-up", "setup"), ("shooting", "shooting")]
SKIP_PAGES = ("", "menu_shooting/", "menu_setup/", "menu_playback/", "introduction/menu_list/")


def menu_key(name: str) -> str:
    low = name.lower()
    for k, v in MENU_KEYS:
        if k in low:
            return v
    return low


def firmware(pages: dict[str, Path]) -> tuple[str | None, str | None]:
    for rel, p in pages.items():
        if not rel.endswith(".pdf"):
            continue
        try:
            text = subprocess.run(
                ["pdftotext", "-l", "2", str(p), "-"], capture_output=True, text=True, check=True
            ).stdout
        except (OSError, subprocess.CalledProcessError):
            return None, rel
        m = re.search(r"Version\s+(\d+\.\d+)", text)
        return (m.group(1) if m else None), rel
    return None, None


def nfg_changes(pages: dict[str, Path]) -> list[dict]:
    """(version, heading) for each change the New Features Guide documents."""
    for rel, p in pages.items():
        if not rel.endswith(".pdf"):
            continue
        try:
            text = subprocess.run(
                ["pdftotext", "-layout", str(p), "-"], capture_output=True, text=True, check=True
            ).stdout
        except (OSError, subprocess.CalledProcessError):
            return []
        out = []
        lines = text.splitlines()
        for i, line in enumerate(lines):
            m = re.search(r"Owner.s Manual: P [\d, ]+.*?Ver\. ?(\d+\.\d+)", line)
            if not m:
                continue
            head = next((x.strip() for x in lines[i + 1 : i + 4] if x.strip()), "")
            head = re.sub(r"^[A-Za-z] (?=[A-Z])", "", head)  # leading menu-icon glyph
            head = re.sub(r"\s{2,}.*$", "", head)
            if head and {"version": m.group(1), "heading": head} not in out:
                out.append({"version": m.group(1), "heading": head})
        return out
    return []


def is_section_page(d: dict) -> bool:
    subs = d["root"]["sub"]
    upper = [s for s in subs if menu_label(s["name"])]
    return len(upper) >= 2


def menu_label(name: str) -> bool:
    """Menu labels are upper case; prose headings ("Fine-Tuning White Balance") are not."""
    letters = re.sub(r"[^A-Za-z]", "", name)
    return bool(letters) and sum(c.isupper() for c in letters) / len(letters) > 0.6


def to_item(node: dict, rel: str | None, dkey: str, defaults: dict[str, str]) -> dict:
    out: dict = {"name": node["name"]}
    for k in ("options", "option_header", "range", "range_from_text"):
        if k in node:
            out[k] = node[k]
    key = dkey + ">" + norm(node["name"])
    if key in defaults:
        out["default"] = defaults[key]
    subs = [s for s in node["sub"] if menu_label(s["name"]) or "options" in s or "range" in s]
    if subs:
        out["sub"] = [to_item(s, rel, key, defaults) for s in subs]
    if rel is not None:
        out["ref"] = rel + ("#" + node["anchor"] if node.get("anchor") else "")
    return out


def find_default(defaults: dict[str, str], tab: str, item: str) -> str:
    """The defaults key prefix for an item: its own tab if listed there, else any tab."""
    k = norm(tab) + ">" + norm(item)
    if k in defaults or any(x.startswith(k + ">") for x in defaults):
        return norm(tab)
    for x in defaults:
        sec, _, rest = x.partition(">")
        if rest == norm(item) or rest.startswith(norm(item) + ">"):
            return sec
    return norm(tab)


def section_names(pages: dict[str, Path]) -> list[str]:
    """Section names from the factory-defaults tables, in order."""
    names: list[str] = []
    for rel, p in pages.items():
        if rel.strip("/").rsplit("/", 1)[-1].startswith("default_"):
            soup = BeautifulSoup(p.read_bytes(), "html.parser")
            for th in soup.select("table.table-menu th.parent"):
                n = clean(th)
                if n and n not in names:
                    names.append(n)
    return names


def build(slug: str, pages: dict[str, Path]) -> dict:
    model, generation, sensor, processor = BODIES[slug]
    fw, nfg = firmware(pages)
    details = []
    defaults: dict[str, str] = {}
    for rel, p in pages.items():
        if rel.endswith(".pdf") or rel in SKIP_PAGES:
            continue
        leaf = rel.strip("/").rsplit("/", 1)[-1]
        if leaf.startswith("default_"):
            defaults.update(parse_defaults(p))
            continue
        if leaf.startswith("use_"):
            continue
        details.append(parse_detail(p, rel))

    # Index every described item by (page, mode, item).
    index: dict[tuple[str, str | None, str], tuple[dict, str]] = {}
    by_name: dict[str, list[tuple[dict, str, str | None]]] = {}
    for d in details:
        nodes = d["root"]["sub"] if is_section_page(d) else [dict(d["root"], anchor=None)]
        for n in nodes:
            index.setdefault((norm(d["name"]), d["mode"], norm(n["name"])), (n, d["rel"]))
            by_name.setdefault(norm(n["name"]), []).append((n, d["rel"], d["mode"]))

    def lookup(tab: str, mode: str | None, name: str) -> tuple[dict, str] | None:
        for m in (mode, None, "stills", "movie"):
            hit = index.get((norm(tab), m, norm(name)))
            if hit:
                return hit
        cands = by_name.get(norm(name), [])
        for n, rel, m in cands:
            if m == mode:
                return n, rel
        return (cands[0][0], cands[0][1]) if cands else None

    tabs_out: list[dict] = []
    matched = total = 0
    if "introduction/menu_list/" in pages:
        layout = "menu list"
        for tab in parse_menu_list(pages["introduction/menu_list/"]):
            t_out = {"menu": menu_key(tab["menu"] or ""), "mode": tab["mode"], "name": tab["name"], "pages": []}
            for pg in tab["pages"]:
                items = []
                for it in pg["items"]:
                    total += 1
                    hit = lookup(tab["name"], tab["mode"], it["name"])
                    dkey = find_default(defaults, tab["name"], it["name"])
                    if hit:
                        matched += 1
                        item = to_item(dict(hit[0], name=it["name"]), hit[1], dkey, defaults)
                    else:
                        item = to_item({"name": it["name"], "sub": []}, None, dkey, defaults)
                    if it["modes"]:
                        item["modes"] = it["modes"]
                    items.append(item)
                t_out["pages"].append({"page": pg["page"], "items": items})
            tabs_out.append(t_out)
    else:
        layout = "section index"
        by_rel = {d["rel"]: d for d in details}
        sections = section_names(pages)
        for sec in ("menu_shooting/", "menu_setup/", "menu_playback/"):
            if sec not in pages:
                continue
            soup = BeautifulSoup(pages[sec].read_bytes(), "html.parser")
            main = main_content(soup)
            h1 = main.find("h1")
            menu_name = re.sub(r"^The ", "", clean(h1) if h1 else sec)
            key = menu_key(sec.replace("_", " "))
            order = []
            for a in main.select("ul.contents-list a[href]"):
                r = rel_path(BASE + slug + "/", a["href"], BASE + slug + "/" + sec)
                if r in by_rel and r not in order:
                    order.append(r)
            local_tabs: dict[str, dict] = {}
            last_tab = None
            for r in order:
                d = by_rel[r]
                section_page = (norm(d["name"]) in {norm(x) for x in sections}) if sections else is_section_page(d)
                if section_page:
                    tname, nodes, anchored = d["name"], d["root"]["sub"], True
                else:
                    tname, nodes, anchored = None, [d["root"]], False
                for n in nodes:
                    name = n["name"]
                    t = tname
                    if t is None:
                        # Item pages: the factory-defaults table names the tab; an item
                        # it does not list stays in the tab of the item before it.
                        sec_key = find_default(defaults, "", name) if key == "shooting" else ""
                        t = next((s for s in sections if norm(s) == sec_key), None)
                        if t is None and key == "shooting":
                            # The page's own procedure names it: "Select AF/MF SETTING > ...".
                            t = next((x for x in d["strong"] if norm(x) in {norm(s) for s in sections}), None)
                        t = t or last_tab or menu_name.upper()
                    last_tab = t
                    tab = local_tabs.get(t)
                    if tab is None:
                        tab = {"menu": key, "mode": None, "name": t, "pages": [{"page": None, "items": []}]}
                        local_tabs[t] = tab
                        tabs_out.append(tab)
                    total += 1
                    matched += 1
                    node = n if anchored else dict(n, anchor=None)
                    tab["pages"][0]["items"].append(to_item(node, r, find_default(defaults, t, name), defaults))

    changes = nfg_changes(pages)
    for item in iter_items({"tabs": tabs_out}):
        vers = sorted({c["version"] for c in changes if norm(c["heading"]) == norm(item["name"])},
                      key=lambda v: [int(x) for x in v.split(".")])
        if vers:
            item["changed_in"] = vers

    p0 = pages.get("")
    fetched = dt.date.fromtimestamp(p0.stat().st_mtime if p0 else time.time()).isoformat()
    return {
        "schema": 1,
        "model": model,
        "manual": {
            "url": BASE + slug + "/",
            "language": "en",
            "firmware": fw,
            "firmware_source": nfg,
            "layout": layout,
            "fetched": fetched,
        },
        "generation": generation,
        "sensor": sensor,
        "processor": processor,
        "stats": {"items": total, "described": matched},
        "tabs": tabs_out,
        "firmware_changes": changes,
    }


def iter_items(doc: dict):
    def walk(items):
        for it in items:
            yield it
            yield from walk(it.get("sub", []))

    for tab in doc["tabs"]:
        for pg in tab["pages"]:
            yield from walk(pg["items"])


# --------------------------------------------------------------------------- main


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("models", nargs="*", help="manual slugs, e.g. x-t5")
    ap.add_argument("--offline", action="store_true", help="use the cache only")
    ap.add_argument("--refetch", action="store_true", help="ignore the cache")
    ap.add_argument("--fetch-only", action="store_true", help="fill the cache, write nothing")
    args = ap.parse_args()
    slugs = args.models or list(BODIES)
    unknown = [s for s in slugs if s not in BODIES]
    if unknown:
        print(f"unknown model: {', '.join(unknown)}", file=sys.stderr)
        return 2
    f = Fetcher(args.offline, args.refetch)
    failed = []
    crawled = {slug: crawl(slug, f) for slug in slugs}
    if not args.fetch_only:
        # Older manuals print some options only as icons; learn their names from
        # every cached manual's shooting-menu pages.
        ICON_LEARNED.update(learn_icons(sorted(CACHE.glob("*/menu_shooting*.html"))))
    for slug in slugs:
        pages = crawled[slug]
        html_pages = [p for p in pages if not p.endswith(".pdf")]
        print(f"{slug}: {len(html_pages)} pages, nfg={'yes' if any(p.endswith('.pdf') for p in pages) else 'no'}")
        if "" not in pages:
            failed.append(slug)
            continue
        if args.fetch_only:
            continue
        doc = build(slug, pages)
        OUT.mkdir(parents=True, exist_ok=True)
        out = OUT / f"{slugify(BODIES[slug][0])}.json"
        out.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
        n = sum(1 for _ in iter_items(doc))
        print(f"  -> {out.relative_to(ROOT)} ({n} items, firmware {doc['manual']['firmware']})")
    print(f"requests: {f.fetched}")
    if failed:
        print(f"failed: {', '.join(failed)}", file=sys.stderr)
        return 1
    return 0


def slugify(model: str) -> str:
    return re.sub(r"[^a-z0-9]", "-", model.lower())


if __name__ == "__main__":
    sys.exit(main())
