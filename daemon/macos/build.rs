use std::error::Error;
use std::fs;
use std::path::PathBuf;

const TRAY_ICON_SIZE: u32 = 64;
const TRAY_ICON_DRAW_SIZE: f32 = 58.0;

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let svg_path = manifest_dir.join("../../wgo.svg");
    let out_path = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("wgo-tray.rgba");

    println!("cargo:rerun-if-changed={}", svg_path.display());

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return Ok(());
    }

    let svg = fs::read(&svg_path)?;
    let rgba = render_tray_icon(&svg)?;
    fs::write(out_path, rgba)?;
    Ok(())
}

fn render_tray_icon(svg: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default())?;
    let mut pixmap = tiny_skia::Pixmap::new(TRAY_ICON_SIZE, TRAY_ICON_SIZE)
        .ok_or("failed to allocate macOS tray icon pixmap")?;

    let svg_size = tree.size();
    let scale = TRAY_ICON_DRAW_SIZE / svg_size.width().max(svg_size.height());
    let x = (TRAY_ICON_SIZE as f32 - svg_size.width() * scale) / 2.0;
    let y = (TRAY_ICON_SIZE as f32 - svg_size.height() * scale) / 2.0;
    let transform = tiny_skia::Transform::from_translate(x, y).pre_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let mut rgba = pixmap.data().to_vec();
    let black_mask = dilate_mask(
        &black_pixel_mask(&rgba, TRAY_ICON_SIZE, TRAY_ICON_SIZE),
        TRAY_ICON_SIZE,
        TRAY_ICON_SIZE,
        1,
    );
    for (index, pixel) in rgba.chunks_exact_mut(4).enumerate() {
        if black_mask[index] {
            pixel[0] = 0;
            pixel[1] = 0;
            pixel[2] = 0;
            pixel[3] = 255;
        } else {
            pixel[0] = 0;
            pixel[1] = 0;
            pixel[2] = 0;
            pixel[3] = 0;
        }
    }
    Ok(rgba)
}

fn black_pixel_mask(bgra: &[u8], width: u32, height: u32) -> Vec<bool> {
    let mut mask = vec![false; (width * height) as usize];
    for (index, pixel) in bgra.chunks_exact(4).enumerate() {
        let [blue, green, red, alpha] = [pixel[0], pixel[1], pixel[2], pixel[3]];
        mask[index] = alpha != 0 && red <= 32 && green <= 32 && blue <= 32;
    }
    mask
}

fn dilate_mask(mask: &[bool], width: u32, height: u32, radius: i32) -> Vec<bool> {
    let mut dilated = vec![false; mask.len()];
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let index = (y as u32 * width + x as u32) as usize;
            if !mask[index] {
                continue;
            }
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    if dx * dx + dy * dy > radius * radius {
                        continue;
                    }
                    let nx = x + dx;
                    let ny = y + dy;
                    if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                        continue;
                    }
                    let next_index = (ny as u32 * width + nx as u32) as usize;
                    dilated[next_index] = true;
                }
            }
        }
    }
    dilated
}
