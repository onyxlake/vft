//! Optional human-entered text contract. This never travels in a visual Control packet.
use crate::{crc32c, Digest, Manifest, Sha256};
use alloc::{format, string::String, vec::Vec};
use core::fmt;

pub const MAX_BYTES: usize = 4096;
pub const MAX_TARGETS: usize = 256;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// Internal zero-based generation; text group numbers are one-based.
    pub generation: u32,
    /// None is the simple-list default of sixteen packets.
    pub deficit: Option<u32>,
}
impl Target {
    pub fn budget(&self) -> u32 {
        self.deficit.map_or(16, |d| (d + d.div_ceil(2) + 4).max(8))
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepairRequest {
    pub id: Option<u64>,
    pub targets: Vec<Target>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Syntax,
    Limit,
    Group,
    Deficit,
    Duplicate,
    Tag,
    Crc,
    Manifest,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Repair request: {self:?}")
    }
}
impl core::error::Error for Error {}
pub fn manifest_tag(manifest: &Manifest) -> Result<String, Error> {
    let digest = Sha256::digest(manifest.encode().map_err(|_| Error::Manifest)?);
    Ok(digest[..8].iter().map(|b| format!("{b:02X}")).collect())
}
fn decimal(s: &str) -> Result<u32, Error> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Syntax);
    }
    s.parse().map_err(|_| Error::Limit)
}
fn hex(s: &str, len: usize) -> Result<u64, Error> {
    if s.len() != len || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Syntax);
    }
    u64::from_str_radix(s, 16).map_err(|_| Error::Syntax)
}
impl RepairRequest {
    pub fn validate(&self, manifest: &Manifest) -> Result<(), Error> {
        manifest.validate().map_err(|_| Error::Manifest)?;
        if self.targets.is_empty() || self.targets.len() > MAX_TARGETS {
            return Err(Error::Limit);
        }
        let mut total = 0u32;
        for (i, t) in self.targets.iter().enumerate() {
            let k = manifest
                .generation_symbols(t.generation)
                .map_err(|_| Error::Group)?;
            if t.deficit.is_some_and(|d| d == 0 || d > k) {
                return Err(Error::Deficit);
            }
            if self.id.is_some() && t.deficit.is_none() {
                return Err(Error::Syntax);
            }
            if self.targets[..i]
                .iter()
                .any(|p| p.generation == t.generation)
            {
                return Err(Error::Duplicate);
            }
            let budget = t.budget();
            if budget > 128 {
                return Err(Error::Limit);
            }
            total = total.checked_add(budget).ok_or(Error::Limit)?;
        }
        if total > 32768 {
            return Err(Error::Limit);
        }
        Ok(())
    }
    pub fn total_budget(&self) -> u32 {
        self.targets.iter().map(Target::budget).sum()
    }
    pub fn entries(&self) -> String {
        let mut targets = self.targets.clone();
        targets.sort_by_key(|t| t.generation);
        targets
            .iter()
            .map(|t| match t.deficit {
                Some(d) => format!("{}:{d}", t.generation + 1),
                None => format!("{}", t.generation + 1),
            })
            .collect::<Vec<_>>()
            .join(",")
    }
    pub fn encode(&self, manifest: &Manifest) -> Result<String, Error> {
        self.validate(manifest)?;
        let id = self.id.ok_or(Error::Syntax)?;
        let body = format!(
            "VFR1/{}/{id:016X}/{}",
            manifest_tag(manifest)?,
            self.entries()
        );
        let code = format!("{body}/{:08X}", crc32c(body.as_bytes()));
        if code.len() > MAX_BYTES {
            return Err(Error::Limit);
        }
        Ok(code)
    }
    pub fn parse(text: &str, manifest: &Manifest) -> Result<Self, Error> {
        if text.len() > MAX_BYTES {
            return Err(Error::Limit);
        }
        let text = text.trim();
        if !text.is_ascii() || text.bytes().any(|b| b.is_ascii_control()) {
            return Err(Error::Syntax);
        }
        let (id, entries, expected_crc) = if text.starts_with("VFR") || text.contains('/') {
            let parts: Vec<_> = text.split('/').collect();
            if parts.len() != 5 || parts[0] != "VFR1" {
                return Err(Error::Syntax);
            }
            let tag = hex(parts[1], 16)?;
            if tag != hex(&manifest_tag(manifest)?, 16)? {
                return Err(Error::Tag);
            }
            (
                Some(hex(parts[2], 16)?),
                parts[3],
                Some(hex(parts[4], 8)? as u32),
            )
        } else {
            (None, text, None)
        };
        let mut targets = Vec::new();
        for entry in entries.split(',') {
            let entry = if id.is_some() { entry } else { entry.trim() };
            let (first, last, deficit) = if let Some((group, deficit)) = entry.split_once(':') {
                let g = decimal(group)?;
                (g, g, Some(decimal(deficit)?))
            } else if id.is_none() {
                match entry.split_once('-') {
                    Some((a, b)) => (decimal(a)?, decimal(b)?, None),
                    None => {
                        let g = decimal(entry)?;
                        (g, g, None)
                    }
                }
            } else {
                return Err(Error::Syntax);
            };
            if first == 0 || last < first || last > manifest.generations() {
                return Err(Error::Group);
            }
            let count = last
                .checked_sub(first)
                .and_then(|n| n.checked_add(1))
                .ok_or(Error::Limit)?;
            if u64::from(count) + targets.len() as u64 > MAX_TARGETS as u64 {
                return Err(Error::Limit);
            }
            for group in first..=last {
                targets.push(Target {
                    generation: group - 1,
                    deficit,
                });
            }
        }
        let request = Self { id, targets };
        request.validate(manifest)?;
        if let Some(crc) = expected_crc {
            // Unique decimal ordering is mandatory; only hexadecimal case is normalized.
            if request.entries() != entries {
                return Err(Error::Syntax);
            }
            let body = format!(
                "VFR1/{}/{:016X}/{}",
                manifest_tag(manifest)?,
                id.unwrap(),
                entries
            );
            if crc32c(body.as_bytes()) != crc {
                return Err(Error::Crc);
            }
        }
        Ok(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest() -> Manifest {
        Manifest {
            session: [0x11; 16],
            object: [0x22; 16],
            filename: String::from("abc.bin"),
            file_size: 3,
            sha256: Sha256::digest(b"abc").into(),
            symbol_size: 2,
            source_symbols: 2,
            profile: crate::ProfileId::Balanced,
        }
    }
    #[test]
    fn independent_golden() {
        let m = manifest();
        let code = include_str!("../../../tests/golden/repair-request-v1.txt").trim();
        let r = RepairRequest::parse(code, &m).unwrap();
        assert_eq!(r.id, Some(0x0123456789abcdef));
        assert_eq!(r.encode(&m).unwrap(), code);
        assert_eq!(r.total_budget(), 8);
        assert!(RepairRequest::parse(&code.replace("1:1", "1:2"), &m).is_err());
        let mut other = m.clone();
        other.session[0] ^= 1;
        assert_eq!(RepairRequest::parse(code, &other), Err(Error::Tag));
        assert_eq!(RepairRequest::parse("1,1", &m), Err(Error::Duplicate));
        for text in [
            "",
            "0",
            "2",
            "1:0",
            "1:3",
            "1-0",
            "1-4294967295",
            "1\n",
            "一",
            "VFR2/x",
            "01:1/",
        ] {
            // Outer whitespace is allowed.
            if text != "1\n" {
                assert!(RepairRequest::parse(text, &m).is_err(), "{text}");
            }
        }
        assert!(RepairRequest::parse(&"1".repeat(MAX_BYTES + 1), &m).is_err());
    }
    #[test]
    fn limits_ranges_and_budgets() {
        let mut m = manifest();
        m.file_size = 64 * 300 * 2;
        m.source_symbols = 64 * 300;
        assert_eq!(
            RepairRequest::parse("18:3,42:1,90:7,91:2", &m)
                .unwrap()
                .total_budget(),
            40
        );
        assert_eq!(
            RepairRequest::parse("1-256", &m).unwrap().targets.len(),
            256
        );
        assert_eq!(RepairRequest::parse("1-257", &m), Err(Error::Limit));
        assert_eq!(RepairRequest::parse("1-3,3", &m), Err(Error::Duplicate));
        assert_eq!(
            RepairRequest::parse("18, 42,90-91", &m)
                .unwrap()
                .total_budget(),
            64
        );
    }
}
