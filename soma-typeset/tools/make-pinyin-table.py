#!/usr/bin/env python3
"""Generate the pinyin collation table used by `soma-collate`.

The engine itself carries no dictionary: this script asks ICU for the Han
reading of every code point in the two Han blocks the metrics table covers,
strips the tone marks, ranks the distinct syllables alphabetically, and writes
a compact `(u16 code point, u16 rank)` table that the crate binary-searches.

    python3 tools/make-pinyin-table.py > crates/soma-collate/pinyin.bin

ICU is the source of the readings (`uconv -x Han-Latin`), so regenerating needs
`uconv` from libicu. The output is checked in: the engine must be reproducible
without it.
"""

import struct
import subprocess
import sys
import unicodedata

RANGES = [(0x3400, 0x4DBF), (0x4E00, 0x9FFF)]
def readings(chars: list[str]) -> list[str]:
    """One reading per line, so alignment survives any odd translation."""
    out = subprocess.run(
        ["uconv", "-x", "Han-Latin"],
        input="\n".join(chars) + "\n",
        capture_output=True,
        check=True,
        text=True,
    ).stdout.splitlines()
    if len(out) != len(chars):
        sys.exit(f"uconv returned {len(out)} lines for {len(chars)} characters")
    return out


def plain(syllable: str) -> str:
    """Drop the tone marks: `zhōng` -> `zhong`."""
    decomposed = unicodedata.normalize("NFD", syllable)
    return "".join(c for c in decomposed if not unicodedata.combining(c))


def main() -> None:
    table: list[tuple[int, str]] = []
    for start, end in RANGES:
        chars = [chr(c) for c in range(start, end + 1)]
        for char, line in zip(chars, readings(chars)):
            syllable = line.split()[0] if line.split() else ""
            table.append((ord(char), plain(syllable).lower()))

    distinct = sorted({syllable for _, syllable in table if syllable})
    rank = {syllable: index for index, syllable in enumerate(distinct)}
    if len(distinct) > 0xFFFF:
        sys.exit("too many syllables for a u16 rank")

    out = bytearray()
    for code, syllable in table:
        if code > 0xFFFF:
            sys.exit("code point does not fit a u16")
        out += struct.pack("<HH", code, rank.get(syllable, 0xFFFF))

    sys.stdout.buffer.write(bytes(out))
    print(
        f"{len(table)} code points, {len(distinct)} syllables, {len(out)} bytes",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
