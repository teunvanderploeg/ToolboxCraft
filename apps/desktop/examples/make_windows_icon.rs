//! Convert the existing artwork to a multi-size Windows ICO, including legacy DIB images for NSIS.
use std::{fs, io::Write, path::Path};
fn main() -> anyhow::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = image::open(root.join("assets/icons/toolboxcraft.png"))?;
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let mut images = Vec::new();
    for size in sizes {
        let pixels = source
            .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
            .into_rgba8();
        let mask_stride = size.div_ceil(32) * 4;
        let mut data = Vec::new();
        for value in [40u32, size, size * 2] {
            data.extend(value.to_le_bytes());
        }
        data.extend(1u16.to_le_bytes());
        data.extend(32u16.to_le_bytes());
        for value in [0u32, size * size * 4, 0, 0, 0, 0] {
            data.extend(value.to_le_bytes());
        }
        for y in (0..size).rev() {
            for x in 0..size {
                let p = pixels.get_pixel(x, y).0;
                data.extend([p[2], p[1], p[0], p[3]]);
            }
        }
        for y in (0..size).rev() {
            let mut mask = vec![0u8; mask_stride as usize];
            for x in 0..size {
                if pixels.get_pixel(x, y).0[3] == 0 {
                    mask[(x / 8) as usize] |= 0x80 >> (x % 8);
                }
            }
            data.extend(mask);
        }
        images.push(data);
    }
    let mut file = fs::File::create(root.join("assets/icons/toolboxcraft.ico"))?;
    file.write_all(&[0, 0, 1, 0])?;
    file.write_all(&(sizes.len() as u16).to_le_bytes())?;
    let mut offset = 6 + sizes.len() as u32 * 16;
    for (size, data) in sizes.iter().zip(&images) {
        file.write_all(&[*size as u8, *size as u8, 0, 0])?;
        file.write_all(&1u16.to_le_bytes())?;
        file.write_all(&32u16.to_le_bytes())?;
        file.write_all(&(data.len() as u32).to_le_bytes())?;
        file.write_all(&offset.to_le_bytes())?;
        offset += data.len() as u32;
    }
    for data in images {
        file.write_all(&data)?;
    }
    Ok(())
}
