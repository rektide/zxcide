
// Appended to block.rs in the archived replay source tree, not shipped.
pub fn replay_huf<'a>(
    data: &'a [u8],
    count: usize,
    out: &'a mut Vec<u8>,
    merge: &mut Vec<u8>,
) -> io::Result<&'a [u8]> {
    match section(2, data, count, None, out, merge)? {
        Cow::Borrowed(bytes) => Ok(bytes),
        Cow::Owned(_) => unreachable!("inline Huffman always fills reusable output"),
    }
}
