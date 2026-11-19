use crate::{
    bad,
    entropy::Tree,
    require,
    wire::{checksum, header_sum},
};
use std::io;

/// An authenticated-by-ID (non-cryptographic) `.zxd` v2 dictionary.
#[derive(Debug)]
pub struct Dictionary {
    pub(crate) content: Vec<u8>,
    pub(crate) tree: Option<Tree>,
    pub(crate) id: u32,
}
impl Dictionary {
    /// Parse and validate the content, optional shared codebook and their binding.
    pub fn from_bytes(data: &[u8]) -> io::Result<Self> {
        let h: &[u8; 16] = data
            .get(..16)
            .ok_or_else(|| bad("short dictionary"))?
            .try_into()
            .unwrap();
        require(
            h[..4] == 0x9cb0d1c7u32.to_le_bytes() && h[4] == 2,
            "unsupported dictionary",
        )?;
        require(
            h[5] == 0 && h[12..14] == [0, 0] && h[14..] == header_sum(h).to_le_bytes(),
            "invalid dictionary header",
        )?;
        let n = u16::from_le_bytes([h[6], h[7]]) as usize;
        require(n > 0 && data.len() == 16 + n + 128, "invalid dictionary size")?;
        let (content, table) = data[16..].split_at(n);
        let id = checksum(table, u64::from(checksum(content, 0)));
        require(h[8..12] == id.to_le_bytes(), "dictionary ID mismatch")?;
        let tree = if table.iter().all(|&b| b == 0) {
            None
        } else {
            Some(Tree::new(table)?)
        };
        Ok(Self {
            content: content.to_vec(),
            tree,
            id,
        })
    }
    pub fn id(&self) -> u32 {
        self.id
    }
}
