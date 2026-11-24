"""Draws the tray icons: four states, each as a macOS template (black on transparent) and as a
colour tile for Windows and Linux. The states differ by shape, never by colour alone.

    python3 scripts/tray_icons.py   # writes app/src-tauri/icons/tray/*.png
"""
from pathlib import Path
from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "app/src-tauri/icons/tray"
S = 64          # output size
K = 8           # supersampling
W = S * K
LIGHT, TILE = (236, 241, 233, 255), (36, 48, 42, 255)
ALARM, WARN = (214, 58, 47, 255), (232, 160, 40, 255)

# The sprawlr mark: a path through four nodes (same shape as the app icon).
PATH = [(0.17, 0.70), (0.38, 0.30), (0.55, 0.58), (0.76, 0.20)]


def mark(d, colour, scale, dashed=False):
    pts = [(x * W * scale + W * 0.02, y * W * scale + W * 0.04) for x, y in PATH]
    lw, r = int(W * 0.075), int(W * 0.075)
    for a, b in zip(pts, pts[1:]):
        if dashed:
            steps = 6
            for i in range(steps):
                if i % 2:
                    continue
                t0, t1 = i / steps, (i + 1) / steps
                p0 = (a[0] + (b[0] - a[0]) * t0, a[1] + (b[1] - a[1]) * t0)
                p1 = (a[0] + (b[0] - a[0]) * t1, a[1] + (b[1] - a[1]) * t1)
                d.line([p0, p1], fill=colour, width=lw)
        else:
            d.line([a, b], fill=colour, width=lw)
    for x, y in pts:
        if dashed:
            d.ellipse([x - r, y - r, x + r, y + r], outline=colour, width=int(W * 0.035), fill=(0, 0, 0, 0))
        else:
            d.ellipse([x - r, y - r, x + r, y + r], fill=colour)


def badge(img, level, fill, cut):
    """Bottom-right badge; `cut` erases a margin around it so it reads apart from the mark."""
    d = ImageDraw.Draw(img)
    cx, cy, R = W * 0.74, W * 0.74, W * 0.25
    d.ellipse([cx - R - W * 0.05, cy - R - W * 0.05, cx + R + W * 0.05, cy + R + W * 0.05], fill=cut)
    if level == "act_now":
        tri = [(cx, cy - R), (cx + R * 1.05, cy + R * 0.85), (cx - R * 1.05, cy + R * 0.85)]
        d.polygon(tri, fill=fill)
        bar_w = W * 0.05
        hole = (0, 0, 0, 0) if cut[3] == 0 else cut
        d.rounded_rectangle([cx - bar_w / 2, cy - R * 0.45, cx + bar_w / 2, cy + R * 0.3], radius=bar_w / 2, fill=hole)
        d.ellipse([cx - bar_w / 2, cy + R * 0.45, cx + bar_w / 2, cy + R * 0.45 + bar_w], fill=hole)
    elif level == "plan_for_it":
        d.ellipse([cx - R * 0.8, cy - R * 0.8, cx + R * 0.8, cy + R * 0.8], outline=fill, width=int(W * 0.075))


def draw(level, template):
    img = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    if template:
        fg, cut = (0, 0, 0, 255), (0, 0, 0, 0)
    else:
        d.rounded_rectangle([0, 0, W - 1, W - 1], radius=W * 0.22, fill=TILE)
        fg, cut = LIGHT, TILE
    scale = 1.0 if level in ("all_clear", "no_status") else 0.82
    mark(d, fg, scale, dashed=(level == "no_status"))
    if level in ("act_now", "plan_for_it"):
        colour = fg if template else (ALARM if level == "act_now" else WARN)
        badge(img, level, colour, cut)
    return img.resize((S, S), Image.LANCZOS)


if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True)
    for level in ("act_now", "plan_for_it", "all_clear", "no_status"):
        draw(level, True).save(OUT / f"{level}-template.png")
        draw(level, False).save(OUT / f"{level}.png")
    print("wrote", sorted(p.name for p in OUT.iterdir()))
