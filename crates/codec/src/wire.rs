use crate::{bad, require};
use std::io;

pub const MAGIC: [u8; 4] = 0x9cb02ef5u32.to_le_bytes();
const GOLD: u64 = 0x9e3779b97f4a7c15;
pub fn header_sum(h: &[u8; 16]) -> u16 {
    let a = u64::from_le_bytes(h[..8].try_into().unwrap());
    let b = u64::from_le_bytes(h[8..].try_into().unwrap()) & 0xffff_ffff_ffff;
    let c = 0xd2d84a61d2d84a61;
    ((a ^ c)
        .wrapping_mul(c)
        .wrapping_add(b)
        .wrapping_add(GOLD)
        .wrapping_mul(GOLD)
        >> 48) as u16
}
pub fn block_header(kind: u8, size: u32) -> [u8; 8] {
    let mut h = [0; 8];
    h[0] = kind;
    h[3..7].copy_from_slice(&size.to_le_bytes());
    h[7] = ((u64::from_le_bytes(h) ^ GOLD).wrapping_mul(GOLD) >> 56) as u8;
    h
}
pub fn parse_block(h: [u8; 8]) -> io::Result<(u8, u32)> {
    let size = u32::from_le_bytes(h[3..7].try_into().unwrap());
    require(h == block_header(h[0], size), "invalid block header")?;
    Ok((h[0], size))
}
pub fn checksum(data: &[u8], seed: u64) -> u32 {
    zone!("checksum");
    use rapidhash::v3::{RapidSecrets, rapidhash_v3_seeded};
    let h = rapidhash_v3_seeded(data, &RapidSecrets::seed_cpp(seed));
    (h ^ (h >> 32)) as u32
}
pub fn fold(previous: u64, sum: u32) -> u64 {
    let x = (u64::from(sum) + 1).wrapping_mul(GOLD);
    let p = u128::from(previous ^ x) * 0x2545f4914f6cdd1d;
    (p ^ (p >> 64)) as u64
}
pub fn add(x: u64, y: u64) -> io::Result<u64> {
    x.checked_add(y).ok_or_else(|| bad("frame size overflow"))
}
fn width(x: u64) -> usize {
    (64 - x.leading_zeros()).div_ceil(8).max(1) as usize
}
pub fn footer(plain: u64, prefix: u64, digest: Option<u64>) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(25);
    if let Some(d) = digest {
        bytes.extend(d.to_le_bytes());
    }
    let nd = width(plain);
    bytes.extend(&plain.to_le_bytes()[..nd]);
    let base = add(prefix, bytes.len() as u64 + 1)?;
    let mut nf = 1;
    loop {
        let size = add(base, nf as u64)?;
        if width(size) <= nf {
            bytes.extend(&size.to_le_bytes()[..nf]);
            bytes.push(((nf as u8 - 1) << 4) | (nd as u8 - 1));
            return Ok(bytes);
        }
        nf += 1;
    }
}
