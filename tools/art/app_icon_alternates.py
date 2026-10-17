# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow>=11", "numpy>=2", "scipy>=1.13"]
# ///
"""Split the Appearance alternate app icons into Liquid Glass layers (Icon Composer bundles).

Usage: app_icon_alternates.py FINALS_DIR OUT_DIR [--only seal|sun|aperture ...]

FINALS_DIR holds the flat finals R3-I2|I4|I13-light|dark.png (image-output.local/finals/,
not in git; from a worktree, pass the main checkout's). Writes OUT_DIR/<id>.icon:

  seal.icon      (R3-I2)  enso, the red seal, the 浮世 characters cut into it
  sun.icon       (R3-I4)  enso, the sun disc (red; paper-white in dark)
  aperture.icon  (R3-I13) enso, the aperture blades (navy; two greys in dark), the red centre

Every layer is flat colour on a transparent ground; the paper is the bundle's fill, with a
sumi dark specialization, as for the main icon (app_icon_layers.py). The enso's alpha comes
from the ink in the light final, with the coloured parts masked out. Circles (the sun, the
aperture centre) are redrawn as true circles from the area and centre of their mask. The
art is scaled by --scale about the centre of the coloured part, which sits at the tile centre.
"""
import argparse
import pathlib

import numpy as np
from PIL import Image
from scipy import ndimage

from app_icon_layers import SIZE, group, layer, load, transform, write_icon

OUTER_R = 560  # px: past this the source shows its own tile corners
SOURCES = {"seal": "R3-I2", "sun": "R3-I4", "aperture": "R3-I13"}


def biggest(mask):
    lbl, n = ndimage.label(mask)
    if n == 0:
        raise SystemExit("no region found")
    sizes = ndimage.sum(mask, lbl, range(1, n + 1))
    return lbl == int(np.argmax(sizes)) + 1


def disc(mask):
    """A true antialiased circle with the mask's centre and area."""
    ys, xs = np.nonzero(mask)
    cy, cx, r = ys.mean(), xs.mean(), np.sqrt(mask.sum() / np.pi)
    yy, xx = np.mgrid[0:SIZE, 0:SIZE]
    return np.clip(r - np.hypot(xx - cx, yy - cy) + 0.5, 0, 1)


def median_rgb(rgb, mask):
    return np.median(rgb[mask], axis=0)


def soft(mask, px=1.0):
    return np.clip(ndimage.gaussian_filter(mask.astype(np.float32), px), 0, 1)


def build(icon_id, finals, out, scale):
    stem = SOURCES[icon_id]
    src = load(finals / f"{stem}-light.png")
    dark = load(finals / f"{stem}-dark.png")
    rgb, drgb = src[..., :3], dark[..., :3]
    lum, dlum = rgb.mean(-1), drgb.mean(-1)
    redness = rgb[..., 0] - (rgb[..., 1] + rgb[..., 2]) / 2
    blueness = rgb[..., 2] - rgb[..., 0]

    paper = median_rgb(rgb, (lum > 200) & (src[..., 3] > 200))
    ink = median_rgb(rgb, lum < 40)
    sumi = median_rgb(drgb, (dlum < 40) & (dark[..., 3] > 200))
    dark_ink = median_rgb(drgb, dlum > 200)

    layers = {}  # name -> (alpha, light rgb, dark rgb or None)
    parts = np.zeros((SIZE, SIZE), bool)

    if icon_id == "seal":
        # The seal is a red field and a red outer frame, split by a paper-white line.
        core = biggest(redness > 80)
        near = ndimage.binary_dilation(core, iterations=30)
        red = (redness > 80) & near
        body = ndimage.binary_fill_holes(ndimage.binary_closing(red, iterations=6))
        body = ndimage.binary_opening(body, iterations=2)
        seal_red = median_rgb(rgb, red)
        # Paper-white marks inside the seal: the characters and the inner frame line.
        text = body & (redness < 60) & (lum > 150)
        text = ndimage.binary_opening(text) & ndimage.binary_erosion(body, iterations=2)
        text_rgb = median_rgb(rgb, text)
        text_a = soft(text, 0.7)
        layers["text"] = (text_a, text_rgb, None)
        layers["seal"] = (np.clip(soft(body, 0.7) - text_a, 0, 1), seal_red, None)
        parts = body
    elif icon_id == "sun":
        red = biggest(redness > 80)
        sun = disc(red)
        dark_disc = median_rgb(drgb, ndimage.binary_erosion(red, iterations=10))
        layers["sun"] = (sun, median_rgb(rgb, red), dark_disc)
        parts = sun > 0
    elif icon_id == "aperture":
        red = biggest(redness > 80)
        centre = disc(red)
        blades = (blueness > 25) & (lum < 160)
        blades = ndimage.binary_opening(blades, iterations=1)
        lbl, n = ndimage.label(blades)
        sizes = ndimage.sum(blades, lbl, range(1, n + 1))
        keep = np.nonzero(sizes > 2000)[0] + 1
        blades = np.isin(lbl, keep)
        # Dark: each blade keeps its own tone from the dark final.
        dark_blades = np.zeros((SIZE, SIZE, 3), np.float32)
        for i in keep:
            core = ndimage.binary_erosion(lbl == i, iterations=6)
            dark_blades[lbl == i] = median_rgb(drgb, core)
        dark_blades = ndimage.grey_dilation(dark_blades, size=(5, 5, 1))
        layers["centre"] = (centre, median_rgb(rgb, red), median_rgb(drgb, red))
        layers["blades"] = (soft(blades, 0.7), median_rgb(rgb, blades), dark_blades)
        parts = blades | (centre > 0)
        print(f"  blades: {len(keep)}")

    # Enso: ink coverage, outside the coloured parts.
    l_paper, l_ink = float(paper.mean()) - 12, float(ink.mean()) + 20
    ink_a = np.clip((l_paper - lum) / (l_paper - l_ink), 0, 1)
    ink_a[ndimage.binary_dilation(parts, iterations=8)] = 0
    ink_a[(redness > 40) | (blueness > 25)] = 0  # ink is neutral; colour belongs to a part
    # Centre on the coloured part: a brush tail makes the enso's box lopsided.
    ys, xs = np.nonzero(parts)
    cx, cy = (xs.min() + xs.max()) / 2, (ys.min() + ys.max()) / 2
    yy, xx = np.mgrid[0:SIZE, 0:SIZE]
    ink_a[(np.hypot(xx - cx, yy - cy) > OUTER_R) | (src[..., 3] < 128)] = 0
    layers["enso"] = (ink_a, ink, dark_ink)

    bundle = out / f"{icon_id}.icon"
    assets = bundle / "Assets"
    assets.mkdir(parents=True, exist_ok=True)
    groups = []
    order = {"seal": ["text", "seal", "enso"], "sun": ["sun", "enso"],
             "aperture": ["centre", "blades", "enso"]}[icon_id]
    for name in order:
        alpha, light_rgb, dark_rgb = layers[name]
        arr = transform(layer(alpha, light_rgb.round().astype(np.uint8)), scale, (cx, cy))
        Image.fromarray(arr).save(assets / f"{name}-light.png", optimize=True)
        dark_name = None
        if dark_rgb is not None:
            if dark_rgb.ndim == 1:
                arr = transform(layer(alpha, dark_rgb.round().astype(np.uint8)), scale, (cx, cy))
            else:  # per-pixel colour
                arr = layer(alpha, 0)
                arr[..., :3] = np.clip(dark_rgb.round(), 0, 255).astype(np.uint8)
                arr = scale_rgba(arr, scale, (cx, cy))
            Image.fromarray(arr).save(assets / f"{name}-dark.png", optimize=True)
            dark_name = f"{name}-dark"
        is_ink = name == "enso"
        groups.append(group(name, f"{name}-light", dark_name,
                            shadow="neutral" if is_ink else "layer-color",
                            specular=not is_ink))
    write_icon(bundle, paper, sumi, groups)
    print(f"{icon_id}: paper {paper.round()} sumi {sumi.round()} centre ({cx:.0f},{cy:.0f})")


def scale_rgba(arr, scale, centre):
    """Scale an RGBA layer with per-pixel colour about `centre` to the canvas centre."""
    w = round(SIZE * scale)
    small = Image.fromarray(arr).resize((w, w), Image.LANCZOS)
    out = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    cx, cy = centre
    out.paste(small, (round(SIZE / 2 - cx * scale), round(SIZE / 2 - cy * scale)))
    return np.asarray(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("finals", type=pathlib.Path)
    ap.add_argument("out", type=pathlib.Path)
    ap.add_argument("--only", nargs="*", choices=list(SOURCES))
    ap.add_argument("--scale", type=float, default=0.86)
    args = ap.parse_args()
    for icon_id in args.only or SOURCES:
        build(icon_id, args.finals, args.out, args.scale)


if __name__ == "__main__":
    main()
