#!/usr/bin/env python3
"""Build Phosphor icon fonts that answer to the Lucide code points the GUI already uses.

The GUI draws icons as glyphs at the private-use code points listed in `src/gui/theme.rs`
(`pub mod lucide`). This script copies the matching Phosphor glyphs (MIT, phosphoricons.com)
into a new font at exactly those code points, scaled and centred like the Lucide glyphs, so an
icon style can be switched by loading a different font, with no change at the call sites.

Usage:
    python3 packaging/icons/build-phosphor-fonts.py <phosphor-web package dir>
Needs fontTools. Writes src/assets/fonts/phosphor-{regular,bold,fill}.ttf.
The package dir is the unpacked npm tarball @phosphor-icons/web (tested with 2.1.1).
"""

import pathlib
import re
import sys

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont

ROOT = pathlib.Path(__file__).resolve().parents[2]

# Lucide constant name -> Phosphor icon name.
PHOSPHOR_NAME = {
    "SEARCH": "magnifying-glass", "REFRESH": "arrows-clockwise", "SMILE": "smiley",
    "SEND": "paper-plane-tilt", "SUN": "sun", "MOON": "moon", "PHONE": "phone",
    "PHONE_CALL": "phone-call", "PHONE_OFF": "phone-x", "VIDEO": "video-camera",
    "VIDEO_OFF": "video-camera-slash", "CAMERA": "camera", "IMAGE": "image",
    "PAPERCLIP": "paperclip", "CHECK": "check", "CHECK_CHECK": "checks",
    "MESSAGE_SQUARE": "chat-circle", "USERS": "users", "USER_PLUS": "user-plus",
    "CLOUD": "cloud", "SETTINGS": "gear-six", "PANEL_RIGHT": "sidebar-simple", "PIN": "push-pin",
    "COPY": "copy", "STAR": "star", "GLOBE": "globe", "LINK": "link",
    "EXTERNAL_LINK": "arrow-square-out", "X": "x", "PLUS": "plus", "MINUS": "minus",
    "STORE": "storefront", "FLAME": "fire", "SPARKLES": "sparkle", "BAN": "prohibit",
    "MIC": "microphone", "MIC_OFF": "microphone-slash", "LOG_OUT": "sign-out",
    "FILE_TEXT": "file-text", "FILM": "film-strip", "SHIELD_CHECK": "shield-check",
    "LOCK": "lock", "BELL": "bell", "PAINTBRUSH": "paint-brush", "SLIDERS": "sliders-horizontal",
    "FOLDER_OPEN": "folder-open", "CHEVRON_RIGHT": "caret-right", "REPLY": "arrow-bend-up-left",
    "FORWARD": "arrow-bend-up-right", "LIST_CHECKS": "list-checks", "INFO": "info",
    "TRASH": "trash", "ELLIPSIS": "dots-three", "UNDO": "arrow-counter-clockwise",
    "DOWNLOAD": "download-simple", "CIRCLE": "circle", "PENCIL": "pencil-simple",
    "STICKER": "sticker", "CLOCK": "clock", "FILE": "file", "CHEVRON_DOWN": "caret-down",
    "USER": "user", "NEWSPAPER": "newspaper",
}

WEIGHTS = {
    "regular": ("regular/Phosphor.ttf", "regular/style.css"),
    "bold": ("bold/Phosphor-Bold.ttf", "bold/style.css"),
    "fill": ("fill/Phosphor-Fill.ttf", "fill/style.css"),
}


def lucide_code_points():
    src = (ROOT / "src/gui/theme.rs").read_text()
    body = src[src.index("pub mod lucide {"):]
    body = body[: body.index("\n}\n")]
    return {
        name: int(cp, 16)
        for name, cp in re.findall(r'pub const (\w+): &str = "\\u\{([0-9a-fA-F]+)\}"', body)
    }


def css_code_points(css_path):
    css = css_path.read_text()
    return {
        name: int(cp, 16)
        for name, cp in re.findall(r'\.ph-([a-z0-9-]+):before\s*\{\s*content:\s*"\\([0-9a-fA-F]+)"', css)
    }


def bounds(glyphset, name):
    pen = BoundsPen(glyphset)
    glyphset[name].draw(pen)
    return pen.bounds


def build(package, weight, lucide_font, lucide_cps):
    ttf_rel, css_rel = WEIGHTS[weight]
    src = TTFont(package / "src" / ttf_rel)
    src_cmap = src.getBestCmap()
    src_glyphs = src.getGlyphSet()
    css = css_code_points(package / "src" / css_rel)
    l_upm = lucide_font["head"].unitsPerEm
    p_upm = src["head"].unitsPerEm
    l_glyphs = lucide_font.getGlyphSet()
    l_cmap = lucide_font.getBestCmap()

    # Phosphor draws on a 256 grid with ~24 units of padding; Lucide on 24 with 2. Match their
    # circles: same diameter and same centre in the output em.
    lb = bounds(l_glyphs, l_cmap[lucide_cps["CIRCLE"]])
    pb = bounds(src_glyphs, src_cmap[css["circle"]])
    # Phosphor's wide glyphs (video, users) run past Lucide's box at equal circles; 0.92 keeps
    # them inside while circles stay close to Lucide's size.
    scale = 0.92 * (lb[2] - lb[0]) / (pb[2] - pb[0])
    dx = (lb[0] + lb[2]) / 2 - scale * (pb[0] + pb[2]) / 2
    dy = (lb[1] + lb[3]) / 2 - scale * (pb[1] + pb[3]) / 2

    order = [".notdef"]
    glyphs = {".notdef": TTGlyphPen(None).glyph()}
    metrics = {".notdef": (l_upm, 0)}
    cmap = {}
    missing = []
    for const, cp in sorted(lucide_cps.items()):
        pname = PHOSPHOR_NAME.get(const)
        if pname is None or pname not in css or css[pname] not in src_cmap:
            missing.append(f"{const}->{pname}")
            continue
        gname = f"icon_{const.lower()}"
        tt = TTGlyphPen(None)
        src_glyphs[src_cmap[css[pname]]].draw(
            TransformPen(Cu2QuPen(tt, max_err=1.0, reverse_direction=False),
                         (scale, 0, 0, scale, dx, dy)))
        order.append(gname)
        glyphs[gname] = tt.glyph()
        adv = lucide_font["hmtx"][l_cmap[cp]][0] if cp in l_cmap else l_upm
        metrics[gname] = (adv, 0)
        cmap[cp] = gname
    if missing:
        sys.exit(f"{weight}: no Phosphor glyph for {', '.join(missing)}")

    fb = FontBuilder(l_upm, isTTF=True)
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap(cmap)
    fb.setupGlyf(glyphs)
    for g in order:
        if hasattr(glyphs[g], "recalcBounds"):
            glyphs[g].recalcBounds(fb.font["glyf"])
    fb.setupHorizontalMetrics({g: (metrics[g][0], getattr(glyphs[g], "xMin", 0)) for g in order})
    hhea = lucide_font["hhea"]
    fb.setupHorizontalHeader(ascent=hhea.ascent, descent=hhea.descent)
    family = f"ctdozlite Phosphor {weight.title()}"
    fb.setupNameTable({"familyName": family, "styleName": "Regular",
                       "copyright": "Phosphor Icons (MIT), remapped for ctdozlite"})
    os2 = lucide_font["OS/2"]
    fb.setupOS2(sTypoAscender=os2.sTypoAscender, sTypoDescender=os2.sTypoDescender,
                usWinAscent=os2.usWinAscent, usWinDescent=os2.usWinDescent)
    fb.setupPost()
    out = ROOT / f"src/assets/fonts/phosphor-{weight}.ttf"
    fb.save(out)
    print(f"{out.relative_to(ROOT)}: {len(cmap)} icons, scale {scale:.4f}")


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    package = pathlib.Path(sys.argv[1])
    lucide_font = TTFont(ROOT / "src/assets/fonts/lucide.ttf")
    lucide_cps = lucide_code_points()
    for weight in WEIGHTS:
        build(package, weight, lucide_font, lucide_cps)


if __name__ == "__main__":
    main()
