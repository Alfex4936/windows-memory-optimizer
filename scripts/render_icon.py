"""Rebuild Memori's vector mark, PNG, and multiresolution Windows icon (Pillow)."""
from pathlib import Path

from PIL import Image, ImageDraw, PngImagePlugin


BACKGROUND = "#d2f780"
FOREGROUND = "#0d0f14"


def main():
    assets = Path(__file__).resolve().parent.parent / "assets"
    assets.mkdir(exist_ok=True)
    scale = 4
    image = Image.new("RGBA", (256 * scale, 256 * scale))
    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle(
        (8 * scale, 8 * scale, 248 * scale, 248 * scale),
        radius=64 * scale, fill=BACKGROUND,
    )
    bars = [(71, 135, 22, 60), (117, 65, 22, 130), (163, 101, 22, 94)]
    for x, y, width, height in bars:
        draw.rounded_rectangle(
            (x * scale, y * scale, (x + width) * scale, (y + height) * scale),
            radius=11 * scale, fill=FOREGROUND,
        )
    image = image.resize((256, 256), Image.Resampling.LANCZOS)
    metadata = PngImagePlugin.PngInfo()
    metadata.add_text("Source", "assets/icon.svg; rendered by scripts/render_icon.py")
    metadata.add_text("License", "MIT")
    image.save(assets / "icon.png", optimize=True, pnginfo=metadata)
    image.save(assets / "memori.ico", sizes=[(s, s) for s in [16, 20, 24, 32, 40, 48, 64, 128, 256]])
    rects = "\n".join(
        f'    <rect x="{x}" y="{y}" width="{w}" height="{h}" rx="11"/>'
        for x, y, w, h in bars
    )
    (assets / "icon.svg").write_text(
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" role="img" aria-labelledby="title">\n'
        '  <title id="title">Memori</title>\n'
        f'  <rect x="8" y="8" width="240" height="240" rx="64" fill="{BACKGROUND}"/>\n'
        f'  <g fill="{FOREGROUND}">\n{rects}\n  </g>\n</svg>\n',
        encoding="utf-8",
    )
    with Image.open(assets / "memori.ico") as icon:
        assert len(icon.ico.sizes()) == 9
    assert image.getpixel((0, 0))[3] == 0
    print("Created SVG, PNG, and ICO (16-256 px).")


if __name__ == "__main__":
    main()
