#!/usr/bin/env python3
"""Print a font's vertical OpenType MATH variants.

    python3 tools/math-variants-report.py NotoSansMath-Regular.ttf 2211 221a

The variant glyphs are usually unencoded, so this prints the design metrics
(horizontal advance, ink box, coverage) that `tools/metrics-gen` records in a
`soma.metrics/6` table. Used for the font survey in ADR-0075.
"""

import struct, sys

U16 = lambda b, o: struct.unpack('>H', b[o:o+2])[0]
I16 = lambda b, o: struct.unpack('>h', b[o:o+2])[0]
U32 = lambda b, o: struct.unpack('>I', b[o:o+4])[0]

def tables(data):
    num = U16(data, 4)
    out = {}
    for i in range(num):
        off = 12 + 16*i
        tag = data[off:off+4].decode('latin1')
        out[tag] = (U32(data, off+8), U32(data, off+12))
    return out

def cmap_map(data, tabs):
    c = tabs['cmap'][0]
    n = U16(data, c+2)
    best = None
    for i in range(n):
        p, e, o = U16(data, c+4+8*i), U16(data, c+6+8*i), U32(data, c+8+8*i)
        fmt = U16(data, c+o)
        rank = {(3,10,12): 4, (3,1,4): 3, (0,3,4): 2, (0,4,12): 2}.get((p,e,fmt), 0)
        if rank and (best is None or rank > best[0]):
            best = (rank, c+o, fmt)
    g2c = {}
    c2g = {}
    _, off, fmt = best
    if fmt == 12:
        ng = U32(data, off+12)
        for i in range(ng):
            s, e, g = U32(data, off+16+12*i), U32(data, off+20+12*i), U32(data, off+24+12*i)
            for cp in range(s, e+1):
                g2c.setdefault(g+cp-s, []).append(cp)
                c2g.setdefault(cp, g+cp-s)
    else:
        seg = U16(data, off+6)//2
        end = off+14; start = end+seg*2+2; delta = start+seg*2; ro = delta+seg*2
        for i in range(seg):
            en, st = U16(data, end+2*i), U16(data, start+2*i)
            d, r = struct.unpack('>h', data[delta+2*i:delta+2*i+2])[0], U16(data, ro+2*i)
            for cp in range(st, en+1):
                if cp == 0xffff: continue
                if r == 0: g = (cp+d) & 0xffff
                else:
                    a = ro+2*i+r+2*(cp-st)
                    v = U16(data, a)
                    g = 0 if v == 0 else (v+d) & 0xffff
                if g:
                    g2c.setdefault(g, []).append(cp)
                    c2g.setdefault(cp, g)
    return g2c, c2g

def coverage(data, off):
    fmt = U16(data, off)
    gids = []
    if fmt == 1:
        n = U16(data, off+2)
        gids = [U16(data, off+4+2*i) for i in range(n)]
    elif fmt == 2:
        n = U16(data, off+2)
        for i in range(n):
            start, end, idx = U16(data, off+4+6*i), U16(data, off+6+6*i), U16(data, off+8+6*i)
            gids.extend(range(start, end+1))
    else:
        raise SystemExit('coverage fmt %d' % fmt)
    return gids

def ink_box(data, tabs, gid):
    if 'glyf' not in tabs: return None
    head = tabs['head'][0]
    loca = tabs['loca'][0]; glyf = tabs['glyf'][0]
    if I16(data, head+50) != 0:
        a, b = U32(data, loca+gid*4), U32(data, loca+gid*4+4)
    else:
        a, b = U16(data, loca+gid*2)*2, U16(data, loca+gid*2+2)*2
    if a >= b: return None
    o = glyf+a
    n = I16(data, o)
    if n == 0: return None
    return (I16(data, o+2), I16(data, o+4), I16(data, o+6), I16(data, o+8))

def main(path, cps):
    data = open(path, 'rb').read()
    tabs = tables(data)
    if 'MATH' not in tabs:
        print(path, 'no MATH'); return
    upem = U16(data, tabs['head'][0]+18)
    g2c, c2g = cmap_map(data, tabs)
    m = tabs['MATH'][0]
    var = U16(data, m+8)
    if not var:
        print('no variants'); return
    v = m + var
    minov, vcov, hcov, vcount, hcount = struct.unpack('>HHHHH', data[v:v+10])
    print(path, 'upem', upem, 'minOverlap', minov, 'vcount', vcount, 'hcount', hcount)
    bases = coverage(data, v+vcov)
    constructions = {}
    for k, g in enumerate(bases):
        co = U16(data, v+10+2*k)
        gc = v+co
        asm_off, nvar = U16(data, gc), U16(data, gc+2)
        variants = [struct.unpack('>HH', data[gc+4+4*i:gc+8+4*i]) for i in range(nvar)]
        assembly = None
        if asm_off:
            a = gc+asm_off
            ital = I16(data, a)
            pc = U16(data, a+2)
            parts = []
            for i in range(pc):
                gg, sc, ec, fa, flags = struct.unpack('>HHHHH', data[a+4+10*i:a+14+10*i])
                parts.append((gg, sc, ec, fa, flags))
            assembly = (ital, parts)
        constructions[g] = (variants, assembly)
    for cp in cps:
        g = c2g.get(cp)
        print(' cp U+%04X -> gid %s' % (cp, g))
        if g is None or g not in constructions: 
            print('   no construction')
            continue
        variants, assembly = constructions[g]
        print('   variants:')
        for vg, adv in variants:
            box = ink_box(data, tabs, vg)
            print('     gid %5d cps %-20s adv %5d box %s' % (vg, g2c.get(vg), adv, box))
        if assembly:
            ital, parts = assembly
            print('   assembly ital %d parts:' % ital)
            for gg, sc, ec, fa, flags in parts:
                print('     gid %5d cps %-20s start %4d end %4d full %4d ext %d box %s' % (gg, g2c.get(gg), sc, ec, fa, flags & 1, ink_box(data, tabs, gg)))

main(sys.argv[1], [int(x, 16) for x in sys.argv[2:]])
