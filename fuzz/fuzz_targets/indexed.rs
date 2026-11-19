#![no_main]
use libfuzzer_sys::fuzz_target;
use zxc_seek::Archive;

// Keep inputs in wire format so the frozen C corpus seeds the target directly.
fuzz_target!(|data: &[u8]| {
    if let Ok(archive) = Archive::open(data) {
        let n = archive.len();
        for start in [0, n / 2, n.saturating_sub(1), n] {
            let end = start.saturating_add(4097).min(n);
            let mut plain = Vec::new();
            if archive.copy_range(start..end, &[], &mut plain).is_ok() {
                assert_eq!(plain.len() as u64, end - start);
            }
        }
    }
});
