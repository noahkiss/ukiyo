# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow>=11", "numpy>=2", "scipy>=1.13"]
# ///
"""Split a brush app-icon final into Liquid Glass layers and write an Icon Composer bundle.

Usage: app_icon_layers.py SRC_LIGHT.png SRC_DARK.png OUT.icon [--scale S]

SRC_LIGHT is the flat final on paper (sumi ink, a red seal). SRC_DARK is the same design
drawn in paper-white ink on sumi; only its colours are sampled. The finals are not in git:
they live in image-output.local/finals/ (from a worktree, pass the main checkout's).

The script writes OUT.icon/icon.json and OUT.icon/Assets/:
  enso-light.png enso-dark.png    the brush circle
  glyph-light.png glyph-dark.png  the character inside it
  seal.png                        the red seal
Each layer is one flat colour on a transparent ground: alpha comes from the ink coverage,
so no paper texture or fringe is carried over. The paper colour becomes the bundle's
`fill`, with a sumi dark specialization. Specular highlights are on for the seal; on the
ink they are on only in the clear and tinted renditions, since in the default ones they
draw a bright rim round every dry-brush speck. The system draws the tile and the glass.

The enso and the character are told apart by connected components: the character is the
big strokes inside RING_R plus anything within TAIL_PX of them; every other speck is enso.
Ink beyond OUTER_R (the source's tile shading) is dropped. The art is scaled by --scale
about its centre (default 0.86), so the enso sits inside the central ~80% of the tile.
"""
import argparse
import json
import pathlib

import numpy as np
from PIL import Image
from scipy import ndimage

SIZE = 1024
RING_R = 360  # px in the 1024 source: glyph strokes stay inside, the enso lies outside
OUTER_R = 492  # px: nothing of the design lies beyond this radius
CORE_AREA = 1500  # px: a glyph stroke is at least this big
TAIL_PX = 14  # px: dry-brush tails within this distance of a stroke stay with it


def load(path):
    img = Image.open(path).convert("RGBA").resize((SIZE, SIZE), Image.LANCZOS)
    return np.asarray(img).astype(np.float32)


def layer(alpha, rgb):
    out = np.zeros((SIZE, SIZE, 4), np.uint8)
    out[..., :3] = rgb
    out[..., 3] = np.clip(alpha * 255 + 0.5, 0, 255).astype(np.uint8)
    return out


def transform(arr, scale, centre):
    """Scale an RGBA layer about `centre` and move that point to the canvas centre."""
    img = Image.fromarray(arr)
    # Scale alpha only; colour is flat, so resampling cannot fringe.
    w = round(SIZE * scale)
    small = img.resize((w, w), Image.LANCZOS)
    out = Image.new("RGBA", (SIZE, SIZE), tuple(int(v) for v in arr[0, 0, :3]) + (0,))
    cx, cy = centre
    ox = round(SIZE / 2 - cx * scale)
    oy = round(SIZE / 2 - cy * scale)
    out.paste(small, (ox, oy))
    res = np.asarray(out).copy()
    res[..., :3] = arr[0, 0, :3]
    return res


def srgb(rgb):
    r, g, b = (round(float(v) / 255, 5) for v in rgb)
    return f"srgb:{r},{g},{b},1"


def group(name, light, dark_img=None, shadow="neutral", specular=False):
    """One Icon Composer group holding one layer, with an optional dark image.

    specular=False keeps highlights off in the default and dark renditions but turns
    them on for clear and tinted, where flat ink would otherwise all but vanish.
    """
    spec = [{"value": f"{light}.png"}]
    if dark_img:
        spec.append({"appearance": "dark", "value": f"{dark_img}.png"})
    g = {
        "name": name.capitalize(),
        "layers": [{"name": name, "image-name-specializations": spec, "glass": True}],
        "shadow": {"kind": shadow, "opacity": 0.5},
        "translucency": {"enabled": False, "value": 0.5},
        "lighting": "combined",
    }
    if specular:
        g["specular"] = True
    else:
        g["specular-specializations"] = [{"value": False}, {"appearance": "tinted", "value": True}]
    return g


def write_icon(out, paper, sumi, groups):
    """icon.json: groups front first, the paper fill with a sumi dark specialization."""
    icon = {
        "fill-specializations": [
            {"value": {"solid": srgb(paper)}},
            {"appearance": "dark", "value": {"solid": srgb(sumi)}},
        ],
        "groups": groups,
        "supported-platforms": {"squares": "shared"},
    }
    (pathlib.Path(out) / "icon.json").write_text(json.dumps(icon, indent=2) + "\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("light")
    ap.add_argument("dark")
    ap.add_argument("out")
    ap.add_argument("--scale", type=float, default=0.86)
    args = ap.parse_args()

    src = load(args.light)
    dark = load(args.dark)
    rgb = src[..., :3]
    lum = rgb.mean(-1)

    # Seal: the red region and the box around it.
    redness = rgb[..., 0] - (rgb[..., 1] + rgb[..., 2]) / 2
    red = redness > 80
    lbl, n = ndimage.label(red)
    sizes = ndimage.sum(red, lbl, range(1, n + 1))
    seal_id = int(np.argmax(sizes)) + 1
    ys, xs = np.nonzero(lbl == seal_id)
    pad = 6
    box = np.zeros_like(red)
    box[ys.min() - pad:ys.max() + pad, xs.min() - pad:xs.max() + pad] = True
    seal_alpha = np.clip((redness - 40) / 100, 0, 1) * box
    seal_rgb = np.median(rgb[red & box], axis=0)

    # Ink: darkness relative to the paper.
    paper = np.median(rgb[(lum > 200) & (src[..., 3] > 200)], axis=0)
    ink_rgb = np.median(rgb[lum < 40], axis=0)
    l_paper, l_ink = float(paper.mean()) - 12, float(ink_rgb.mean()) + 20
    ink_alpha = np.clip((l_paper - lum) / (l_paper - l_ink), 0, 1)
    ink_alpha[box] = 0

    # Split the ink into enso and glyph.
    solid = ink_alpha > 0.35
    lbl, n = ndimage.label(solid, structure=np.ones((3, 3)))
    yy, xx = np.mgrid[0:SIZE, 0:SIZE]
    ys, xs = np.nonzero(solid)
    cx, cy = (xs.min() + xs.max()) / 2, (ys.min() + ys.max()) / 2
    r = np.hypot(xx - cx, yy - cy)
    # The source's own tile shading sits in the corners, outside the enso.
    ink_alpha[r > OUTER_R] = 0
    med = np.asarray(ndimage.median(r, lbl, range(1, n + 1)))
    area = np.asarray(ndimage.sum(solid, lbl, range(1, n + 1)))
    core_ids = np.nonzero((med < RING_R) & (area >= CORE_AREA))[0] + 1
    core = np.isin(lbl, core_ids)
    # The character is its big strokes plus anything close to them (dry-brush tails).
    # Every other speck, solid or faint, belongs to the enso.
    glyph_mask = ndimage.distance_transform_edt(~core) <= TAIL_PX
    enso_alpha = ink_alpha * ~glyph_mask
    glyph_alpha = ink_alpha * glyph_mask
    n_core = len(core_ids)

    # Dark colours from the dark final: its ground and its paper-white ink.
    dl = dark[..., :3].mean(-1)
    sumi = np.median(dark[..., :3][(dl < 40) & (dark[..., 3] > 200)], axis=0)
    dark_ink = np.median(dark[..., :3][dl > 200], axis=0)

    centre = (cx, cy)
    out = pathlib.Path(args.out)
    assets = out / "Assets"
    assets.mkdir(parents=True, exist_ok=True)
    for name, alpha, col in [
        ("enso-light", enso_alpha, ink_rgb),
        ("enso-dark", enso_alpha, dark_ink),
        ("glyph-light", glyph_alpha, ink_rgb),
        ("glyph-dark", glyph_alpha, dark_ink),
        ("seal", seal_alpha, seal_rgb),
    ]:
        arr = transform(layer(alpha, col.round().astype(np.uint8)), args.scale, centre)
        Image.fromarray(arr).save(assets / f"{name}.png", optimize=True)

    write_icon(out, paper, sumi, [
        group("seal", "seal", shadow="layer-color", specular=True),
        group("glyph", "glyph-light", "glyph-dark"),
        group("enso", "enso-light", "enso-dark"),
    ])
    print(f"paper {paper.round()} ink {ink_rgb.round()} seal {seal_rgb.round()} "
          f"sumi {sumi.round()} dark ink {dark_ink.round()} centre ({cx:.0f},{cy:.0f}) "
          f"glyph strokes {n_core} of {n} components")


if __name__ == "__main__":
    main()
