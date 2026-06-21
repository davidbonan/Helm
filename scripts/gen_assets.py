#!/usr/bin/env python3
"""Generate all Helm brand assets into assets/brand/.

Source of truth: the logomark geometry (focus frame, 4 brackets) measured on the
brand sheet. Output: SVG, PNG (icons, favicon, GitHub, splash), icon.icns
(iconutil) and .ico (PIL).

Usage: python3 scripts/gen_assets.py
"""

import math
import subprocess
import tempfile
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "brand"

# ── Palette ──────────────────────────────────────────────────────────────────
BG = "#0A0A0A"
FG = "#FFFFFF"
ACCENT_PURPLE = "#A78BFA"
GLOW_VIOLET = (109, 40, 217)  # #6D28D9, social preview glow

# ── Logomark geometry (measured on the sheet, units 0..1) ────────────────────
STROKE = 0.15  # stroke thickness
ARM = 0.35     # arm length
# The bracket is a filled L with small fillets (no round caps) — measured on the
# sheet for a 15px stroke: outer corner ≈ 5px, ends/inner ≈ 2px.
R_OUT = STROKE / 3       # outer corner fillet
R_CAP = 0.15 * STROKE    # arm-end fillets
R_IN = 0.15 * STROKE     # inner corner fillet

# Main logo lockup (measured): mark→text gap and text height = mark
LOCKUP_GAP = 0.46    # × mark height
TEXT_W_RATIO = 3.23  # "Helm" width ≈ 3.23 × mark height (reference)

# Icon tile
TILE_RADIUS = 0.22  # corner radius × tile side
MARK_IN_TILE = 0.50  # mark size × tile side
APPLE_MARGIN = 100 / 1024  # transparent inset around dock/icns tiles (Apple grid)

SFNS = "/System/Library/Fonts/SFNS.ttf"


def mark_brackets(x: float, y: float, size: float):
    """The mark's 4 brackets: lists of ((px, py), fillet radius), tracing the
    top-left L then mirroring H/V."""
    T, A = STROKE, ARM
    base = [
        ((0, 0), R_OUT), ((A, 0), R_CAP), ((A, T), R_CAP),
        ((T, T), R_IN), ((T, A), R_CAP), ((0, A), R_CAP),
    ]
    out = []
    for mx, my in ((0, 0), (1, 0), (1, 1), (0, 1)):
        out.append(
            [
                ((x + (1 - u if mx else u) * size, y + (1 - v if my else v) * size),
                 r * size)
                for (u, v), r in base
            ]
        )
    return out


def _fillet(verts, i):
    """Tangents and center of the fillet at vertex i (right-angled, axis-aligned corners)."""
    (vx, vy), r = verts[i]
    (px, py), _ = verts[i - 1]
    (nx, ny), _ = verts[(i + 1) % len(verts)]
    u1 = ((px - vx) / abs(px - vx) if px != vx else 0,
          (py - vy) / abs(py - vy) if py != vy else 0)
    u2 = ((nx - vx) / abs(nx - vx) if nx != vx else 0,
          (ny - vy) / abs(ny - vy) if ny != vy else 0)
    t1 = (vx + r * u1[0], vy + r * u1[1])
    t2 = (vx + r * u2[0], vy + r * u2[1])
    c = (vx + r * (u1[0] + u2[0]), vy + r * (u1[1] + u2[1]))
    return t1, t2, c, r


def rounded_polygon_points(verts, seg: int = 24):
    """Filleted polygon → list of points (sampled arcs), for PIL."""
    pts = []
    for i in range(len(verts)):
        t1, t2, c, r = _fillet(verts, i)
        a1 = math.atan2(t1[1] - c[1], t1[0] - c[0])
        a2 = math.atan2(t2[1] - c[1], t2[0] - c[0])
        delta = (a2 - a1 + math.pi) % (2 * math.pi) - math.pi
        for k in range(seg + 1):
            a = a1 + delta * k / seg
            pts.append((c[0] + r * math.cos(a), c[1] + r * math.sin(a)))
    return pts


def rounded_polygon_svg_d(verts) -> str:
    """Filleted polygon → SVG path d attribute (arcs)."""
    cmds = []
    for i in range(len(verts)):
        t1, t2, c, r = _fillet(verts, i)
        a1 = math.atan2(t1[1] - c[1], t1[0] - c[0])
        a2 = math.atan2(t2[1] - c[1], t2[0] - c[0])
        delta = (a2 - a1 + math.pi) % (2 * math.pi) - math.pi
        sweep = 1 if delta > 0 else 0
        start = "M" if i == 0 else "L"
        cmds.append(
            f"{start}{t1[0]:.2f} {t1[1]:.2f} "
            f"A{r:.2f} {r:.2f} 0 0 {sweep} {t2[0]:.2f} {t2[1]:.2f}"
        )
    return " ".join(cmds) + " Z"


# ═════════════════════════════════════════════════════════════════════════════
# SVG
# ═════════════════════════════════════════════════════════════════════════════

def mark_svg_paths(color: str, scale: float = 100, dx: float = 0, dy: float = 0) -> str:
    d = " ".join(
        rounded_polygon_svg_d(verts) for verts in mark_brackets(dx, dy, scale)
    )
    return f'<path d="{d}" fill="{color}"/>'


def write_mark_svg(path: Path, color: str):
    svg = (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">\n'
        f"  {mark_svg_paths(color)}\n"
        "</svg>\n"
    )
    path.write_text(svg)


def wordmark_glyph_paths():
    """Outlines of "Helm" (SF Pro Semibold) in font units, Y pointing up.

    Returns (list of (path_d, x_offset), l_height, x_min, x_max)."""
    from fontTools.misc.transform import Transform
    from fontTools.pens.boundsPen import BoundsPen
    from fontTools.pens.svgPathPen import SVGPathPen
    from fontTools.pens.transformPen import TransformPen
    from fontTools.ttLib import TTFont
    from fontTools.varLib.instancer import instantiateVariableFont

    font = TTFont(SFNS)
    inst = next(
        i
        for i in font["fvar"].instances
        if font["name"].getDebugName(i.subfamilyNameID) == "Semibold"
    )
    instantiateVariableFont(font, dict(inst.coordinates), inplace=True)

    cmap = font.getBestCmap()
    glyph_set = font.getGlyphSet()
    hmtx = font["hmtx"]

    glyphs, x = [], 0.0
    bounds = {}
    for ch in "Helm":
        gname = cmap[ord(ch)]
        bp = BoundsPen(glyph_set)
        glyph_set[gname].draw(bp)
        bounds[ch] = bp.bounds  # (xMin, yMin, xMax, yMax)
        glyphs.append((gname, x))
        x += hmtx[gname][0]

    l_height = bounds["l"][3]
    x_min = bounds["H"][0]
    x_max = glyphs[-1][1] + bounds["m"][2]

    paths = []
    for gname, gx in glyphs:
        pen = SVGPathPen(glyph_set, ntos=lambda v: f"{v:.1f}")
        glyph_set[gname].draw(TransformPen(pen, Transform().translate(gx, 0)))
        paths.append(pen.getCommands())
    return paths, l_height, x_min, x_max


def write_logo_svg(path: Path, color: str):
    """Horizontal lockup: mark (100×100) + "Helm", baseline = bottom of the mark,
    top of the "l" = top of the mark (proportions measured on the sheet)."""
    paths, l_height, x_min, x_max = wordmark_glyph_paths()
    s = 100.0 / l_height
    text_x = 100 + LOCKUP_GAP * 100 - x_min * s
    width = text_x + x_max * s

    text = "".join(
        f'  <path transform="translate({text_x:.1f} 100) scale({s:.5f} -{s:.5f})" '
        f'd="{d}" fill="{color}"/>\n'
        for d in paths
    )
    svg = (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.0f} 100">\n'
        f"  {mark_svg_paths(color)}\n"
        f"{text}"
        "</svg>\n"
    )
    path.write_text(svg)


def write_favicon_svg(path: Path):
    rad = TILE_RADIUS * 100
    mark = MARK_IN_TILE * 100
    off = (100 - mark) / 2
    svg = (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">\n'
        f'  <rect width="100" height="100" rx="{rad:g}" fill="{BG}"/>\n'
        f"  {mark_svg_paths(FG, scale=mark, dx=off, dy=off)}\n"
        "</svg>\n"
    )
    path.write_text(svg)


# ═════════════════════════════════════════════════════════════════════════════
# PIL rendering
# ═════════════════════════════════════════════════════════════════════════════

def draw_mark(d: ImageDraw.ImageDraw, x: float, y: float, size: float, color):
    for verts in mark_brackets(x, y, size):
        d.polygon(rounded_polygon_points(verts), fill=color)


def tile_image(px: int, margin_frac: float = 0.0, rim: bool = True) -> Image.Image:
    """Icon tile: dark rounded square + white mark, supersampled."""
    ss = 8 if px <= 64 else 4
    S = px * ss
    m = margin_frac * S
    tile = S - 2 * m
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([m, m, S - m - 1, S - m - 1], radius=TILE_RADIUS * tile, fill=BG)
    if rim and px > 32:
        d.rounded_rectangle(
            [m, m, S - m - 1, S - m - 1],
            radius=TILE_RADIUS * tile,
            outline=(255, 255, 255, 22),
            width=ss,
        )
    mark_frac = MARK_IN_TILE + (0.06 if px <= 32 else 0)
    mark = mark_frac * tile
    off = m + (tile - mark) / 2
    draw_mark(d, off, off, mark, FG)
    return img.resize((px, px), Image.LANCZOS)


def sf_font(size: int, weight: str = "Semibold") -> ImageFont.FreeTypeFont:
    f = ImageFont.truetype(SFNS, size)
    f.set_variation_by_name(weight)
    return f


def draw_wordmark(d: ImageDraw.ImageDraw, x: float, top: float, height: float,
                  color, text: str = "Helm", weight: str = "Semibold") -> float:
    """Draws `text` with an exact ink height `height`, left edge at x. Returns the
    ink width."""
    trial = sf_font(int(height * 1.2), weight)
    bb = d.textbbox((0, 0), text, font=trial)
    size = int(round(height * 1.2 * height / (bb[3] - bb[1])))
    f = sf_font(size, weight)
    bb = d.textbbox((0, 0), text, font=f)
    d.text((x - bb[0], top - bb[1]), text, font=f, fill=color)
    return bb[2] - bb[0]


def measure_wordmark(text: str, height: float, weight: str = "Semibold") -> float:
    img = Image.new("RGB", (8, 8))
    d = ImageDraw.Draw(img)
    trial = sf_font(int(height * 1.2), weight)
    bb = d.textbbox((0, 0), text, font=trial)
    size = int(round(height * 1.2 * height / (bb[3] - bb[1])))
    f = sf_font(size, weight)
    bb = d.textbbox((0, 0), text, font=f)
    return bb[2] - bb[0]


def social_preview(path: Path):
    """1200×630: #0A0A0A background, violet glows, centered lockup. Rendered at 2×."""
    W, H = 2400, 1260
    base = np.full((H, W, 3), 10.0)
    yy, xx = np.mgrid[0:H, 0:W].astype(float)
    glows = [
        ((1.00 * W, 1.35 * H), 0.376 * W, 0.74),  # bottom-right corner, dominant
        ((0.50 * W, -0.45 * H), 0.30 * W, 0.16),  # top-center, subtle
    ]
    for (cx, cy), radius, strength in glows:
        dist = np.hypot(xx - cx, yy - cy)
        falloff = np.exp(-((dist / radius) ** 2)) * strength
        for ch in range(3):
            base[..., ch] += GLOW_VIOLET[ch] * falloff
    img = Image.fromarray(np.clip(base, 0, 255).astype(np.uint8))
    d = ImageDraw.Draw(img)

    mark_h = 200
    gap = LOCKUP_GAP * mark_h
    text_w = measure_wordmark("Helm", mark_h)
    total = mark_h + gap + text_w
    x0 = (W - total) / 2
    y0 = (H - mark_h) / 2
    draw_mark(d, x0, y0, mark_h, FG)
    draw_wordmark(d, x0 + mark_h + gap, y0, mark_h, FG)
    img.resize((1200, 630), Image.LANCZOS).save(path)


def splash(path: Path):
    """2000×1200: black background, mark + "Helm" + violet tagline. Rendered at 2×."""
    W, H = 4000, 2400
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)

    mark_h, gap1, text_h, gap2, tag_h = 440, 170, 340, 150, 88
    block = mark_h + gap1 + text_h + gap2 + tag_h
    y = (H - block) / 2

    draw_mark(d, (W - mark_h) / 2, y, mark_h, FG)
    y += mark_h + gap1
    text_w = measure_wordmark("Helm", text_h)
    draw_wordmark(d, (W - text_w) / 2, y, text_h, FG)
    y += text_h + gap2
    tag = "Stay at the Helm."
    tag_w = measure_wordmark(tag, tag_h, "Medium")
    draw_wordmark(d, (W - tag_w) / 2, y, tag_h, ACCENT_PURPLE, tag, "Medium")
    img.resize((2000, 1200), Image.LANCZOS).save(path)


# ═════════════════════════════════════════════════════════════════════════════
# Icon containers
# ═════════════════════════════════════════════════════════════════════════════

def make_icns(path: Path):
    """iconset with the Apple margin (824/1024 content) → iconutil."""
    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / "icon.iconset"
        iconset.mkdir()
        by_px = {px: tile_image(px, margin_frac=APPLE_MARGIN) for px in
                 (16, 32, 64, 128, 256, 512, 1024)}
        names = {
            "icon_16x16.png": 16, "icon_16x16@2x.png": 32,
            "icon_32x32.png": 32, "icon_32x32@2x.png": 64,
            "icon_128x128.png": 128, "icon_128x128@2x.png": 256,
            "icon_256x256.png": 256, "icon_256x256@2x.png": 512,
            "icon_512x512.png": 512, "icon_512x512@2x.png": 1024,
        }
        for name, px in names.items():
            by_px[px].save(iconset / name)
        subprocess.run(
            ["iconutil", "-c", "icns", str(iconset), "-o", str(path)], check=True
        )


def make_ico(path: Path, sizes: tuple):
    imgs = [tile_image(px) for px in sizes]
    imgs[-1].save(
        path,
        format="ICO",
        sizes=[(i.width, i.height) for i in imgs],
        append_images=imgs[:-1],
    )


# ═════════════════════════════════════════════════════════════════════════════

def main():
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "icons").mkdir(exist_ok=True)

    write_mark_svg(OUT / "helm-mark.svg", FG)
    write_mark_svg(OUT / "helm-mark-white.svg", FG)
    write_mark_svg(OUT / "helm-mark-black.svg", "#0A0A0A")
    write_logo_svg(OUT / "helm-logo.svg", FG)
    write_logo_svg(OUT / "helm-logo-black.svg", "#0A0A0A")
    write_favicon_svg(OUT / "favicon.svg")

    for px in (16, 32, 64, 128, 256, 512, 1024):
        tile_image(px).save(OUT / "icons" / f"icon-{px}.png")

    tile_image(512).save(OUT / "github-avatar.png")
    # Runtime dock icon (src/app.rs): Apple margin so it sits at the same
    # visual size as neighbouring dock icons.
    tile_image(512, margin_frac=APPLE_MARGIN).save(OUT / "icons" / "icon-dock-512.png")
    social_preview(OUT / "github-social-preview.png")
    splash(OUT / "splash.png")

    make_icns(OUT / "icon.icns")
    make_ico(OUT / "icon.ico", (16, 24, 32, 48, 64, 128, 256))
    make_ico(OUT / "favicon.ico", (16, 32, 64, 128, 256))

    for f in sorted(OUT.rglob("*")):
        if f.is_file():
            print(f"{f.relative_to(ROOT)}  ({f.stat().st_size:,} o)")


if __name__ == "__main__":
    main()
