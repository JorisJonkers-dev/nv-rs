// SPDX-License-Identifier: GPL-3.0-only
//! The pictures, in the look of upstream's `docs/media/progress.svg`: a dark panel, amber bars,
//! monospace text. Every coordinate is an integer computed from the counts, so the same counts
//! give the same bytes.

pub const BG: &str = "#14110b";
pub const TRACK: &str = "#2b2418";
pub const EDGE: &str = "#5c4a26";
pub const TITLE: &str = "#f2c14e";
pub const TEXT: &str = "#d9c9a3";
pub const MUTED: &str = "#8c7d5c";
/// Bar segments, brightest first: done, part-way, the third kind.
pub const SEG: [&str; 3] = ["#f2c14e", "#a77d2a", "#6e5420"];
const FONT: &str = "Consolas, 'Courier New', monospace";
const W: u32 = 800;
const BAR_X: u32 = 20;
const BAR_W: u32 = 760;

/// One part of a bar: how many, and the legend's words after the number.
#[derive(Debug, Clone)]
pub struct Seg {
    pub n: usize,
    pub words: String,
}

/// One bar: its label and right-hand text, its parts (brightest first) and what they are out of;
/// `rest` names what is left of the total (the empty track) in the legend, or nothing when empty.
#[derive(Debug, Clone)]
pub struct Row {
    pub label: String,
    pub right: String,
    pub segs: Vec<Seg>,
    pub total: usize,
    pub rest: String,
}

pub fn seg(n: usize, words: impl Into<String>) -> Seg {
    Seg {
        n,
        words: words.into(),
    }
}

/// `1234567` as `1,234,567`.
pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `n` of `total` as a percentage with one decimal, rounded half up; `0.0%` of nothing.
pub fn pct(n: usize, total: usize) -> String {
    if total == 0 {
        return "0.0%".into();
    }
    let tenths = (n as u128 * 1000 + total as u128 / 2) / total as u128;
    format!("{}.{}%", tenths / 10, tenths % 10)
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn text(x: u32, y: u32, size: u32, fill: &str, anchor_end: bool, body: &str) -> String {
    format!(
        "  <text x=\"{x}\" y=\"{y}\"{} font-family=\"{FONT}\" font-size=\"{size}\" fill=\"{fill}\">{}</text>\n",
        if anchor_end { " text-anchor=\"end\"" } else { "" },
        esc(body)
    )
}

/// The bar's parts as rectangles inside its rounded track, at whole pixels from the running sum
/// (so the parts meet without gaps and end where their sum ends).
fn bar(out: &mut String, id: usize, y: u32, h: u32, row: &Row) {
    out.push_str(&format!(
        "  <clipPath id=\"c{id}\"><rect x=\"{BAR_X}\" y=\"{y}\" width=\"{BAR_W}\" height=\"{h}\" rx=\"4\"/></clipPath>\n"
    ));
    out.push_str(&format!(
        "  <rect x=\"{BAR_X}\" y=\"{y}\" width=\"{BAR_W}\" height=\"{h}\" rx=\"4\" fill=\"{TRACK}\"/>\n"
    ));
    let mut sum = 0usize;
    let px = |n: usize| -> u32 {
        if row.total == 0 {
            0
        } else {
            ((n.min(row.total) as u128 * BAR_W as u128 + row.total as u128 / 2) / row.total as u128)
                as u32
        }
    };
    for (k, s) in row.segs.iter().enumerate() {
        let (a, b) = (px(sum), px(sum + s.n));
        sum += s.n;
        if b > a {
            out.push_str(&format!(
                "  <rect x=\"{}\" y=\"{y}\" width=\"{}\" height=\"{h}\" fill=\"{}\" clip-path=\"url(#c{id})\"/>\n",
                BAR_X + a,
                b - a,
                SEG[k.min(2)]
            ));
        }
    }
    out.push_str(&format!(
        "  <rect x=\"{BAR_X}\" y=\"{y}\" width=\"{BAR_W}\" height=\"{h}\" rx=\"4\" fill=\"none\" stroke=\"{EDGE}\" stroke-width=\"1\"/>\n"
    ));
}

/// The legend of a bar: a swatch and `n words (p%)` per part that is not empty, then the rest.
fn legend(out: &mut String, y: u32, size: u32, row: &Row) {
    let mut x = BAR_X;
    let mut item = |out: &mut String, n: usize, words: &str, fill: &str| {
        let words = format!("{} {} ({})", thousands(n), words, pct(n, row.total));
        out.push_str(&format!(
            "  <rect x=\"{x}\" y=\"{}\" width=\"10\" height=\"10\" fill=\"{fill}\" stroke=\"{EDGE}\" stroke-width=\"1\"/>\n",
            y - 9
        ));
        out.push_str(&text(x + 16, y, size, TEXT, false, &words));
        // A monospace character is about 0.6 of the font size wide.
        x += 16 + (words.chars().count() as u32 * size * 6).div_ceil(10) + 20;
    };
    for (k, s) in row.segs.iter().enumerate() {
        if s.n > 0 {
            item(out, s.n, &s.words, SEG[k.min(2)]);
        }
    }
    let done: usize = row.segs.iter().map(|s| s.n).sum();
    if !row.rest.is_empty() && row.total > done {
        item(out, row.total - done, &row.rest, TRACK);
    }
}

fn open(h: u32, aria: &str, title: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{W}\" height=\"{h}\" viewBox=\"0 0 {W} {h}\" role=\"img\" aria-label=\"{}\">\n  <title>{}</title>\n  <rect x=\"0\" y=\"0\" width=\"{W}\" height=\"{h}\" rx=\"10\" fill=\"{BG}\"/>\n",
        esc(aria),
        esc(title)
    )
}

/// One bar the size of upstream's: a title and percentage above, the legend below.
pub fn single(title: &str, aria: &str, row: &Row) -> String {
    let mut s = open(96, aria, title);
    s.push_str(&text(BAR_X, 30, 15, TITLE, false, &row.label));
    s.push_str(&text(BAR_X + BAR_W, 30, 15, TITLE, true, &row.right));
    bar(&mut s, 0, 42, 18, row);
    legend(&mut s, 81, 12, row);
    s.push_str("</svg>\n");
    s
}

/// A panel of bars under one heading.
pub fn panel(heading: &str, aria: &str, rows: &[Row]) -> String {
    let h = 46 + 58 * rows.len() as u32 + 4;
    let mut s = open(h, aria, heading);
    s.push_str(&text(BAR_X, 30, 15, TITLE, false, heading));
    for (i, row) in rows.iter().enumerate() {
        let y = 46 + 58 * i as u32;
        s.push_str(&text(BAR_X, y + 12, 13, TEXT, false, &row.label));
        s.push_str(&text(BAR_X + BAR_W, y + 12, 13, TITLE, true, &row.right));
        bar(&mut s, i, y + 20, 12, row);
        legend(&mut s, y + 48, 11, row);
    }
    s.push_str("</svg>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(thousands(69459), "69,459");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000_000), "1,000,000");
        assert_eq!(pct(47733, 69459), "68.7%");
        assert_eq!(pct(16936, 69459), "24.4%");
        assert_eq!(pct(4790, 69459), "6.9%");
        assert_eq!(pct(1, 0), "0.0%");
        assert_eq!(pct(5, 5), "100.0%");
    }

    #[test]
    fn upstreams_bar_has_upstreams_geometry() {
        let row = Row {
            label: "ENGINE PORT".into(),
            right: "100%".into(),
            segs: vec![
                seg(47733, "by hand"),
                seg(16936, "start-up table"),
                seg(4790, "shut-down table"),
            ],
            total: 69459,
            rest: String::new(),
        };
        let s = single("t", "a", &row);
        // Parts at 0..522, 522..708, 708..760 of the 760-pixel track: no gap, no overlap.
        assert!(s.contains("x=\"20\" y=\"42\" width=\"522\""));
        assert!(s.contains("x=\"542\" y=\"42\" width=\"186\""));
        assert!(s.contains("x=\"728\" y=\"42\" width=\"52\""));
        assert!(s.contains("47,733 by hand (68.7%)"));
        assert_eq!(s, single("t", "a", &row), "same bytes");
    }

    #[test]
    fn text_is_escaped() {
        let row = Row {
            label: "lStewieAl's <Tweaks> & co".into(),
            right: String::new(),
            segs: vec![seg(0, "tested")],
            total: 3,
            rest: "not built".into(),
        };
        let s = panel("h", "a", &[row]);
        assert!(s.contains("lStewieAl's &lt;Tweaks&gt; &amp; co"));
        assert!(!s.contains("0 tested"), "an empty part has no legend");
        assert!(s.contains("3 not built (100.0%)"));
    }
}
