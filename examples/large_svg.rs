//! Writes `test-svgs/large-map.svg`, a made-up map of 6 MB or more, for checking
//! that big files open without freezing the window and still move smoothly.
//! The file is generated instead of committed so the repo stays small.
//!
//! Run with `cargo run --release --example large_svg`.

use std::fmt::Write as _;
use std::path::Path;

const WIDTH: f32 = 4000.0;
const HEIGHT: f32 = 3000.0;
/// Land is cut into a grid of regions, each an irregular polygon.
const COLUMNS: usize = 130;
const ROWS: usize = 100;

/// A tiny xorshift generator, so every run makes the same file without a crate.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.next()
    }
}

fn main() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut svg = String::with_capacity(8 << 20);
    let cell_w = WIDTH / COLUMNS as f32;
    let cell_h = HEIGHT / ROWS as f32;

    let _ = writeln!(svg, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}">"#);
    svg.push_str(r##"<defs>
<linearGradient id="sea" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#a8d4f0"/><stop offset="1" stop-color="#5d9fd0"/></linearGradient>
<radialGradient id="hill"><stop offset="0" stop-color="#8a6d3b" stop-opacity="0.5"/><stop offset="1" stop-color="#8a6d3b" stop-opacity="0"/></radialGradient>
</defs>
<rect width="100%" height="100%" fill="url(#sea)"/>
"##);

    // Grid corners, jittered so the regions look like borders, not squares.
    let corner = |rng: &mut Rng, column: usize, row: usize| {
        let x = column as f32 * cell_w;
        let y = row as f32 * cell_h;
        let edge = column == 0 || row == 0 || column == COLUMNS || row == ROWS;
        if edge { (x, y) } else { (x + rng.range(-0.3, 0.3) * cell_w, y + rng.range(-0.3, 0.3) * cell_h) }
    };
    let corners: Vec<Vec<(f32, f32)>> = (0..=ROWS).map(|row| (0..=COLUMNS).map(|column| corner(&mut rng, column, row)).collect()).collect();

    // Regions: each edge between two corners gets a few wiggles.
    svg.push_str(r##"<g stroke="#5a5a5a" stroke-width="0.6" stroke-linejoin="round">
"##);
    for row in 0..ROWS {
        for column in 0..COLUMNS {
            let ring = [corners[row][column], corners[row][column + 1], corners[row + 1][column + 1], corners[row + 1][column]];
            let hue = 70.0 + rng.range(0.0, 60.0);
            let light = 55.0 + rng.range(0.0, 25.0);
            let _ = write!(svg, r#"<path fill="hsl({hue:.0},45%,{light:.0}%)" d="M{:.1},{:.1}"#, ring[0].0, ring[0].1);
            for side in 0..4 {
                let (a, b) = (ring[side], ring[(side + 1) % 4]);
                for step in 1..6 {
                    let t = step as f32 / 6.0;
                    let x = a.0 + (b.0 - a.0) * t + rng.range(-1.5, 1.5);
                    let y = a.1 + (b.1 - a.1) * t + rng.range(-1.5, 1.5);
                    let _ = write!(svg, " L{x:.1},{y:.1}");
                }
                let _ = write!(svg, " L{:.1},{:.1}", b.0, b.1);
            }
            svg.push_str(r#" Z"/>
"#);
        }
    }
    svg.push_str("</g>\n");

    // Hills: soft gradient spots.
    for _ in 0..600 {
        let _ = writeln!(svg, r#"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="url(#hill)"/>"#, rng.range(0.0, WIDTH), rng.range(0.0, HEIGHT), rng.range(10.0, 60.0));
    }

    // Rivers and roads: long wandering lines.
    for (count, style) in [(80, r##"stroke="#3b7dc4" stroke-width="2.5""##), (300, r##"stroke="#c0392b" stroke-width="1.2" stroke-dasharray="6 3""##)] {
        for _ in 0..count {
            let (mut x, mut y) = (rng.range(0.0, WIDTH), rng.range(0.0, HEIGHT));
            let mut angle = rng.range(0.0, std::f32::consts::TAU);
            let _ = write!(svg, r#"<path fill="none" {style} d="M{x:.1},{y:.1}"#);
            for _ in 0..120 {
                angle += rng.range(-0.4, 0.4);
                x += angle.cos() * 12.0;
                y += angle.sin() * 12.0;
                let _ = write!(svg, " L{x:.1},{y:.1}");
            }
            svg.push_str("\"/>\n");
        }
    }

    // Place names, so the file also exercises text.
    svg.push_str(r##"<g font-family="sans-serif" fill="#222" text-anchor="middle">
"##);
    let syllables = ["ka", "lo", "mir", "ven", "dra", "su", "tel", "ash", "or", "bri"];
    for _ in 0..1500 {
        let mut name = String::new();
        for i in 0..2 + (rng.next() * 2.0) as usize {
            let syllable = syllables[(rng.next() * syllables.len() as f32) as usize % syllables.len()];
            if i == 0 {
                let mut letters = syllable.chars();
                name.extend(letters.next().map(|c| c.to_ascii_uppercase()));
                name.extend(letters);
            } else {
                name.push_str(syllable);
            }
        }
        let _ = writeln!(svg, r#"<text x="{:.1}" y="{:.1}" font-size="{:.0}">{name}</text>"#, rng.range(0.0, WIDTH), rng.range(0.0, HEIGHT), rng.range(6.0, 16.0));
    }
    svg.push_str("</g>\n</svg>\n");

    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-svgs").join("large-map.svg");
    if let Err(e) = std::fs::write(&path, &svg) {
        eprintln!("could not write {}: {e}", path.display());
        std::process::exit(1);
    }
    println!("wrote {} ({:.1} MB)", path.display(), svg.len() as f64 / 1e6);
}
