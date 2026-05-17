"""Generate NSIS installer banners from the master Lucy icon.

Outputs (BMPv3 24-bit, no alpha — NSIS requirement):
  - installer-sidebar.bmp   164 x 314   (Welcome / Finish pages, left strip)
  - installer-header.bmp    150 x  57   (top-right of every other page)

Design:
  - Dark background #060a0f (matches Lucy window backgroundColor)
  - Sidebar: large centered icon (~140 px) over background, soft radial glow,
    "Lucy Assistant" wordmark + tagline at the bottom.
  - Header:  small icon (40 px) at the left, "Lucy Assistant" wordmark right.

Re-run any time the master icon changes.
"""
from PIL import Image, ImageDraw, ImageFont, ImageFilter
from pathlib import Path

ROOT     = Path(__file__).resolve().parent.parent  # src-tauri/icons
SRC      = ROOT / "source" / "lucy-icon-1024.png"
SIDEBAR  = ROOT / "installer-sidebar.bmp"
HEADER   = ROOT / "installer-header.bmp"

BG       = (6, 10, 15)        # #060a0f
ACCENT   = (96, 165, 250)     # cyan-blue used in Lucy UI
FG       = (220, 230, 240)
DIM      = (140, 155, 170)


def load_font(size: int) -> ImageFont.FreeTypeFont:
    """Best-effort font load — falls back to default if Segoe/Inter missing."""
    for name in ("seguisb.ttf", "segoeui.ttf", "arial.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()


def radial_glow(size: tuple[int, int], center: tuple[int, int], radius: int,
                color: tuple[int, int, int], opacity: int = 90) -> Image.Image:
    """Soft circular glow, used behind the icon for depth."""
    w, h = size
    glow = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    draw = ImageDraw.Draw(glow)
    cx, cy = center
    draw.ellipse((cx - radius, cy - radius, cx + radius, cy + radius),
                 fill=(*color, opacity))
    return glow.filter(ImageFilter.GaussianBlur(radius * 0.45))


def build_sidebar() -> None:
    W, H = 164, 314
    img = Image.new("RGB", (W, H), BG)

    # Soft glow behind icon
    glow = radial_glow((W, H), (W // 2, 110), 90, ACCENT, opacity=80)
    img.paste(glow, (0, 0), glow)

    # Icon, ~120px centered upper-third
    icon = Image.open(SRC).convert("RGBA").resize((120, 120), Image.LANCZOS)
    img.paste(icon, ((W - 120) // 2, 50), icon)

    draw = ImageDraw.Draw(img)
    f_title = load_font(15)
    f_tag   = load_font(10)

    title = "Lucy Assistant"
    tw = draw.textlength(title, font=f_title)
    draw.text(((W - tw) // 2, 200), title, font=f_title, fill=FG)

    tag = "Autonomous SysAdmin AI"
    tw = draw.textlength(tag, font=f_tag)
    draw.text(((W - tw) // 2, 222), tag, font=f_tag, fill=DIM)

    # Accent underline
    draw.line([(W // 2 - 20, 244), (W // 2 + 20, 244)], fill=ACCENT, width=1)

    # Version slot at the very bottom
    ver = "v1.4.0"
    vw = draw.textlength(ver, font=f_tag)
    draw.text(((W - vw) // 2, H - 22), ver, font=f_tag, fill=DIM)

    # BMP v3 24-bit — NSIS rejects v5/alpha
    img.save(SIDEBAR, format="BMP")
    print(f"wrote {SIDEBAR.relative_to(ROOT.parent)}  ({W}x{H})")


def build_header() -> None:
    W, H = 150, 57
    img = Image.new("RGB", (W, H), BG)

    # Small icon on the left
    icon = Image.open(SRC).convert("RGBA").resize((40, 40), Image.LANCZOS)
    img.paste(icon, (8, (H - 40) // 2), icon)

    draw  = ImageDraw.Draw(img)
    f_t   = load_font(11)
    f_sub = load_font(8)

    draw.text((56, 14), "Lucy",           font=f_t,   fill=FG)
    draw.text((56, 30), "Assistant",      font=f_sub, fill=DIM)
    draw.text((56, 42), "v1.4.0",         font=f_sub, fill=ACCENT)

    img.save(HEADER, format="BMP")
    print(f"wrote {HEADER.relative_to(ROOT.parent)}  ({W}x{H})")


if __name__ == "__main__":
    build_sidebar()
    build_header()
