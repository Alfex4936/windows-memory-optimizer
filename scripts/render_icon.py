"""Package the transparent Memori mark as PNG and a multi-size Windows ICO."""

import argparse
from pathlib import Path

from PIL import Image


def main():
    assets = Path(__file__).resolve().parent.parent / "assets"
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", nargs="?", type=Path, default=assets / "icon.png")
    args = parser.parse_args()

    with Image.open(args.source) as source:
        if source.width != source.height or "A" not in source.getbands():
            parser.error("source must be a square image with an alpha channel")
        icon = source.convert("RGBA").resize((256, 256), Image.Resampling.LANCZOS)

    alpha = icon.getchannel("A")
    corners = [(0, 0), (0, 255), (255, 0), (255, 255)]
    if alpha.getextrema()[1] < 240 or any(alpha.getpixel(point) for point in corners):
        parser.error("source must have transparent corners and a visible mark")

    sizes = [(side, side) for side in (16, 20, 24, 32, 40, 48, 64, 128, 256)]
    icon.save(assets / "icon.png", optimize=True)
    icon.save(assets / "memori.ico", sizes=sizes)
    with Image.open(assets / "memori.ico") as windows_icon:
        assert windows_icon.ico.sizes() == set(sizes)
    print("Packaged Memori PNG and ICO (16-256 px).")


if __name__ == "__main__":
    main()
