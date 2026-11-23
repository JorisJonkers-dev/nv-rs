// SPDX-License-Identifier: GPL-3.0-only
//! The panel's font: a TrueType file made at start from the port's own 5 by 7 bitmap glyphs
//! (`d3d9_wgpu::bitmap_font`, the F11 counter's), so the overlay needs no font file and no font
//! licence beyond the engine's own. Each lit pixel is a square of the outline; runs of lit pixels are
//! merged into rectangles. At a text size that is a whole multiple of the cell (8, 16, 24 pixels)
//! every square falls on whole pixels and the text is as sharp as the counter's.
//!
//! The file has the tables a TrueType reader needs (`cmap`, `glyf`, `head`, `hhea`, `hmtx`, `loca`,
//! `maxp`, `name`, `OS/2`, `post`); glyph 0 is a hollow box, drawn for every character outside
//! printable ASCII.

/// Rows of one glyph, top first: 7 rows of 5 columns, bit 4 the leftmost.
pub type Glyph = [u8; 7];

/// Font units of one bitmap pixel; the em is the cell's 8 rows.
const PIXEL: i16 = 128;
const UNITS_PER_EM: u16 = 8 * PIXEL as u16;
/// The cell: 6 columns (5 drawn and one of space) by 8 rows (7 drawn, the baseline under the 7th).
const ADVANCE: u16 = 6 * PIXEL as u16;
const ASCENT: i16 = 7 * PIXEL;
const DESCENT: i16 = -PIXEL;

/// The rectangles (column, row, width, height in pixels; row 0 at the top) that cover the lit
/// pixels of `g`: runs of a row, merged with the same run in the rows below.
pub fn rectangles(g: &Glyph) -> Vec<(u8, u8, u8, u8)> {
    let mut used = [[false; 5]; 7];
    let lit = |r: usize, c: usize| g[r] & (0b10000 >> c) != 0;
    let mut out = Vec::new();
    for r in 0..7 {
        let mut c = 0;
        while c < 5 {
            if !lit(r, c) || used[r][c] {
                c += 1;
                continue;
            }
            let mut w = 1;
            while c + w < 5 && lit(r, c + w) && !used[r][c + w] {
                w += 1;
            }
            let mut h = 1;
            while r + h < 7 && (c..c + w).all(|x| lit(r + h, x) && !used[r + h][x]) {
                h += 1;
            }
            for row in used.iter_mut().skip(r).take(h) {
                for u in row.iter_mut().skip(c).take(w) {
                    *u = true;
                }
            }
            out.push((c as u8, r as u8, w as u8, h as u8));
            c += w;
        }
    }
    out
}

/// One contour in font units, clockwise (TrueType's outer direction, y up).
type Contour = [(i16, i16); 4];

fn rect_contour(x0: i16, y0: i16, x1: i16, y1: i16, clockwise: bool) -> Contour {
    if clockwise {
        [(x0, y0), (x0, y1), (x1, y1), (x1, y0)]
    } else {
        [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    }
}

/// A pixel rectangle of the cell in font units: x from the left, y up from the baseline.
fn pixel_rect(c: u8, r: u8, w: u8, h: u8) -> Contour {
    let x0 = i16::from(c) * PIXEL;
    let x1 = i16::from(c + w) * PIXEL;
    let y1 = (7 - i16::from(r)) * PIXEL;
    let y0 = (7 - i16::from(r + h)) * PIXEL;
    rect_contour(x0, y0, x1, y1, true)
}

/// A simple glyph's `glyf` data (empty for a glyph without contours).
fn glyph_data(contours: &[Contour]) -> Vec<u8> {
    if contours.is_empty() {
        return Vec::new();
    }
    let pts = contours.iter().flatten();
    let (x_min, x_max) = (
        pts.clone().map(|p| p.0).min().unwrap_or(0),
        pts.clone().map(|p| p.0).max().unwrap_or(0),
    );
    let (y_min, y_max) = (
        pts.clone().map(|p| p.1).min().unwrap_or(0),
        pts.clone().map(|p| p.1).max().unwrap_or(0),
    );
    let mut d = Vec::new();
    put16(&mut d, contours.len() as u16);
    for v in [x_min, y_min, x_max, y_max] {
        put16(&mut d, v as u16);
    }
    for i in 0..contours.len() {
        put16(&mut d, (i * 4 + 3) as u16);
    }
    put16(&mut d, 0); // no instructions
                      // Every point on the curve, each coordinate a 16-bit delta.
    d.extend(std::iter::repeat_n(1u8, contours.len() * 4));
    let mut last = 0i16;
    for p in pts.clone() {
        put16(&mut d, (p.0 - last) as u16);
        last = p.0;
    }
    last = 0;
    for p in pts {
        put16(&mut d, (p.1 - last) as u16);
        last = p.1;
    }
    while d.len() % 4 != 0 {
        d.push(0);
    }
    d
}

fn put16(d: &mut Vec<u8>, v: u16) {
    d.extend_from_slice(&v.to_be_bytes());
}

fn put32(d: &mut Vec<u8>, v: u32) {
    d.extend_from_slice(&v.to_be_bytes());
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, c| {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        sum.wrapping_add(u32::from_be_bytes(w))
    })
}

/// The first character `ascii` holds (space), and its count (to `~`).
const FIRST: u16 = 0x20;

/// A TrueType file of `ascii` (characters 0x20 to 0x7e, in order).
pub fn truetype(ascii: &[Glyph; 95]) -> Vec<u8> {
    // Glyph 0: a hollow box over the cell's 5 by 7.
    let notdef = vec![
        rect_contour(0, 0, 5 * PIXEL, 7 * PIXEL, true),
        rect_contour(PIXEL, PIXEL, 4 * PIXEL, 6 * PIXEL, false),
    ];
    let mut glyphs: Vec<Vec<Contour>> = vec![notdef];
    for g in ascii {
        glyphs.push(
            rectangles(g)
                .into_iter()
                .map(|(c, r, w, h)| pixel_rect(c, r, w, h))
                .collect(),
        );
    }
    let count = glyphs.len() as u16;
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for g in &glyphs {
        put32(&mut loca, glyf.len() as u32);
        glyf.extend(glyph_data(g));
    }
    put32(&mut loca, glyf.len() as u32);
    let max_contours = glyphs.iter().map(Vec::len).max().unwrap_or(0) as u16;

    let mut head = Vec::new();
    put32(&mut head, 0x0001_0000);
    put32(&mut head, 0x0001_0000); // font revision 1.0
    put32(&mut head, 0); // checksum adjustment, set below
    put32(&mut head, 0x5F0F_3CF5);
    put16(&mut head, 0x000B); // baseline at y 0, left side bearing at x 0, integer scaling
    put16(&mut head, UNITS_PER_EM);
    head.extend_from_slice(&[0; 16]); // created, modified
    for v in [0, DESCENT, 5 * PIXEL, ASCENT] {
        put16(&mut head, v as u16);
    }
    put16(&mut head, 0); // mac style
    put16(&mut head, 8); // smallest readable size in pixels
    put16(&mut head, 2); // font direction hint
    put16(&mut head, 1); // long offsets in loca
    put16(&mut head, 0);

    let mut hhea = Vec::new();
    put32(&mut hhea, 0x0001_0000);
    for v in [ASCENT, DESCENT, 0] {
        put16(&mut hhea, v as u16);
    }
    put16(&mut hhea, ADVANCE);
    for v in [0, PIXEL, 5 * PIXEL] {
        put16(&mut hhea, v as u16); // min left and right side bearing, x max extent
    }
    for v in [1, 0, 0, 0, 0, 0, 0] {
        put16(&mut hhea, v); // caret upright (rise, run, offset); four reserved
    }
    put16(&mut hhea, 0);
    put16(&mut hhea, count);

    // Each glyph's left side bearing is its outline's left edge (a reader places the outline by it).
    let mut hmtx = Vec::new();
    for g in &glyphs {
        put16(&mut hmtx, ADVANCE);
        let x_min = g.iter().flatten().map(|p| p.0).min().unwrap_or(0);
        put16(&mut hmtx, x_min as u16);
    }

    let mut maxp = Vec::new();
    put32(&mut maxp, 0x0001_0000);
    put16(&mut maxp, count);
    put16(&mut maxp, max_contours * 4);
    put16(&mut maxp, max_contours);
    for v in [0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0] {
        put16(&mut maxp, v);
    }

    // cmap: one Windows Unicode subtable, format 4: one segment for printable ASCII (glyph
    // = character - 0x1f), and the closing one.
    let mut sub = Vec::new();
    put16(&mut sub, 4);
    put16(&mut sub, 32); // length: 14 + 2 segments of 8 bytes + 2
    put16(&mut sub, 0);
    for v in [4, 4, 1, 0] {
        put16(&mut sub, v); // 2 x segments, search range, entry selector, range shift
    }
    put16(&mut sub, FIRST + 94);
    put16(&mut sub, 0xFFFF);
    put16(&mut sub, 0);
    put16(&mut sub, FIRST);
    put16(&mut sub, 0xFFFF);
    put16(&mut sub, 1u16.wrapping_sub(FIRST)); // id delta
    put16(&mut sub, 1);
    put16(&mut sub, 0);
    put16(&mut sub, 0); // id range offsets
    let mut cmap = Vec::new();
    put16(&mut cmap, 0);
    put16(&mut cmap, 1);
    put16(&mut cmap, 3);
    put16(&mut cmap, 1);
    put32(&mut cmap, 12);
    cmap.extend(sub);

    let mut os2 = Vec::new();
    put16(&mut os2, 4);
    put16(&mut os2, ADVANCE);
    put16(&mut os2, 400);
    put16(&mut os2, 5);
    put16(&mut os2, 0); // fs type: installable
    os2.extend_from_slice(&[0; 20]); // sub- and superscript, strikeout
    put16(&mut os2, 0); // family class
    os2.extend_from_slice(&[0; 10]); // panose
    put32(&mut os2, 1); // Basic Latin
    os2.extend_from_slice(&[0; 12]);
    os2.extend_from_slice(b"NVRS");
    put16(&mut os2, 0x40 | 0x80); // regular, use the typographic metrics
    put16(&mut os2, FIRST);
    put16(&mut os2, FIRST + 94);
    for v in [ASCENT, DESCENT, 0] {
        put16(&mut os2, v as u16);
    }
    put16(&mut os2, ASCENT as u16);
    put16(&mut os2, (-DESCENT) as u16);
    put32(&mut os2, 1); // Latin 1
    put32(&mut os2, 0);
    for v in [5 * PIXEL, 7 * PIXEL, 0, 0x20, 1] {
        put16(&mut os2, v as u16); // x height, cap height, default and break character, context
    }

    let mut post = Vec::new();
    put32(&mut post, 0x0003_0000);
    put32(&mut post, 0);
    put16(&mut post, (-PIXEL) as u16);
    put16(&mut post, PIXEL as u16);
    put32(&mut post, 1); // fixed pitch
    post.extend_from_slice(&[0; 16]);

    let name = name_table(&[(1, "nv-rs overlay"), (2, "Regular"), (4, "nv-rs overlay")]);

    let mut tables: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"OS/2", os2),
        (b"cmap", cmap),
        (b"glyf", glyf),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca),
        (b"maxp", maxp),
        (b"name", name),
        (b"post", post),
    ];
    tables.sort_by_key(|t| *t.0);
    let n = tables.len() as u16;
    let mut out = Vec::new();
    put32(&mut out, 0x0001_0000);
    put16(&mut out, n);
    let pow = 1u16 << (15 - n.leading_zeros() as u16);
    put16(&mut out, pow * 16);
    put16(&mut out, pow.trailing_zeros() as u16);
    put16(&mut out, n * 16 - pow * 16);
    let mut offset = 12 + 16 * u32::from(n);
    let mut head_at = 0;
    for (tag, data) in &tables {
        if *tag == b"head" {
            head_at = offset as usize;
        }
        out.extend_from_slice(*tag);
        put32(&mut out, checksum(data));
        put32(&mut out, offset);
        put32(&mut out, data.len() as u32);
        offset += (data.len() as u32).div_ceil(4) * 4;
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    let adjust = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    out[head_at + 8..head_at + 12].copy_from_slice(&adjust.to_be_bytes());
    out
}

/// A `name` table of Windows Unicode English records.
fn name_table(names: &[(u16, &str)]) -> Vec<u8> {
    let strings: Vec<Vec<u8>> = names
        .iter()
        .map(|(_, s)| s.encode_utf16().flat_map(u16::to_be_bytes).collect())
        .collect();
    let mut d = Vec::new();
    put16(&mut d, 0);
    put16(&mut d, names.len() as u16);
    put16(&mut d, 6 + 12 * names.len() as u16);
    let mut at = 0u16;
    for ((id, _), s) in names.iter().zip(&strings) {
        for v in [3, 1, 0x409, *id, s.len() as u16, at] {
            put16(&mut d, v);
        }
        at += s.len() as u16;
    }
    for s in strings {
        d.extend(s);
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles_cover_every_lit_pixel_once() {
        let g: Glyph = [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ];
        let rs = rectangles(&g);
        let mut seen = [[0u8; 5]; 7];
        for (c, r, w, h) in &rs {
            for row in seen.iter_mut().skip(*r as usize).take(*h as usize) {
                for s in row.iter_mut().skip(*c as usize).take(*w as usize) {
                    *s += 1;
                }
            }
        }
        for (r, row) in seen.iter().enumerate() {
            for (c, s) in row.iter().enumerate() {
                let lit = g[r] & (0b10000 >> c) != 0;
                assert_eq!(*s, u8::from(lit), "row {r} column {c}");
            }
        }
        // The two sides of 'A' are one rectangle each, through the bar.
        assert!(
            rs.contains(&(0, 1, 1, 6)) && rs.contains(&(4, 1, 1, 6)),
            "{rs:?}"
        );
        assert_eq!(rs.len(), 4);
    }

    #[test]
    fn the_file_has_its_tables_in_order_and_a_whole_checksum() {
        let mut ascii = [[0u8; 7]; 95];
        ascii[b'A' as usize - 0x20] = [0b11111; 7];
        let f = truetype(&ascii);
        assert_eq!(&f[..4], &[0, 1, 0, 0]);
        let n = u16::from_be_bytes([f[4], f[5]]) as usize;
        let tags: Vec<&[u8]> = (0..n).map(|i| &f[12 + 16 * i..16 + 16 * i]).collect();
        let mut sorted = tags.clone();
        sorted.sort();
        assert_eq!(tags, sorted);
        assert!(tags.contains(&&b"glyf"[..]) && tags.contains(&&b"cmap"[..]));
        // Each table's length as the format has it.
        let len = |tag: &[u8]| {
            let i = tags.iter().position(|t| *t == tag).unwrap();
            u32::from_be_bytes(f[24 + 16 * i..28 + 16 * i].try_into().unwrap())
        };
        assert_eq!(
            [
                len(b"head"),
                len(b"hhea"),
                len(b"maxp"),
                len(b"OS/2"),
                len(b"post")
            ],
            [54, 36, 32, 96, 32]
        );
        assert_eq!(len(b"hmtx"), 96 * 4);
        assert_eq!(checksum(&f), 0xB1B0_AFBA);
    }
}
