# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow>=11", "numpy>=2"]
# ///
"""Build the Hub's camera art: one 3/4 ink drawing per body, light and dark, as webp.

Usage: camera_art.py [SRC_ROOT] [--only ID ...]
  SRC_ROOT holds the generated drawings (default: $UKIYO_ART_SRC, else the repo's
  image-output.local/; from a worktree, pass the main checkout's).
  Paths in SOURCES are relative to it. The drawings are not in git.

Each source is a transparent PNG. A sheet holds a front view and a 3/4 view; `pick` says
which blob to take ("right": the right-hand view, "all": the whole image, for a single
re-rendered view). Every body must face the X-T5's way (lens toward the viewer's left).
Never mirror one to fix that: it swaps the dials and the grip. Re-render it instead.

Framing matches the first X-T5 asset: an 800x628 canvas, the body fitted into the box
x 0-796, y 21-623, centred. Light keeps the drawing's grey and alpha. Dark negates it and
levels it to 24-225, the same alpha. Writes app/src/assets/camera/<id>-light.webp and
<id>-dark.webp.
"""
import argparse
import os
import pathlib

import numpy as np
from PIL import Image

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
OUT = ROOT / "app/src/assets/camera"
DEFAULT_SRC = pathlib.Path(os.environ.get("UKIYO_ART_SRC", ROOT / "image-output.local"))

# id: (source path under SRC_ROOT, pick)
SOURCES = {
    "x-t5": ("round2/body/2026-10-17/001_cdx_xt5-sheet.png", "right"),
    "x-t3": ("bodies-r3/2026-10-17/002_cdx_xt3-a1.png", "right"),
    "x-t4": ("round2/bodies/2026-10-17/008_cdx_xt4.png", "right"),
    "x-h2": ("round2/bodies/2026-10-17/004_cdx_xh2-fix.png", "right"),
    "x-h2s": ("hub-art/2026-10-17/002_cdx_xh2s-tq-a1.png", "all"),
    "x-s20": ("hub-art/2026-10-17/004_cdx_xs20-tq-a2.png", "all"),
    "x100vi": ("bodies-r3/2026-10-17/005_cdx_x100vi-a1.png", "right"),
    "x-pro3": ("bodies-r3/2026-10-17/001_cdx_xpro3-a1.png", "right"),
    "x-e5": ("bodies-r3/2026-10-17/006_cdx_xe5-a2.png", "right"),
    "generic": ("hub-art/2026-10-17/003_cdx_generic-tq-a1.png", "all"),
}

W, H = 800, 628
BOX = (0, 21, 797, 624)  # x0, y0, x1, y1 (exclusive) of the X-T5 body in the first asset
ALPHA_FLOOR = 12  # alpha below this is generator speckle, cleared
DARK_LO, DARK_HI = 24, 225
QUALITY = 62  # webp; keeps each file near 100 KB


def solid(a: np.ndarray) -> np.ndarray:
    return a > 40


def pick_right(alpha: np.ndarray) -> slice:
    """Columns of the right-hand view: right of the widest empty column run in the middle."""
    cols = solid(alpha).sum(axis=0)
    w = len(cols)
    empty = cols < 3
    best, run, start = (0, w // 2), 0, 0
    for x in range(int(w * 0.3), int(w * 0.7)):
        if empty[x]:
            if run == 0:
                start = x
            run += 1
            if run > best[0]:
                best = (run, start + run)
        else:
            run = 0
    return slice(best[1], w)


def bbox(alpha: np.ndarray) -> tuple[int, int, int, int]:
    m = solid(alpha)
    xs = np.nonzero(m.sum(axis=0) >= 3)[0]
    ys = np.nonzero(m.sum(axis=1) >= 3)[0]
    return xs[0], ys[0], xs[-1] + 1, ys[-1] + 1


def build(src: pathlib.Path, pick: str) -> Image.Image:
    rgba = np.array(Image.open(src).convert("RGBA"))
    if pick == "right":
        rgba = rgba[:, pick_right(rgba[..., 3])]
    gray = np.array(Image.fromarray(rgba).convert("L"))
    alpha = rgba[..., 3].copy()
    alpha[alpha < ALPHA_FLOOR] = 0
    x0, y0, x1, y1 = bbox(alpha)
    la = np.dstack([gray, alpha])[y0:y1, x0:x1]
    bw, bh = BOX[2] - BOX[0], BOX[3] - BOX[1]
    s = min(bw / (x1 - x0), bh / (y1 - y0))
    nw, nh = round((x1 - x0) * s), round((y1 - y0) * s)
    # Premultiply so resampling does not pull grey from the transparent area into the edges.
    l = la[..., 0].astype(np.float32) * la[..., 1] / 255
    pre = Image.fromarray(np.dstack([l, la[..., 1]]).astype(np.uint8), "LA")
    pre = np.array(pre.resize((nw, nh), Image.LANCZOS)).astype(np.float32)
    a = pre[..., 1]
    l = np.where(a > 0, pre[..., 0] * 255 / np.maximum(a, 1), 255).clip(0, 255)
    canvas = np.zeros((H, W, 2), np.uint8)
    ox = BOX[0] + (bw - nw) // 2
    oy = BOX[1] + (bh - nh) // 2
    canvas[oy:oy + nh, ox:ox + nw] = np.dstack([l, a]).astype(np.uint8)
    return Image.fromarray(canvas, "LA")


def dark(img: Image.Image) -> Image.Image:
    la = np.array(img).astype(np.float32)
    la[..., 0] = DARK_LO + (255 - la[..., 0]) * (DARK_HI - DARK_LO) / 255
    return Image.fromarray(la.round().astype(np.uint8), "LA")


def save(img: Image.Image, path: pathlib.Path) -> None:
    img.convert("RGBA").save(path, "WEBP", quality=QUALITY, alpha_quality=70, method=6)
    print(f"{path.relative_to(ROOT)}  {path.stat().st_size // 1024} KB")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("src_root", nargs="?", type=pathlib.Path, default=DEFAULT_SRC)
    ap.add_argument("--only", nargs="*")
    args = ap.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    for cid, (rel, pick) in SOURCES.items():
        if args.only and cid not in args.only:
            continue
        light = build(args.src_root / rel, pick)
        save(light, OUT / f"{cid}-light.webp")
        save(dark(light), OUT / f"{cid}-dark.webp")


if __name__ == "__main__":
    main()
