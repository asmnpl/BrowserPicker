"""Generates the app and tray icons. Run: python3 scripts/gen_icons.py (needs Pillow)."""
import math
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter

OUT = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
OUT.mkdir(parents=True, exist_ok=True)
S = 1024 * 4  # supersampled canvas


def lerp(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def draw_glyph(d, cx, cy, scale, color):
    """A stem that forks into two arrows: "one link, many browsers"."""
    w = int(64 * scale)
    base = (cx, cy + 250 * scale)
    fork = (cx, cy + 20 * scale)
    left = (cx - 190 * scale, cy - 200 * scale)
    right = (cx + 190 * scale, cy - 200 * scale)
    d.line([base, fork], fill=color, width=w)
    for tip in (left, right):
        d.line([fork, tip], fill=color, width=w)
        ang = math.atan2(tip[1] - fork[1], tip[0] - fork[0])
        size = 120 * scale
        pts = [
            (tip[0] + math.cos(ang) * size * 0.55, tip[1] + math.sin(ang) * size * 0.55),
            (tip[0] + math.cos(ang + 2.4) * size, tip[1] + math.sin(ang + 2.4) * size),
            (tip[0] + math.cos(ang - 2.4) * size, tip[1] + math.sin(ang - 2.4) * size),
        ]
        d.polygon(pts, fill=color)
    r = w / 2
    for p in (base, fork):
        d.ellipse([p[0] - r, p[1] - r, p[0] + r, p[1] + r], fill=color)
    r = 70 * scale
    d.ellipse([base[0] - r, base[1] - r, base[0] + r, base[1] + r], fill=color)


def app_icon():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    k = S / 1024
    inset, radius = 100 * k, 185 * k
    box = [inset, inset, S - inset, S - inset]

    shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle(
        [box[0], box[1] + 12 * k, box[2], box[3] + 12 * k], radius, fill=(0, 0, 0, 90))
    img = Image.alpha_composite(img, shadow.filter(ImageFilter.GaussianBlur(18 * k)))

    grad = Image.new("RGBA", (S, S))
    top, bottom = (94, 182, 255), (48, 86, 240)
    gd = ImageDraw.Draw(grad)
    for y in range(S):
        gd.line([(0, y), (S, y)], fill=lerp(top, bottom, y / S) + (255,))
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle(box, radius, fill=255)
    img.paste(grad, (0, 0), mask)

    glyph = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    draw_glyph(ImageDraw.Draw(glyph), S / 2, S / 2, k, (255, 255, 255, 255))
    img = Image.alpha_composite(img, glyph)
    return img.resize((1024, 1024), Image.LANCZOS)


def tray_icon(color):
    n = 44 * 16
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    draw_glyph(ImageDraw.Draw(img), n / 2, n / 2 + 10, n / 540, color)
    return img.resize((44, 44), Image.LANCZOS)


icon = app_icon()
icon.save(OUT / "icon.png")
for size, name in [(32, "32x32.png"), (128, "128x128.png"), (256, "128x128@2x.png")]:
    icon.resize((size, size), Image.LANCZOS).save(OUT / name)
icon.save(OUT / "icon.icns")
icon.save(OUT / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (256, 256)])
tray_icon((0, 0, 0, 255)).save(OUT / "tray-template.png")
icon.resize((64, 64), Image.LANCZOS).save(OUT / "tray.png")
print("icons written to", OUT)
