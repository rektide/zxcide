use super::{cursor, tiles32};
use core::arch::x86_64::{__cpuid, _pdep_u64};
use fearless_simd::Level;
use std::sync::OnceLock;

pub(super) fn select(level: Level) -> Option<fearless_simd::Avx2> {
    // BMI2 establishes legality, not speed (Zen 2 PDEP is microcoded).
    // Start with the measured Vermeer signature only; no blanket BMI2 tier.
    static FAST: OnceLock<bool> = OnceLock::new();
    let Level::Avx2(proof) = level else { return None };
    FAST.get_or_init(|| {
        let id = __cpuid(0);
        id.ebx == u32::from_le_bytes(*b"Auth")
            && id.edx == u32::from_le_bytes(*b"enti")
            && id.ecx == u32::from_le_bytes(*b"cAMD")
            && id.eax >= 1
            && __cpuid(1).eax & 0x0fff_3fff == 0x00a2_0f12
    }).then_some(proof)
}

static BYTE_MASKS: [u64; 256] = {
    let mut table = [0; 256];
    let mut b = 0;
    while b < 256 {
        let mut i = 0;
        while i < 8 {
            table[b] |= (((b >> i) & 1) as u64 * 255) << (8 * i);
            i += 1;
        }
        b += 1;
    }
    table
};

fearless_simd::kernel!(
    #[inline(always)]
    fn deposit_word(_simd: Avx2, bits: u8, a: u64, b: u64) -> [u8; 8] {
        let mask = BYTE_MASKS[usize::from(bits)];
        (_pdep_u64(a, !mask) | _pdep_u64(b, mask)).to_le_bytes()
    }
);

#[inline(always)]
fn load(slab: &[u8; 32], offset: usize) -> u64 {
    u64::from_le_bytes(*slab[offset.min(24)..].first_chunk().unwrap())
}

fearless_simd::kernel!(
    pub(super) fn merge32_deposit(simd: Avx2, region: &[u8], dst: &mut [u8], bitmap: &[u8], left: usize) -> usize {
        let (groups, l, r) = tiles32(region, dst, bitmap, left, |out, a, b, bits| {
            let [o0, o1, o2, o3] = out.as_chunks_mut::<8>().0 else { unreachable!() };
            let p1 = (bits as u8).count_ones() as usize;
            let p2 = (bits as u16).count_ones() as usize;
            let p3 = (bits & 0x00ff_ffff).count_ones() as usize;
            let [b0, b1, b2, b3] = bits.to_le_bytes();
            *o0 = deposit_word(simd, b0, load(a, 0), load(b, 0));
            *o1 = deposit_word(simd, b1, load(a, 8 - p1), load(b, p1));
            *o2 = deposit_word(simd, b2, load(a, 16 - p2), load(b, p2));
            *o3 = deposit_word(simd, b3, load(a, 24 - p3), load(b, p3));
        });
        cursor(region, &mut dst[32 * groups..], &bitmap[4 * groups..], [l, r]);
        groups
    }
);
