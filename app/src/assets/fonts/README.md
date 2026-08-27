# Bundled fonts

Four files, four jobs. All four are cut down from larger originals — a font
dropped in at full size adds megabytes to the installer, and nobody notices
until someone complains about the download.

Everything here is bundled rather than named in a font stack, because a font
that is only installed on some machines is a layout that is only right on some
machines. The system faces still listed behind them in `app.css` cover a
character none of these files carry; they are the safety net, not the plan.

Requires `fonttools` and `brotli` to rebuild any of it:

```bash
pip install fonttools brotli
```

## outfit-latin.woff2 — the interface

**Outfit**, Latin subset, from Google Fonts. SIL Open Font License 1.1.

Geometric and monoline: circular bowls, even stroke weight, terminals cut clean.
One variable file across the whole weight range the app uses, so 500 is a real
500 and nothing is synthesised.

32 KB. Taken from the Google Fonts CSS for `Outfit:wght@300..700`, the
`unicode-range: U+0000-00FF…` block — that is the Latin subset, already cut.

## comfortaa-latin.woff2 — headings and the app's name

**Comfortaa**, Latin subset, from Google Fonts. SIL Open Font License 1.1.

Scoped to `--font-display`, which only `h1` and the title bar use. The letters
are drawn unusually on purpose — the `g` finishes in a full hook, the `r` curls
rather than turning a corner — and that is exactly what makes a name memorable
at 36px and exhausting at 13px. It must not reach body text.

23 KB, same subset method as Outfit.

## huninn.woff2 — every Chinese character

**jf open 粉圓 2.1** by justfont — <https://github.com/justfont/open-huninn-font>
SIL Open Font License 1.1.

Round-terminal gothic: strokes finish in a curve, counters run open, the weight
sits high in the frame. Same vocabulary as Comfortaa, which is the reason for
it — a round Latin beside a conventional Chinese gothic reads as two decisions
rather than one. Drawn in Taiwan, so the character shapes are the Traditional
ones.

2.04 MB, the whole face — 11,988 glyphs, **not** a common-character subset.
Server names, MOTDs and player names are typed by the user, so no subset covers
what they might enter, and a missing character falls back to the system gothic
as one visibly different word.

```bash
python -m fontTools.subset jf-openhuninn-2.1.ttf \
  --output-file=huninn.woff2 \
  --flavor=woff2 \
  --unicodes='*' \
  --layout-features='' \
  --name-IDs=''
```

Nothing is dropped except layout features and name records; the flags above are
a TTF-to-woff2 conversion, not a cut.

### One weight only

The face ships a single weight (400). `app.css` sets `font-synthesis-weight:
none` on `body` because of it: without that the browser thickens Chinese
wherever the design asks for 500 or 600, and a synthesised stroke does not match
a real one — some Chinese looked heavy and some looked thin on the same screen.

Outfit and Comfortaa are unaffected by that rule. They carry real weights, so
nothing was ever being synthesised for them; Latin still goes to 500 and 600.

Chinese hierarchy therefore comes from size and colour. That is what the rest of
the design already runs on, so it costs nothing — but if a heading ever needs to
be louder, **make it bigger, do not make it bolder.** The weight will not land.

The alternative was 源泉圓體 GenSenRounded2 TW, the same round vocabulary with
real weights, measured at 6.7 MB per weight as woff2 — 13.6 MB for the two this
app would need, against 2 MB for the whole of 粉圓. It buys bold text in five
elements. It was not worth it.

## plex-mono-latin.woff2 — the console only

**IBM Plex Mono**, Latin subset, from Google Fonts. SIL Open Font License 1.1.

Log output is aligned by column — timestamps, levels, the `key=value` lines of
`server.properties`. A proportional face there tilts every column. Plex Mono is
a grotesque rather than a geometric, which is the point: machine output should
not look like it was set by the same hand as the interface.

Chinese inside a monospace run still falls through to 粉圓, which is
proportional. That is correct — a Chinese comment in a config file is prose, and
only the keys and values need to line up.

15 KB.
