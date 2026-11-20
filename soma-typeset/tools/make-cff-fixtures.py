#!/usr/bin/env python3
"""Build the two CFF fixtures used by the PDF backend's tests.

`tiny-cff.otf` is CID-keyed (ROS `Adobe-Identity-0`, FDArray/FDSelect), the
shape Noto CJK ships in. `tiny-cff-name.otf` is name-keyed (charset of SIDs,
Private DICT on the Top DICT), the shape most Latin OTFs ship in.

Both are 6 glyphs, 1000 upem: `.notdef`, `A` (500 wide), `B` (600),
`中` (1000 em box), and two empty glyphs so a subset has something to blank.

    python3 tools/make-cff-fixtures.py \
        crates/soma-render-pdf/tests/fixtures/tiny-cff.otf \
        crates/soma-render-pdf/tests/fixtures/tiny-cff-name.otf
"""

import struct
import sys


def dict_int(value):
    if -107 <= value <= 107:
        return bytes([value + 139])
    if 108 <= value <= 1131:
        value -= 108
        return bytes([247 + value // 256, value % 256])
    if -1131 <= value <= -108:
        value = -value - 108
        return bytes([251 + value // 256, value % 256])
    if -32768 <= value <= 32767:
        return b"\x1c" + struct.pack(">h", value)
    return b"\x1d" + struct.pack(">i", value)


def op_bytes(op):
    if isinstance(op, tuple):
        return bytes([12, op[1]])
    return bytes([op])


def dict_bytes(entries):
    out = b""
    for op, args in entries:
        for arg in args:
            out += dict_int(arg)
        out += op_bytes(op)
    return out


def cs_int(value):
    if -107 <= value <= 107:
        return bytes([value + 139])
    if 108 <= value <= 1131:
        value -= 108
        return bytes([247 + value // 256, value % 256])
    if -1131 <= value <= -108:
        value = -value - 108
        return bytes([251 + value // 256, value % 256])
    return b"\x1c" + struct.pack(">h", value)


def index(items):
    if not items:
        return b"\x00\x00"
    offsets = [1]
    for item in items:
        offsets.append(offsets[-1] + len(item))
    total = offsets[-1]
    if total <= 0xFF:
        size = 1
    elif total <= 0xFFFF:
        size = 2
    elif total <= 0xFFFFFF:
        size = 3
    else:
        size = 4
    out = struct.pack(">H", len(items)) + bytes([size])
    for offset in offsets:
        out += offset.to_bytes(size, "big")
    return out + b"".join(items)


def rect(width, height):
    out = cs_int(0) + cs_int(0) + b"\x15"  # rmoveto
    for dx, dy in ((width, 0), (0, height), (-width, 0)):
        out += cs_int(dx) + cs_int(dy) + b"\x05"  # rlineto
    return out + b"\x09\x0e"  # closepath, endchar


GLYPHS = [b"\x0e", rect(500, 700), rect(600, 700), rect(1000, 800), b"\x0e", b"\x0e"]


def cff(cid_keyed):
    header = b"\x01\x00\x04\x04"
    name = index([b"SomaTest"])
    strings = index([b"Adobe", b"Identity"] if cid_keyed else [])
    gsubrs = index([])
    charset = b"\x00" + b"".join(struct.pack(">H", g) for g in range(len(GLYPHS)))
    fdselect = (
        b"\x03"
        + struct.pack(">H", 1)
        + struct.pack(">H", 0)
        + b"\x00"
        + struct.pack(">H", len(GLYPHS))
    )
    charstrings = index(GLYPHS)
    private = dict_bytes([(20, [0]), (21, [0])])

    offsets = {}
    topdict = b""
    # Iterate to a fixed point: an offset growing from 1 to 2 bytes changes the
    # DICT length, which changes the offsets.
    for _ in range(8):
        before = dict(offsets)
        if cid_keyed:
            entries = [
                ((12, 30), [391, 392, 0]),
                (5, [-100, -200, 1100, 900]),
                (15, [offsets.get("charset", 0)]),
                (17, [offsets.get("charstrings", 0)]),
                ((12, 36), [offsets.get("fdarray", 0)]),
                ((12, 37), [offsets.get("fdselect", 0)]),
                ((12, 34), [len(GLYPHS)]),
            ]
        else:
            entries = [
                (5, [-100, -200, 1100, 900]),
                (15, [offsets.get("charset", 0)]),
                (17, [offsets.get("charstrings", 0)]),
                (18, [len(private), offsets.get("private", 0)]),
            ]
        fdarray = (
            index([dict_bytes([(18, [len(private), offsets.get("private", 0)])])])
            if cid_keyed
            else b""
        )
        topdict = index([dict_bytes(entries)])
        position = len(header) + len(name) + len(topdict) + len(strings) + len(gsubrs)
        offsets = {"charset": position}
        position += len(charset)
        offsets["fdselect"] = position
        if cid_keyed:
            position += len(fdselect)
        offsets["charstrings"] = position
        position += len(charstrings)
        if cid_keyed:
            offsets["fdarray"] = position
            position += len(fdarray)
        offsets["private"] = position
        if offsets == before:
            break

    body = header + name + topdict + strings + gsubrs + charset
    if cid_keyed:
        body += fdselect
    body += charstrings
    if cid_keyed:
        body += fdarray
    body += private
    return body


# A name-keyed face whose third glyph is a `seac` composite: "A" plus an
# accent box shifted by (25, 10). The accent calls a local subroutine, so the
# Type 2 interpreter has to follow one to read the composite.
SEAC_ADVANCES = [500, 500, 500, 500, 500, 600]
SEAC_GLYPHS = [
    b"\x0e",  # .notdef
    rect(500, 700),  # A
    cs_int(200) + cs_int(700) + b"\x15" + cs_int(-107) + b"\x0a\x0e",  # acute
    cs_int(500)  # width
    + cs_int(25)  # adx
    + cs_int(10)  # ady
    + cs_int(65)  # bchar: "A"
    + cs_int(194)  # achar: "acute"
    + b"\x0e",
    b"\x0e",  # unused
    rect(600, 700),  # B
]
# Subroutine 0: the accent's outline, in absolute coordinates from its origin.
SEAC_SUBRS = [cs_int(100) + cs_int(0) + b"\x05" + cs_int(0) + cs_int(100) + b"\x05"
              + cs_int(-100) + cs_int(0) + b"\x05" + b"\x09\x0b"]


def cff_seac():
    glyphs = SEAC_GLYPHS
    header = b"\x01\x00\x04\x04"
    name = index([b"SomaSeac"])
    strings = index([b"Aacute"])
    gsubrs = index([])
    # GID: 0 .notdef, 1 "A", 2 "acute", 3 "Aacute" (custom string), 4 space, 5 "B".
    sids = [0, 34, 125, 391, 1, 35]
    charset = b"\x00" + b"".join(struct.pack(">H", sid) for sid in sids[1:])
    charstrings = index(glyphs)
    subrs = index(SEAC_SUBRS)

    offsets = {}
    private = b""
    for _ in range(8):
        before = dict(offsets)
        private = dict_bytes([(20, [0]), (21, [0]), (19, [offsets.get("subrs", 0)])])
        entries = [
            (5, [-100, -200, 1100, 900]),
            (15, [offsets.get("charset", 0)]),
            (17, [offsets.get("charstrings", 0)]),
            (18, [len(private), offsets.get("private", 0)]),
        ]
        topdict = index([dict_bytes(entries)])
        position = len(header) + len(name) + len(topdict) + len(strings) + len(gsubrs)
        offsets = {"charset": position}
        position += len(charset)
        offsets["charstrings"] = position
        position += len(charstrings)
        offsets["private"] = position
        position += len(private)
        offsets["subrs"] = position - offsets["private"]
        if offsets == before:
            break

    return header + name + topdict + strings + gsubrs + charset + charstrings + private + subrs


def sfnt(cff_data, num_glyphs, upem=1000, advances=None, segments=None):
    head = bytearray(54)
    struct.pack_into(">I", head, 0, 0x00010000)
    struct.pack_into(">I", head, 12, 0x5F0F3CF5)
    struct.pack_into(">H", head, 18, upem)
    struct.pack_into(">h", head, 36, -100)
    struct.pack_into(">h", head, 38, -200)
    struct.pack_into(">h", head, 40, 1100)
    struct.pack_into(">h", head, 42, 900)
    struct.pack_into(">h", head, 50, 0)

    maxp = struct.pack(">I", 0x00005000) + struct.pack(">H", num_glyphs)

    advances = (advances or [500, 500, 600, 1000, 500, 500])[:num_glyphs]
    hhea = bytearray(36)
    struct.pack_into(">I", hhea, 0, 0x00010000)
    struct.pack_into(">h", hhea, 4, 880)
    struct.pack_into(">h", hhea, 6, -120)
    struct.pack_into(">H", hhea, 34, num_glyphs)
    hmtx = b"".join(struct.pack(">Hh", advance, 0) for advance in advances)

    segments = segments or [
        (0x41, 0x41, 1),
        (0x42, 0x42, 2),
        (0x4E2D, 0x4E2D, 3),
        (0xFFFF, 0xFFFF, 0),
    ]
    ends = b"".join(struct.pack(">H", s[1]) for s in segments)
    starts = b"".join(struct.pack(">H", s[0]) for s in segments)
    deltas = b"".join(struct.pack(">H", (s[2] - s[0]) & 0xFFFF) for s in segments)
    ranges = b"".join(struct.pack(">H", 0) for _ in segments)
    sub = (
        struct.pack(">HHHH", 2 * len(segments), 0, 0, 0)
        + ends
        + b"\x00\x00"
        + starts
        + deltas
        + ranges
    )
    cmap = struct.pack(">HH", 0, 1) + struct.pack(">HHI", 3, 1, 12) + struct.pack(
        ">HHH", 4, 0, 0
    ) + sub

    tables = {
        b"CFF ": cff_data,
        b"cmap": cmap,
        b"head": bytes(head),
        b"hhea": bytes(hhea),
        b"hmtx": hmtx,
        b"maxp": maxp,
    }
    tags = sorted(tables)
    out = struct.pack(">IHHHH", 0x4F54544F, len(tags), 0, 0, 0)
    offset = 12 + 16 * len(tags)
    records = b""
    body = b""
    for tag in tags:
        data = tables[tag]
        records += tag + struct.pack(">III", 0, offset + len(body), len(data))
        body += data + b"\x00" * ((4 - len(data) % 4) % 4)
    return out + records + body


def main():
    open(sys.argv[1], "wb").write(sfnt(cff(True), len(GLYPHS)))
    open(sys.argv[2], "wb").write(sfnt(cff(False), len(GLYPHS)))
    if len(sys.argv) > 3:
        open(sys.argv[3], "wb").write(
            sfnt(
                cff_seac(),
                len(SEAC_GLYPHS),
                advances=SEAC_ADVANCES,
                segments=[(0x41, 0x41, 1), (0xC1, 0xC1, 3), (0x42, 0x42, 5), (0xFFFF, 0xFFFF, 0)],
            )
        )


if __name__ == "__main__":
    main()
