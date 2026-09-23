"""Generates assets/icon.ico (exe/installer) and assets/tray-32.rgba (raw 32x32 RGBA for the tray).

Run once with `python tools/make_icon.py`; the outputs are committed. Needs Pillow.
"""
from pathlib import Path

from PIL import Image, ImageDraw


def draw(size: int) -> Image.Image:
    s = size * 4  # supersample, then downscale for smooth edges
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, s - 1, s - 1], radius=s // 5, fill=(17, 24, 39, 255))
    d.line(
        [(s * 0.27, s * 0.52), (s * 0.44, s * 0.69), (s * 0.75, s * 0.33)],
        fill=(59, 130, 246, 255),
        width=max(4, s // 10),
        joint="curve",
    )
    return img.resize((size, size), Image.LANCZOS)


out = Path(__file__).resolve().parent.parent / "assets"
out.mkdir(exist_ok=True)
draw(256).save(out / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
(out / "tray-32.rgba").write_bytes(draw(32).tobytes())
