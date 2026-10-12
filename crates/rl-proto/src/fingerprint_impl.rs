/// FNV-1a 64 over `(relative path, contents)` of every `.rs` file, sorted by path.
pub fn fingerprint(files: &[(&str, &[u8])]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: &[u8]| {
        for &x in b {
            h = (h ^ x as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for (path, body) in files {
        eat(path.as_bytes());
        eat(&[0]);
        eat(body);
        eat(&[0]);
    }
    h
}
