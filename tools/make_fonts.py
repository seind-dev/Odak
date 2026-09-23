"""Builds the bundled fonts in assets/fonts/ (run once; the outputs are committed).

- Manrope (UI text) and Unbounded (headings): downloaded from the Google Fonts repository (OFL),
  cut into static weights and subset to Latin + Turkish.
- Flaticon UIcons, regular rounded (icons): downloaded from the @flaticon/flaticon-uicons npm
  package and subset to the glyphs referenced in src/ui/icons.rs. Free with attribution
  ("Uicons by Flaticon"), which the Settings page shows.

Needs `pip install fonttools brotli`. Usage: python tools/make_fonts.py
"""
import io
import re
import tarfile
import urllib.request
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "fonts"
GOOGLE = "https://github.com/google/fonts/raw/main/ofl"
UICONS = "https://registry.npmjs.org/@flaticon/flaticon-uicons/-/flaticon-uicons-3.3.1.tgz"
# Latin, Latin-1, Latin Extended-A/B (Turkish), general punctuation, arrows, minus sign.
TEXT_UNICODES = "U+0000-024F,U+2000-206F,U+20AC,U+2122,U+2190-2193,U+2212"
TEXT_FONTS = {
    "Manrope": ("manrope/Manrope%5Bwght%5D.ttf", {"Regular": 400, "Medium": 500, "SemiBold": 600, "Bold": 700}),
    "Unbounded": ("unbounded/Unbounded%5Bwght%5D.ttf", {"SemiBold": 600, "Bold": 700}),
}


def download(url: str) -> bytes:
    with urllib.request.urlopen(url) as response:
        return response.read()


def subset_font(font: TTFont, unicodes: list[int], path: Path) -> None:
    options = subset.Options()
    options.name_IDs = ["*"]
    options.layout_features = ["*"]
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=unicodes)
    subsetter.subset(font)
    font.flavor = None
    font.save(path)
    print(f"{path.relative_to(ROOT)}: {path.stat().st_size // 1024} KB")


def text_fonts() -> None:
    unicodes = subset.parse_unicodes(TEXT_UNICODES)
    for family, (source, weights) in TEXT_FONTS.items():
        variable = download(f"{GOOGLE}/{source}")
        # The OFL requires its text to travel with the (modified) font files.
        (OUT / f"{family}-OFL.txt").write_bytes(download(f"{GOOGLE}/{source.split('/')[0]}/OFL.txt"))
        for style, weight in weights.items():
            font = instancer.instantiateVariableFont(TTFont(io.BytesIO(variable)), {"wght": weight}, updateFontNames=True)
            subset_font(font, unicodes, OUT / f"{family}-{style}.ttf")


def icon_font() -> None:
    icons_rs = (ROOT / "src" / "ui" / "icons.rs").read_text(encoding="utf-8")
    codepoints = sorted({int(cp, 16) for cp in re.findall(r"\\u\{([0-9a-fA-F]+)\}", icons_rs)})
    with tarfile.open(fileobj=io.BytesIO(download(UICONS))) as tar:
        member = next(m for m in tar.getmembers() if re.search(r"css/uicons-regular-rounded-\w+\.woff2$", m.name))
        woff2 = tar.extractfile(member).read()
    subset_font(TTFont(io.BytesIO(woff2)), codepoints, OUT / "uicons-regular-rounded.ttf")


if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True)
    text_fonts()
    icon_font()
