# Bundled Unicode fallback fonts

Source: official Google Fonts repository, pinned commit `6c1dbe27dcf7d817d4b62cec033ba77fa8e2a5ec`.

- `NotoEmoji-Variable.ttf` (1982596 bytes), SHA-256 `de6c18832938afc99caf132b39d6a30a19bac7f2e812e28db2535b4608d27551`
  - https://raw.githubusercontent.com/google/fonts/6c1dbe27dcf7d817d4b62cec033ba77fa8e2a5ec/ofl/notoemoji/NotoEmoji%5Bwght%5D.ttf
- `NotoEmoji-OFL.txt` (4330 bytes), SHA-256 `500bb1ccf43df7bbb522112f9133a52b16e1c35e809632f5d8609b179152de5b`
  - https://raw.githubusercontent.com/google/fonts/6c1dbe27dcf7d817d4b62cec033ba77fa8e2a5ec/ofl/notoemoji/OFL.txt

Noto Emoji is the monochrome TrueType outline variable font, not Noto Color Emoji (CBDT/CBLC). Font bytes are unmodified. They are licensed under SIL OFL 1.1; the complete upstream license notices are included alongside the fonts.

Runtime fallback keeps existing sans-serif text and Lucide icons before this font. The app does not normalize or rewrite stored Unicode text. Color emoji, ZWJ ligature shaping and skin-tone/flag composition are not provided by egui 0.29 character-by-character layout.

The inline renderer shapes whole graphemes through the optional macOS EmojiPreview helper for color. The font remains the bounded monochrome fallback while that worker result is pending or unavailable.
