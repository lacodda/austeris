//! Guards the mark the product is recognised by.
//!
//! The rasters in `assets/` are built by `docs/export-assets.mjs` from the
//! three brand SVGs, and nothing downstream of that script would notice if it
//! built them wrong: an `.ico` with the correct sizes is a valid `.ico` whatever
//! is drawn inside it, the docs site renders whatever PNG it is handed, and
//! GitHub shows whatever the README points at. The failure is visual, and a
//! visual failure in a file nobody opens is a failure nobody finds - this one
//! shipped for two weeks before the owner noticed it in a browser tab.
//!
//! So the pixels are read here. The `.ico` is a container of PNGs, and these
//! are 8-bit RGBA with no interlacing and no palette, which is the one PNG
//! shape simple enough to unpack in a page of code: inflate the image data and
//! undo the per-row filter. A decoder crate would be a dependency taken on for
//! a single sample of a single pixel (`flate2` is already in the tree).

use std::path::{Path, PathBuf};

/// Where the rasters live.
fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// One image inside an `.ico`, as the directory describes it.
#[derive(Debug)]
struct Entry {
    size: u32,
    offset: usize,
    length: usize,
}

/// Reads the directory of an `.ico`.
///
/// The container is a header, a table of these, and the payloads. A malformed
/// one gives an empty list rather than an error: the tests below then fail on
/// "no images", which says more than a panic inside the parser would.
fn entries(ico: &[u8]) -> Vec<Entry> {
    // Header: reserved (2), type (2), count (2). Then 16 bytes per entry.
    let Some(count) = ico.get(4..6).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize) else {
        return Vec::new();
    };

    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let at = 6 + index * 16;
        let Some(entry) = ico.get(at..at + 16) else { break };
        // A zero width means 256: the field is one byte and 256 does not fit.
        let size = if entry[0] == 0 { 256 } else { u32::from(entry[0]) };
        let length = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
        let offset = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;
        if ico.len() < offset + length {
            continue;
        }
        out.push(Entry { size, offset, length });
    }
    out
}

/// A decoded image: RGBA rows, eight bits a channel.
struct Image {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Image {
    /// The RGBA at a position.
    fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = (y as usize * self.width as usize + x as usize) * 4;
        self.pixels[at..at + 4].try_into().expect("four channels")
    }
}

/// Unpacks an 8-bit RGBA, non-interlaced PNG.
///
/// Only that shape, deliberately: it is what the exporter writes, and a parser
/// covering the rest of the format would be a decoder crate written badly. A
/// PNG of any other shape fails the assertion rather than being guessed at.
fn decode_png(png: &[u8]) -> Image {
    use std::io::Read;

    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");

    // IHDR is always the first chunk: length (4), type (4), then its fields.
    let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
    let (depth, colour, interlace) = (png[24], png[25], png[28]);
    assert_eq!(depth, 8, "the exporter wrote a {depth}-bit PNG; this reader only unpacks 8");
    assert_eq!(colour, 6, "the exporter wrote colour type {colour}; this reader only unpacks RGBA");
    assert_eq!(interlace, 0, "the exporter wrote an interlaced PNG; this reader only unpacks flat ones");

    // Concatenate every IDAT: a large image is split across several, and
    // inflating only the first gives a short, silently truncated picture.
    let mut deflated = Vec::new();
    let mut at = 8;
    while at + 8 <= png.len() {
        let length = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        let kind = &png[at + 4..at + 8];
        if kind == b"IDAT" {
            deflated.extend_from_slice(&png[at + 8..at + 8 + length]);
        }
        // length + the length field, the type, and the CRC.
        at += length + 12;
        if kind == b"IEND" {
            break;
        }
    }
    assert!(!deflated.is_empty(), "the PNG carries no image data");

    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(deflated.as_slice())
        .read_to_end(&mut raw)
        .expect("inflating the image data");

    // Each row is a filter byte followed by the pixels, and every filter is
    // defined against the row above and the pixel to the left (RFC 2083 §6).
    let stride = width as usize * 4;
    assert_eq!(
        raw.len(),
        (stride + 1) * height as usize,
        "the inflated data is not {height} rows of {stride} bytes"
    );

    let mut pixels = vec![0u8; stride * height as usize];
    for y in 0..height as usize {
        let filter = raw[y * (stride + 1)];
        let row = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        for x in 0..stride {
            let left = if x >= 4 { pixels[y * stride + x - 4] } else { 0 };
            let up = if y > 0 { pixels[(y - 1) * stride + x] } else { 0 };
            let up_left = if y > 0 && x >= 4 { pixels[(y - 1) * stride + x - 4] } else { 0 };
            let value = match filter {
                0 => row[x],
                1 => row[x].wrapping_add(left),
                2 => row[x].wrapping_add(up),
                // The Average filter is the floor of the mean, which is what
                // `midpoint` computes without the sum overflowing on the way.
                3 => row[x].wrapping_add(u8::midpoint(left, up)),
                4 => row[x].wrapping_add(paeth(left, up, up_left)),
                other => panic!("row {y} uses filter {other}, which is not a PNG filter"),
            };
            pixels[y * stride + x] = value;
        }
    }

    Image { width, height, pixels }
}

/// The Paeth predictor: whichever neighbour the gradient points at.
fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let p = i16::from(left) + i16::from(up) - i16::from(up_left);
    let (dl, du, dul) = ((p - i16::from(left)).abs(), (p - i16::from(up)).abs(), (p - i16::from(up_left)).abs());
    if dl <= du && dl <= dul {
        left
    } else if du <= dul {
        up
    } else {
        up_left
    }
}

/// Whether an image is the filled tile rather than the plated mark.
///
/// One sample, a quarter of the way across and vertically centred: inside the
/// hexagon, clear of the code and of the bars beneath it. The S tile fills that
/// hexagon with the brand orchid (`#C25BD9`); M and L fill it with the near-black
/// plate (`#1B2126`) and draw the mark on top. Brightness tells them apart,
/// which is exactly what the eye does at a glance.
fn is_filled_tile(image: &Image) -> bool {
    let sample = image.pixel(image.width / 4, image.height / 2);
    assert!(sample[3] > 40, "the sample point is transparent; the tile is not where this expects it");
    let brightness = u32::from(sample[0]) + u32::from(sample[1]) + u32::from(sample[2]);
    brightness > 180
}

/// Reads an image out of the `.ico`.
fn image_in(ico: &[u8], entry: &Entry) -> Image {
    decode_png(&ico[entry.offset..entry.offset + entry.length])
}

fn read(path: impl AsRef<Path>) -> Vec<u8> {
    let path = assets().join(path);
    std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The level rule of the line, held against the actual pixels.
///
/// This is the test the fix exists for. The exporter took the S tile - a hexagon
/// filled with the brand colour - for *every* size, so a 256px icon was a flat
/// orchid blob with `au` on it instead of the mark, and `apple-touch-icon.png`
/// was the same at 180px. Nothing held it.
///
/// The rule is: S at 27px and below, where the outline and the tonal steps
/// collapse into noise and the filled tile is all that survives; the plated mark
/// above it.
#[test]
fn every_size_carries_the_level_that_reads_at_it() {
    let ico = read("icon.ico");
    let entries = entries(&ico);
    assert!(!entries.is_empty(), "assets/icon.ico gave no images");

    for entry in &entries {
        let image = image_in(&ico, entry);
        assert_eq!(image.width, entry.size, "the {}px entry holds a {}px image", entry.size, image.width);
        assert_eq!(image.height, entry.size, "the {}px entry is not square", entry.size);

        if entry.size <= 27 {
            assert!(
                is_filled_tile(&image),
                "the {}px image is not the filled tile; below 28px the outline collapses into noise",
                entry.size
            );
        } else {
            assert!(
                !is_filled_tile(&image),
                "the {}px image is the filled S tile, not the plated mark - the level rule of the line puts S at 27px and below",
                entry.size
            );
        }
    }
}

/// Largest first.
///
/// Windows picks by closest size and ignores order, but readers that take the
/// first entry verbatim exist - kilna's titlebar was stretched from a 16px entry
/// for exactly this reason. Cheap to hold, expensive to notice.
#[test]
fn the_largest_image_comes_first() {
    let ico = read("icon.ico");
    let sizes: Vec<u32> = entries(&ico).iter().map(|entry| entry.size).collect();
    assert!(!sizes.is_empty(), "assets/icon.ico gave no images");

    let mut sorted = sizes.clone();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(sizes, sorted, "the images are not ordered largest first");
}

/// Both ends of the range are actually in the container.
///
/// A `levelFor` that is right about sizes nothing asks for proves nothing: the
/// rule only means something if the file holds a size on each side of the
/// boundary, so this pins that it does.
#[test]
fn the_container_spans_the_level_boundary() {
    let ico = read("icon.ico");
    let sizes: Vec<u32> = entries(&ico).iter().map(|entry| entry.size).collect();

    assert!(sizes.iter().any(|&size| size <= 27), "no image small enough to need the S tile: {sizes:?}");
    assert!(sizes.iter().any(|&size| size >= 64), "no image large enough to carry the full mark: {sizes:?}");
}

/// The PNGs beside the `.ico` follow the same rule.
///
/// `apple-touch-icon.png` is 180px - a home-screen tile, the largest place the
/// mark is shown outside the site itself - and it was the filled tile too. The
/// favicon is the one documented exception: a browser draws it into 16px of tab
/// whatever the file's size, so it takes S at 32px on purpose.
#[test]
fn the_standalone_rasters_follow_the_rule_and_its_one_exception() {
    let apple = decode_png(&read("apple-touch-icon.png"));
    assert_eq!(apple.width, 180, "apple-touch-icon.png is {}px", apple.width);
    assert!(
        !is_filled_tile(&apple),
        "apple-touch-icon.png is the filled S tile; at 180px it should carry the full mark"
    );

    let logo = decode_png(&read("logo-512.png"));
    assert_eq!(logo.width, 512, "logo-512.png is {}px", logo.width);
    assert!(!is_filled_tile(&logo), "logo-512.png is the filled S tile rather than the mark");

    // The exception, asserted rather than assumed: if someone "fixes" the
    // favicon to follow `levelFor`, this says why it does not.
    let favicon = decode_png(&read("favicon-32.png"));
    assert_eq!(favicon.width, 32, "favicon-32.png is {}px", favicon.width);
    assert!(
        is_filled_tile(&favicon),
        "favicon-32.png is not the filled tile; a browser draws it into 16px of tab whatever its size"
    );
}
