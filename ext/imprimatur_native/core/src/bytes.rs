//! Checked integer reads shared by binary format parsers.

#[cfg(test)]
mod tests;

fn array<const N: usize>(data: &[u8], at: usize) -> Option<[u8; N]> {
    data.get(at..at.checked_add(N)?)?.try_into().ok()
}

pub fn u16_le(data: &[u8], at: usize) -> Option<u16> {
    array(data, at).map(u16::from_le_bytes)
}

pub fn u32_le(data: &[u8], at: usize) -> Option<u32> {
    array(data, at).map(u32::from_le_bytes)
}
