//! Seeded synthetic transport. These distortions do not replace real video codec benchmarks.
use vft_fec::splitmix64;
use vft_visual::{Error, LumaImage, VisualFrame};

#[derive(Clone, Copy, Debug)]
pub struct Distortion {
    pub scale_percent: u32,
    pub blur_radius: usize,
    pub brightness: i16,
    pub contrast_percent: u32,
    pub noise: u8,
    pub corrupt_cells_percent: u32,
}
impl Default for Distortion {
    fn default() -> Self {
        Self {
            scale_percent: 100,
            blur_radius: 0,
            brightness: 0,
            contrast_percent: 100,
            noise: 0,
            corrupt_cells_percent: 0,
        }
    }
}

pub struct Transport {
    pub seed: u64,
    pub drop_percent: u32,
    pub duplicate_percent: u32,
    pub cell_size: usize,
    pub distortion: Distortion,
}
impl Default for Transport {
    fn default() -> Self {
        Self {
            seed: 0,
            drop_percent: 0,
            duplicate_percent: 0,
            cell_size: 4,
            distortion: Distortion::default(),
        }
    }
}
impl Transport {
    pub fn transmit(&self, frame: &VisualFrame, sequence: u64) -> Result<Vec<LumaImage>, Error> {
        if self.drop_percent > 100 || self.duplicate_percent > 100 {
            return Err(Error::Dimensions);
        }
        let random = splitmix64(self.seed ^ sequence);
        if random % 100 < u64::from(self.drop_percent) {
            return Ok(Vec::new());
        }
        let mut corrupted = frame.clone();
        if self.distortion.corrupt_cells_percent > 100 {
            return Err(Error::Dimensions);
        }
        for (i, cell) in corrupted.cells.iter_mut().enumerate() {
            if splitmix64(random ^ i as u64) % 100
                < u64::from(self.distortion.corrupt_cells_percent)
            {
                *cell = 255 - *cell;
            }
        }
        let image = distort(&corrupted.render(self.cell_size)?, self.distortion, random)?;
        if splitmix64(random) % 100 < u64::from(self.duplicate_percent) {
            Ok(vec![image.clone(), image])
        } else {
            Ok(vec![image])
        }
    }
    pub fn reorder<T>(&self, batch: &mut [T]) {
        for i in (1..batch.len()).rev() {
            batch.swap(
                i,
                (splitmix64(self.seed ^ i as u64) % (i as u64 + 1)) as usize,
            );
        }
    }
}

pub fn distort(image: &LumaImage, config: Distortion, seed: u64) -> Result<LumaImage, Error> {
    image.validate()?;
    if !(25..=200).contains(&config.scale_percent)
        || config.blur_radius > 3
        || config.contrast_percent > 200
    {
        return Err(Error::Dimensions);
    }
    let mut result = image.clone();
    if config.blur_radius > 0 {
        result = blur(&result, config.blur_radius);
    }
    if config.scale_percent != 100 {
        let width = image.width * config.scale_percent as usize / 100;
        let height = image.height * config.scale_percent as usize / 100;
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err(Error::Dimensions);
        }
        result = resize(&result, width, height);
    }
    for (i, pixel) in result.pixels.iter_mut().enumerate() {
        let range = u64::from(config.noise) * 2 + 1;
        let noise = (splitmix64(seed ^ i as u64) % range) as i32 - i32::from(config.noise);
        let value = i32::from(*pixel) * config.contrast_percent as i32 / 100
            + i32::from(config.brightness)
            + noise;
        *pixel = value.clamp(0, 255) as u8;
    }
    Ok(result)
}

/// Bilinear resizing at pixel centers, with clamped edge coordinates.
pub fn resize(image: &LumaImage, width: usize, height: usize) -> LumaImage {
    let mut output = LumaImage {
        width,
        height,
        pixels: vec![0; width * height],
    };
    for y in 0..height {
        let sy = ((y as f64 + 0.5) * image.height as f64 / height as f64 - 0.5).max(0.0);
        let y0 = (sy as usize).min(image.height - 1);
        let y1 = (y0 + 1).min(image.height - 1);
        let fy = sy - y0 as f64;
        for x in 0..width {
            let sx = ((x as f64 + 0.5) * image.width as f64 / width as f64 - 0.5).max(0.0);
            let x0 = (sx as usize).min(image.width - 1);
            let x1 = (x0 + 1).min(image.width - 1);
            let fx = sx - x0 as f64;
            let top = f64::from(image.pixels[y0 * image.width + x0]) * (1.0 - fx)
                + f64::from(image.pixels[y0 * image.width + x1]) * fx;
            let bottom = f64::from(image.pixels[y1 * image.width + x0]) * (1.0 - fx)
                + f64::from(image.pixels[y1 * image.width + x1]) * fx;
            output.pixels[y * width + x] = (top * (1.0 - fy) + bottom * fy).round() as u8;
        }
    }
    output
}
fn blur(image: &LumaImage, radius: usize) -> LumaImage {
    let mut horizontal = image.clone();
    for y in 0..image.height {
        for x in 0..image.width {
            let start = x.saturating_sub(radius);
            let end = (x + radius).min(image.width - 1);
            let sum: u32 = image.pixels[y * image.width + start..=y * image.width + end]
                .iter()
                .map(|&v| u32::from(v))
                .sum();
            horizontal.pixels[y * image.width + x] = (sum / (end - start + 1) as u32) as u8;
        }
    }
    let mut output = image.clone();
    for y in 0..image.height {
        for x in 0..image.width {
            let start = y.saturating_sub(radius);
            let end = (y + radius).min(image.height - 1);
            let sum: u32 = (start..=end)
                .map(|yy| u32::from(horizontal.pixels[yy * image.width + x]))
                .sum();
            output.pixels[y * image.width + x] = (sum / (end - start + 1) as u32) as u8;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seed_reproducibility_and_reorder() {
        let image = LumaImage {
            width: 2,
            height: 2,
            pixels: vec![0, 80, 170, 255],
        };
        let cfg = Distortion {
            noise: 8,
            ..Default::default()
        };
        assert_eq!(
            distort(&image, cfg, 99).unwrap().pixels,
            distort(&image, cfg, 99).unwrap().pixels
        );
        let mut items = (0..32).collect::<Vec<_>>();
        Transport::default().reorder(&mut items);
        assert_ne!(items, (0..32).collect::<Vec<_>>());
        items.sort();
        assert_eq!(items, (0..32).collect::<Vec<_>>());
    }
}
