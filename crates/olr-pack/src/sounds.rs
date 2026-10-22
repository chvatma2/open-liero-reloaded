//! Limits on the sound files a pack may make the game decode: a hostile
//! pack must not make `olr-audio` decode hours of audio into memory. Here
//! (moved from `olr-content` in issue #19) so the editors' imports check
//! a sound as the game will.
//!
//! OLX's largest sound (MW 1.0's `infest.ogg`) is 36 KB and a few seconds
//! long, so the limits leave a wide margin.

/// The largest sound file, in bytes.
pub const MAX_SOUND_BYTES: usize = 2 << 20;

/// The longest sound, in seconds, as its header gives it (an Ogg stream's
/// last granule position over its sample rate; a WAV's data size over its
/// byte rate).
pub const MAX_SOUND_SECONDS: f64 = 60.0;

/// Why sound file `bytes` must not be decoded, if it must not: larger than
/// [`MAX_SOUND_BYTES`], or longer than [`MAX_SOUND_SECONDS`] by its header.
/// A file this does not recognise is left to the decoder, which refuses
/// what it cannot read.
pub fn check_sound(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_SOUND_BYTES {
        return Err(format!("{} bytes, more than {MAX_SOUND_BYTES}", bytes.len()));
    }
    let seconds = if bytes.starts_with(b"OggS") {
        ogg_seconds(bytes)
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        wav_seconds(bytes)
    } else {
        None
    };
    match seconds {
        Some(s) if s > MAX_SOUND_SECONDS => Err(format!("{s:.0} s long, more than {MAX_SOUND_SECONDS} s")),
        _ => Ok(()),
    }
}

/// Whether `bytes` starts as a sound the game reads (issue #19: what the
/// editors import): Ogg (`OggS`) or WAV (`RIFF` … `WAVE`).
pub fn is_sound(bytes: &[u8]) -> bool {
    bytes.starts_with(b"OggS") || bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE")
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// An Ogg Vorbis stream's length: the largest granule position of its
/// pages (samples per channel) over the identification header's rate.
/// `None` when it is not Vorbis or its first page is cut short; a rate of
/// 0 is infinitely long.
fn ogg_seconds(b: &[u8]) -> Option<f64> {
    // Page header: "OggS", version, type, granule (8), serial (4),
    // sequence (4), CRC (4), segment count, segment table.
    let segments = usize::from(*b.get(26)?);
    let first_packet = 27 + segments;
    if b.get(first_packet..first_packet + 7)? != b"\x01vorbis" {
        return None;
    }
    let rate = u32_at(b, first_packet + 12)?;
    let (mut at, mut granule) = (0usize, 0u64);
    while b.get(at..at + 4) == Some(b"OggS") {
        let Some(n) = b.get(at + 26).map(|&n| usize::from(n)) else { break };
        let Some(table) = b.get(at + 27..at + 27 + n) else { break };
        let g = u64::from_le_bytes(b[at + 6..at + 14].try_into().ok()?);
        // -1 marks a page on which no packet ends.
        if g != u64::MAX {
            granule = granule.max(g);
        }
        at += 27 + n + table.iter().map(|&s| usize::from(s)).sum::<usize>();
    }
    Some(if rate == 0 { f64::INFINITY } else { granule as f64 / f64::from(rate) })
}

/// A WAV file's length: its `data` chunk's size over the `fmt ` chunk's
/// byte rate. `None` without both chunks.
fn wav_seconds(b: &[u8]) -> Option<f64> {
    let (mut at, mut byte_rate, mut data) = (12usize, None, None);
    while let (Some(id), Some(size)) = (b.get(at..at + 4), u32_at(b, at + 4)) {
        match id {
            b"fmt " => byte_rate = u32_at(b, at + 16),
            b"data" => data = Some(size),
            _ => {}
        }
        // Chunks are padded to an even size.
        at = at.checked_add(8)?.checked_add(size as usize + (size as usize & 1))?;
    }
    let (rate, data) = (byte_rate?, data?);
    Some(if rate == 0 { f64::INFINITY } else { f64::from(data) / f64::from(rate) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An Ogg page: header, segment table for one packet of `body`.
    fn page(granule: u64, body: &[u8]) -> Vec<u8> {
        assert!(body.len() < 255);
        let mut p = b"OggS\0\x02".to_vec();
        p.extend_from_slice(&granule.to_le_bytes());
        p.extend_from_slice(&[0; 12]);
        p.push(1);
        p.push(body.len() as u8);
        p.extend_from_slice(body);
        p
    }

    fn vorbis(rate: u32, granules: &[u64]) -> Vec<u8> {
        let mut id = b"\x01vorbis".to_vec();
        id.extend_from_slice(&0u32.to_le_bytes());
        id.push(2);
        id.extend_from_slice(&rate.to_le_bytes());
        id.extend_from_slice(&[0; 13]);
        let mut b = page(0, &id);
        for &g in granules {
            b.extend(page(g, &[0; 40]));
        }
        b
    }

    fn wav(byte_rate: u32, data: u32) -> Vec<u8> {
        let mut b = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&[1, 0, 1, 0]);
        b.extend_from_slice(&22050u32.to_le_bytes());
        b.extend_from_slice(&byte_rate.to_le_bytes());
        b.extend_from_slice(&[1, 0, 8, 0]);
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        b.resize(b.len() + data.min(1000) as usize, 0);
        b
    }

    #[test]
    fn sounds_too_large_or_too_long_are_refused() {
        // Short sounds pass, as does a stream whose last page has no granule.
        check_sound(&vorbis(22050, &[22050, 44100, u64::MAX])).unwrap();
        check_sound(&wav(22050, 1000)).unwrap();
        // Too long by the header.
        let long = check_sound(&vorbis(22050, &[22050, 22050 * 3600])).unwrap_err();
        assert!(long.contains("3600 s"), "{long}");
        assert!(check_sound(&vorbis(0, &[1])).is_err(), "no sample rate");
        assert!(check_sound(&wav(1, 1000)).is_err(), "1000 s of WAV");
        // Too large, whatever it is.
        let big = vec![0u8; MAX_SOUND_BYTES + 1];
        assert!(check_sound(&big).unwrap_err().contains("bytes"));
        // Cut short or unknown: the decoder decides, nothing panics.
        let v = vorbis(22050, &[22050]);
        for n in 0..v.len() {
            let _ = check_sound(&v[..n]);
        }
        let w = wav(22050, 100);
        for n in 0..w.len() {
            let _ = check_sound(&w[..n]);
        }
        check_sound(b"not a sound").unwrap();
        assert!(is_sound(&v) && is_sound(&w) && !is_sound(b"not a sound") && !is_sound(b"RIFF\0\0\0\0AVI "));
    }
}
