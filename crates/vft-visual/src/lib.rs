//! Independent, calibrated luminance frames. Rendering and sampling are platform-neutral.
#![no_std]
extern crate alloc;
use alloc::{vec, vec::Vec};
use core::fmt;
use vft_protocol::{Packet, ProfileId, CRC_LEN, HEADER_LEN};

const BOOTSTRAP_CELLS: usize = 80;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridConfig {
    pub columns: usize,
    pub rows: usize,
}
impl GridConfig {
    pub fn for_profile(profile: ProfileId) -> Self {
        match profile {
            ProfileId::Robust => Self {
                columns: 96,
                rows: 54,
            },
            ProfileId::Balanced => Self {
                columns: 192,
                rows: 108,
            },
        }
    }
    pub fn validate(self) -> Result<(), Error> {
        if !(96..=512).contains(&self.columns) || !(8..=512).contains(&self.rows) {
            return Err(Error::Dimensions);
        }
        Ok(())
    }
    pub fn packet_capacity(self, profile: ProfileId) -> Result<usize, Error> {
        self.validate()?;
        Ok((self.columns - 2) * (self.rows - 4) * bits(profile) / 8)
    }
    pub fn payload_capacity(self, profile: ProfileId) -> Result<usize, Error> {
        self.packet_capacity(profile)?
            .checked_sub(HEADER_LEN + CRC_LEN)
            .ok_or(Error::Capacity)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualFrame {
    pub grid: GridConfig,
    pub cells: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct LumaImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}
impl LumaImage {
    pub fn validate(&self) -> Result<(), Error> {
        if self.width == 0
            || self.height == 0
            || self.width > 8192
            || self.height > 8192
            || self.width.checked_mul(self.height) != Some(self.pixels.len())
        {
            return Err(Error::Dimensions);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Dimensions,
    Capacity,
    Sync,
    Calibration,
    Metadata,
    Protocol(vft_protocol::Error),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VFT visual: {self:?}")
    }
}
impl core::error::Error for Error {}
impl From<vft_protocol::Error> for Error {
    fn from(e: vft_protocol::Error) -> Self {
        Self::Protocol(e)
    }
}
fn bits(profile: ProfileId) -> usize {
    match profile {
        ProfileId::Robust => 1,
        ProfileId::Balanced => 2,
    }
}
fn level(index: usize, profile: ProfileId) -> u8 {
    match profile {
        ProfileId::Robust => (index * 255) as u8,
        ProfileId::Balanced => (index * 85) as u8,
    }
}
fn sync(x: usize) -> u8 {
    if (x / 8) % 2 == 0 {
        0
    } else {
        255
    }
}

impl VisualFrame {
    pub fn encode(packet: &Packet, grid: GridConfig) -> Result<Self, Error> {
        let bytes = packet.encode()?;
        if bytes.len() > grid.packet_capacity(packet.profile)? {
            return Err(Error::Capacity);
        }
        let mut cells = vec![0; grid.columns * grid.rows];
        for x in 1..grid.columns - 1 {
            cells[x] = sync(x);
            cells[(grid.rows - 1) * grid.columns + x] = 255 - sync(x);
            cells[grid.columns + x] = (((x - 1) * 4 / (grid.columns - 2)) * 85) as u8;
        }
        let mut metadata = Vec::from(&b"VFG0"[..]);
        metadata.extend_from_slice(&[packet.profile as u8, 0]);
        metadata.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        for i in 0..BOOTSTRAP_CELLS {
            cells[2 * grid.columns + 1 + i] = ((metadata[i / 8] >> (7 - i % 8)) & 1) * 255;
        }
        let bpc = bits(packet.profile);
        let per_byte = 8 / bpc;
        for (i, &byte) in bytes.iter().enumerate() {
            for part in 0..per_byte {
                let data_index = i * per_byte + part;
                let y = 3 + data_index / (grid.columns - 2);
                let x = 1 + data_index % (grid.columns - 2);
                let index = (usize::from(byte) >> (8 - bpc * (part + 1))) & ((1 << bpc) - 1);
                cells[y * grid.columns + x] = level(index, packet.profile);
            }
        }
        Ok(Self { grid, cells })
    }
    fn validate(&self) -> Result<(), Error> {
        self.grid.validate()?;
        if self.cells.len() != self.grid.columns * self.grid.rows {
            return Err(Error::Dimensions);
        }
        Ok(())
    }
    pub fn decode(&self) -> Result<Packet, Error> {
        self.validate()?;
        let grid = self.grid;
        let mut calibration = [0u8; 4];
        for (i, value) in calibration.iter_mut().enumerate() {
            let x = 1 + (grid.columns - 2) * (2 * i + 1) / 8;
            *value = self.cells[grid.columns + x];
        }
        if calibration
            .windows(2)
            .any(|w| i16::from(w[1]) - i16::from(w[0]) < 25)
        {
            return Err(Error::Calibration);
        }
        let threshold = (u16::from(calibration[0]) + u16::from(calibration[3])) / 2;
        let binary = |v: u8| u8::from(u16::from(v) > threshold);
        let mut mismatches = 0;
        for x in 1..grid.columns - 1 {
            mismatches += usize::from(binary(self.cells[x]) != binary(sync(x)));
            mismatches += usize::from(
                binary(self.cells[(grid.rows - 1) * grid.columns + x]) != binary(255 - sync(x)),
            );
        }
        if mismatches * 20 > 2 * (grid.columns - 2) {
            return Err(Error::Sync);
        }
        let mut metadata = [0u8; 10];
        for i in 0..BOOTSTRAP_CELLS {
            metadata[i / 8] |= binary(self.cells[2 * grid.columns + 1 + i]) << (7 - i % 8);
        }
        if &metadata[..4] != b"VFG0" || metadata[5] != 0 {
            return Err(Error::Metadata);
        }
        let profile = ProfileId::try_from(metadata[4])?;
        let length = u32::from_le_bytes(metadata[6..10].try_into().unwrap()) as usize;
        if length < HEADER_LEN + CRC_LEN || length > grid.packet_capacity(profile)? {
            return Err(Error::Capacity);
        }
        let mut bytes = vec![0u8; length];
        let bpc = bits(profile);
        let per_byte = 8 / bpc;
        for (i, byte) in bytes.iter_mut().enumerate() {
            for part in 0..per_byte {
                let index = i * per_byte + part;
                let value = self.cells[(3 + index / (grid.columns - 2)) * grid.columns
                    + 1
                    + index % (grid.columns - 2)];
                let symbol = match profile {
                    ProfileId::Robust => binary(value),
                    ProfileId::Balanced => {
                        calibration
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, &v)| value.abs_diff(v))
                            .unwrap()
                            .0 as u8
                    }
                };
                *byte |= symbol << (8 - bpc * (part + 1));
            }
        }
        let packet = Packet::decode(&bytes)?;
        if packet.profile != profile {
            return Err(Error::Metadata);
        }
        Ok(packet)
    }
    pub fn render(&self, cell_size: usize) -> Result<LumaImage, Error> {
        self.validate()?;
        if !(2..=32).contains(&cell_size) {
            return Err(Error::Dimensions);
        }
        let width = self.grid.columns * cell_size;
        let height = self.grid.rows * cell_size;
        if width > 8192 || height > 8192 {
            return Err(Error::Dimensions);
        }
        let mut pixels = vec![0; width * height];
        for y in 0..height {
            for (x, pixel) in pixels[y * width..(y + 1) * width].iter_mut().enumerate() {
                *pixel = self.cells[(y / cell_size) * self.grid.columns + x / cell_size];
            }
        }
        Ok(LumaImage {
            width,
            height,
            pixels,
        })
    }
    /// The image is the exact frame rectangle. Sample nine central points per cell after scaling.
    pub fn sample(image: &LumaImage, grid: GridConfig) -> Result<Self, Error> {
        image.validate()?;
        grid.validate()?;
        if image.width < grid.columns * 2 || image.height < grid.rows * 2 {
            return Err(Error::Dimensions);
        }
        let mut cells = vec![0; grid.columns * grid.rows];
        for y in 0..grid.rows {
            for x in 0..grid.columns {
                let mut sum = 0u32;
                for dy in [3, 4, 5] {
                    for dx in [3, 4, 5] {
                        let px = (x * 8 + dx) * image.width / (grid.columns * 8);
                        let py = (y * 8 + dy) * image.height / (grid.rows * 8);
                        sum += u32::from(image.pixels[py * image.width + px]);
                    }
                }
                cells[y * grid.columns + x] = (sum / 9) as u8;
            }
        }
        Ok(Self { grid, cells })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vft_protocol::{PacketType, SymbolId};
    #[test]
    fn profiles_roundtrip_full_capacity_and_calibration() {
        for profile in [ProfileId::Robust, ProfileId::Balanced] {
            let grid = GridConfig::for_profile(profile);
            let packet = Packet {
                session: [1; 16],
                object: [2; 16],
                kind: PacketType::RepairSymbol,
                symbol: SymbolId {
                    generation: 1,
                    index: 99,
                },
                profile,
                payload: (0..grid.payload_capacity(profile).unwrap())
                    .map(|i| i as u8)
                    .collect(),
            };
            let frame = VisualFrame::encode(&packet, grid).unwrap();
            assert_eq!(frame.decode().unwrap(), packet);
            let mut image = frame.render(4).unwrap();
            for pixel in &mut image.pixels {
                *pixel = (u16::from(*pixel) * 3 / 4 + 20) as u8;
            }
            assert_eq!(
                VisualFrame::sample(&image, grid).unwrap().decode().unwrap(),
                packet
            );
        }
    }
    #[test]
    fn malformed_grid_and_missing_sync_rejected() {
        let grid = GridConfig::for_profile(ProfileId::Robust);
        assert!(VisualFrame {
            grid,
            cells: vec![]
        }
        .decode()
        .is_err());
        assert!(VisualFrame {
            grid,
            cells: vec![128; grid.columns * grid.rows]
        }
        .decode()
        .is_err());
        assert!(LumaImage {
            width: usize::MAX,
            height: 1,
            pixels: vec![]
        }
        .validate()
        .is_err());
    }
}
