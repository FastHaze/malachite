//! Makes the app's icon out of the drawing in `assets/malachite-icon.svg`,
//! so that the drawing is the one place the icon is kept.
//!
//! Two things come of it. One is the picture the window is given for its
//! title bar and the taskbar, which `main.rs` takes in as plain pixels. The
//! other, on Windows, is the icon of the program file itself, the one a
//! folder shows it by: a resource, at the sizes Windows asks for, that is
//! handed to the linker to be put into `malachite.exe`.
//!
//! There is no library here to read drawings or to write icons with. The
//! drawing is made of flat-coloured `<polygon>`s and nothing else, which
//! takes little to read and to fill in; anything else found in it stops the
//! build, rather than leaving the app with an icon that is not the drawing.

use std::path::PathBuf;
use std::{env, fs};

const DRAWING: &str = "assets/malachite-icon.svg";

/// How many pixels across the window's picture is. Whoever shows it scales
/// it to the size they want.
const WINDOW: usize = 256;

/// The sizes of the program file's icon: what Windows shows in lists, on
/// the taskbar and as tiles, at its usual scales.
const SIZES: [usize; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

/// How much of the picture's height the drawing takes: a hair's room is left
/// above and below it.
const FILL: f32 = 0.96;

/// A polygon of the drawing: its corners, its colour and how solid it is.
struct Shape {
    corners: Vec<(f32, f32)>,
    color: [f32; 3],
    opacity: f32,
}

fn main() {
    println!("cargo:rerun-if-changed={DRAWING}");
    println!("cargo:rerun-if-changed=build.rs");
    let text = fs::read_to_string(DRAWING).unwrap_or_else(|e| panic!("the icon's drawing, {DRAWING}, cannot be read: {e}"));
    let (size, shapes) = read(&text);
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo says where built files go"));
    fs::write(out.join("icon.rgba"), picture(&shapes, size, WINDOW)).expect("the window's icon can be written");

    // The linker of the Windows toolchain takes a resource file as it is.
    let target = |name: &str| env::var(format!("CARGO_CFG_TARGET_{name}")).unwrap_or_default();
    if target("OS") == "windows" && target("ENV") == "msvc" {
        let resource = out.join("icon.res");
        fs::write(&resource, resources(&shapes, size)).expect("the icon resource can be written");
        println!("cargo:rustc-link-arg-bin=malachite={}", resource.display());
    }
}

/// What a tag says for `name`, if it says anything.
fn said<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let from = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    Some(&tag[from..from + tag[from..].find('"')?])
}

/// The size of the drawing and its polygons, in the order they are drawn.
fn read(text: &str) -> ((f32, f32), Vec<Shape>) {
    let numbers = |list: &str| -> Vec<f32> {
        let parts = list.split(|c: char| c.is_whitespace() || c == ',').filter(|part| !part.is_empty());
        parts.map(|part| part.parse().unwrap_or_else(|_| panic!("{DRAWING}: `{part}` is not a number"))).collect()
    };
    let mut size = None;
    let mut shapes = Vec::new();
    for tag in text.split('<').skip(1) {
        let name = tag.split(|c: char| c.is_whitespace() || c == '>' || c == '/').next().unwrap_or_default();
        match name {
            "svg" => {
                let view = numbers(said(tag, "viewBox").unwrap_or_else(|| panic!("{DRAWING} has no viewBox to say how large it is")));
                assert!(view.len() == 4 && view[2] > 0.0 && view[3] > 0.0, "{DRAWING}: the viewBox is four numbers");
                assert!(view[0] == 0.0 && view[1] == 0.0, "{DRAWING}: the viewBox is expected to start at 0 0");
                size = Some((view[2], view[3]));
            }
            "polygon" => {
                let corners = numbers(said(tag, "points").unwrap_or_else(|| panic!("{DRAWING}: a polygon without points")));
                assert!(corners.len() >= 6 && corners.len() % 2 == 0, "{DRAWING}: a polygon's points come in pairs, three or more");
                let fill = said(tag, "fill").unwrap_or("#000000");
                let hex = fill.strip_prefix('#').filter(|hex| hex.len() == 6 && hex.is_ascii()).unwrap_or_else(|| panic!("{DRAWING}: the fill `{fill}` is not #RRGGBB"));
                let channel = |at: usize| f32::from(u8::from_str_radix(&hex[at..at + 2], 16).unwrap_or_else(|_| panic!("{DRAWING}: the fill `{fill}` is not #RRGGBB"))) / 255.0;
                let opacity = said(tag, "opacity").map_or(1.0, |opacity| numbers(opacity)[0]);
                shapes.push(Shape { corners: corners.chunks(2).map(|pair| (pair[0], pair[1])).collect(), color: [channel(0), channel(2), channel(4)], opacity });
            }
            // What says something about the drawing without drawing anything.
            "" | "metadata" | "title" | "desc" | "c2pa:manifest" => {}
            _ if name.starts_with('!') || name.starts_with('?') => {}
            other => panic!("{DRAWING} has a <{other}> in it. The icon is made from <polygon>s only (see build.rs): draw it with those, or teach build.rs the rest."),
        }
    }
    assert!(!shapes.is_empty(), "{DRAWING} draws nothing");
    (size.unwrap_or_else(|| panic!("{DRAWING} is not an <svg>")), shapes)
}

/// Whether a point is inside a polygon: a line from it to the right crosses
/// the polygon's edge an odd number of times.
fn inside(corners: &[(f32, f32)], (x, y): (f32, f32)) -> bool {
    let mut inside = false;
    let mut before = corners[corners.len() - 1];
    for &corner in corners {
        let ((x0, y0), (x1, y1)) = (before, corner);
        if (y0 > y) != (y1 > y) && x < x0 + (y - y0) / (y1 - y0) * (x1 - x0) {
            inside = !inside;
        }
        before = corner;
    }
    inside
}

/// The drawing as a square picture `side` pixels across, in the middle of
/// it: red, green, blue and opacity for each pixel, row after row from the
/// top. Each pixel is looked at in a grid of places, for smooth edges.
fn picture(shapes: &[Shape], (wide, tall): (f32, f32), side: usize) -> Vec<u8> {
    let scale = side as f32 * FILL / wide.max(tall);
    let (left, top) = ((side as f32 - wide * scale) / 2.0, (side as f32 - tall * scale) / 2.0);
    // Small pictures are few pixels, each of which matters more.
    let grid = if side >= 128 { 4 } else { 8 };
    let mut pixels = Vec::with_capacity(side * side * 4);
    for y in 0..side {
        for x in 0..side {
            // Colour weighted by opacity, and opacity, summed over the places.
            let mut sum = [0.0f32; 4];
            for place in 0..grid * grid {
                let (across, down) = ((place % grid) as f32 + 0.5, (place / grid) as f32 + 0.5);
                let point = ((x as f32 + across / grid as f32 - left) / scale, (y as f32 + down / grid as f32 - top) / scale);
                let mut seen = [0.0f32; 4];
                for shape in shapes.iter().filter(|shape| inside(&shape.corners, point)) {
                    for channel in 0..3 {
                        seen[channel] = shape.color[channel] * shape.opacity + seen[channel] * (1.0 - shape.opacity);
                    }
                    seen[3] = shape.opacity + seen[3] * (1.0 - shape.opacity);
                }
                for channel in 0..4 {
                    sum[channel] += seen[channel];
                }
            }
            let opacity = sum[3] / (grid * grid) as f32;
            for channel in 0..3 {
                let color = if sum[3] > 0.0 { sum[channel] / sum[3] } else { 0.0 };
                pixels.push((color * 255.0).round().clamp(0.0, 255.0) as u8);
            }
            pixels.push((opacity * 255.0).round().clamp(0.0, 255.0) as u8);
        }
    }
    pixels
}

/// One picture of an icon as Windows keeps it: a bitmap, bottom row first,
/// in blue, green, red and opacity, followed by a mask of the pixels that
/// are not there at all.
fn bitmap(pixels: &[u8], side: usize) -> Vec<u8> {
    let mask_row = side.div_ceil(32) * 4;
    let mut out = Vec::with_capacity(40 + side * side * 4 + side * mask_row);
    out.extend(40u32.to_le_bytes());
    out.extend((side as i32).to_le_bytes());
    // Twice the height: the picture, and its mask.
    out.extend((2 * side as i32).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(((side * side * 4 + side * mask_row) as u32).to_le_bytes());
    out.extend([0u8; 16]);
    for y in (0..side).rev() {
        for pixel in pixels[y * side * 4..(y + 1) * side * 4].chunks(4) {
            out.extend([pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    for y in (0..side).rev() {
        let mut row = vec![0u8; mask_row];
        for x in (0..side).filter(|x| pixels[(y * side + x) * 4 + 3] == 0) {
            row[x / 8] |= 0x80 >> (x % 8);
        }
        out.extend(row);
    }
    out
}

/// One entry of a resource file: its kind and number, then what it holds.
fn entry(file: &mut Vec<u8>, kind: u16, number: u16, flags: u16, data: &[u8]) {
    file.extend((data.len() as u32).to_le_bytes());
    // The size of this heading: the two sizes, kind and number, and the rest.
    file.extend(32u32.to_le_bytes());
    for id in [kind, number] {
        // 0xFFFF says a number follows where a name could be.
        file.extend(0xFFFFu16.to_le_bytes());
        file.extend(id.to_le_bytes());
    }
    file.extend(0u32.to_le_bytes());
    file.extend(flags.to_le_bytes());
    // No language in particular, version or characteristics.
    file.extend([0u8; 10]);
    file.extend(data);
    while file.len() % 4 != 0 {
        file.push(0);
    }
}

/// The program file's icon as a compiled resource file: what a resource
/// compiler would make of an `.ico` with these pictures in it. Each picture
/// is an icon resource, and a group resource lists them for Windows to pick
/// from; the group is number 1, the first, which is the one a file is shown by.
fn resources(shapes: &[Shape], size: (f32, f32)) -> Vec<u8> {
    const ICON: u16 = 3;
    const GROUP: u16 = 14;
    let mut file = Vec::new();
    // Every resource file opens with an empty entry.
    entry(&mut file, 0, 0, 0, &[]);
    let mut group = Vec::new();
    group.extend(0u16.to_le_bytes());
    group.extend(1u16.to_le_bytes());
    group.extend((SIZES.len() as u16).to_le_bytes());
    for (index, side) in SIZES.into_iter().enumerate() {
        let number = index as u16 + 1;
        let image = bitmap(&picture(shapes, size, side), side);
        // A size of 256 is written as 0: a byte holds no more.
        group.extend([side as u8, side as u8, 0, 0]);
        group.extend(1u16.to_le_bytes());
        group.extend(32u16.to_le_bytes());
        group.extend((image.len() as u32).to_le_bytes());
        group.extend(number.to_le_bytes());
        entry(&mut file, ICON, number, 0x1010, &image);
    }
    entry(&mut file, GROUP, 1, 0x1030, &group);
    file
}
