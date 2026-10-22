//! The frame delta codec: which slots' inputs a host datagram carries for
//! each tick, as changes against the slot's previous tick.

use alloc::vec::Vec;

use crate::{Frame, ProtoError};

fn start(first: u32, slots: usize, base: &dyn Fn(usize) -> Option<u16>) -> [Option<u16>; 32] {
    let mut prev: [Option<u16>; 32] = [None; 32];
    for (s, p) in prev.iter_mut().enumerate().take(slots) {
        *p = if first == 0 { Some(0) } else { base(s) };
    }
    prev
}

/// Encodes `ticks` frames from `first`: `value(tick, slot)` is the final
/// input when known; `base(slot)` the receiver's value at `first - 1`.
pub fn encode_frames(
    first: u32,
    ticks: u32,
    slots: usize,
    value: &dyn Fn(u32, usize) -> Option<u16>,
    base: &dyn Fn(usize) -> Option<u16>,
) -> Vec<Frame> {
    let slots = slots.min(32);
    let mut out = Vec::with_capacity(ticks as usize);
    // What the receiver will know for the previous tick, per slot.
    let mut prev = start(first, slots, base);
    for k in 0..ticks {
        let tick = first.wrapping_add(k);
        let (mut known, mut changed, mut keys) = (0u32, 0u32, Vec::new());
        let mut next = [None; 32];
        for s in 0..slots {
            if let Some(v) = value(tick, s) {
                known |= 1 << s;
                if prev[s] != Some(v) {
                    changed |= 1 << s;
                    keys.push(v);
                }
                next[s] = Some(v);
            }
        }
        prev = next;
        out.push(Frame { known, changed, keys });
    }
    out
}

/// Decodes frames, calling `out(tick, slot, keys)` for every known input.
/// `out` may run for earlier frames before a later one proves invalid:
/// callers collect first and apply on `Ok`.
pub fn decode_frames(
    first: u32,
    frames: &[Frame],
    slots: usize,
    base: &dyn Fn(usize) -> Option<u16>,
    out: &mut dyn FnMut(u32, usize, u16),
) -> Result<(), ProtoError> {
    if slots > 32 {
        return Err(ProtoError::Invalid);
    }
    let mut prev = start(first, slots, base);
    for (k, f) in frames.iter().enumerate() {
        let valid = slots >= 32 || f.known >> slots == 0;
        if !valid || f.changed & !f.known != 0 || f.keys.len() != f.changed.count_ones() as usize {
            return Err(ProtoError::Invalid);
        }
        let tick = u32::try_from(k).ok().and_then(|k| first.checked_add(k)).ok_or(ProtoError::Invalid)?;
        let mut keys = f.keys.iter();
        let mut next = [None; 32];
        for s in 0..slots {
            if f.known & (1 << s) == 0 {
                continue;
            }
            let v = if f.changed & (1 << s) != 0 {
                *keys.next().ok_or(ProtoError::Invalid)?
            } else {
                prev[s].ok_or(ProtoError::Invalid)?
            };
            next[s] = Some(v);
            out(tick, s, v);
        }
        prev = next;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    /// A table of known inputs: `table[t][s]`.
    fn table(rows: &[&[Option<u16>]]) -> Vec<Vec<Option<u16>>> {
        rows.iter().map(|r| r.to_vec()).collect()
    }

    fn round_trip(t: &[Vec<Option<u16>>], first: u32, base: &[Option<u16>]) -> Vec<Vec<Option<u16>>> {
        let slots = base.len();
        let frames =
            encode_frames(first, t.len() as u32, slots, &|tick, s| t[(tick - first) as usize][s], &|s| {
                base[s]
            });
        let mut got = vec![vec![None; slots]; t.len()];
        decode_frames(first, &frames, slots, &|s| base[s], &mut |tick, s, v| {
            got[(tick - first) as usize][s] = Some(v)
        })
        .unwrap();
        got
    }

    #[test]
    fn frames_round_trip_with_partial_knowledge() {
        let t = table(&[
            &[Some(1), Some(2), None],
            &[Some(1), Some(3), None],
            &[None, Some(3), Some(9)],
            &[Some(4), Some(3), Some(9)],
        ]);
        assert_eq!(round_trip(&t, 10, &[Some(1), Some(0), None]), t);
    }

    #[test]
    fn unchanged_inputs_cost_no_key_bytes() {
        let t: Vec<Vec<Option<u16>>> = (0..8).map(|_| vec![Some(5); 32]).collect();
        let frames = encode_frames(100, 8, 32, &|tick, s| t[(tick - 100) as usize][s], &|_| Some(5));
        assert!(frames.iter().all(|f| f.keys.is_empty() && f.changed == 0));
    }

    #[test]
    fn tick_zero_starts_from_zero_keys() {
        let t = table(&[&[Some(0), Some(7)]]);
        let frames = encode_frames(0, 1, 2, &|_, s| t[0][s], &|_| Some(0));
        assert_eq!(frames[0].changed, 0b10);
    }

    #[test]
    fn an_unresolvable_repeat_is_refused() {
        // Slot 0 claims "unchanged" at the first frame, but the receiver lacks its base.
        let frames = vec![Frame { known: 0b1, changed: 0, keys: vec![] }];
        let r = decode_frames(5, &frames, 1, &|_| None, &mut |_, _, _| {});
        assert_eq!(r, Err(ProtoError::Invalid));
    }

    #[test]
    fn inconsistent_frames_are_refused() {
        let bad = [
            Frame { known: 0b01, changed: 0b10, keys: vec![1] }, // changed outside known
            Frame { known: 0b11, changed: 0b11, keys: vec![1] }, // too few keys
            Frame { known: 1 << 5, changed: 1 << 5, keys: vec![1] }, // slot beyond `slots`
        ];
        for f in bad {
            assert_eq!(decode_frames(1, &[f], 2, &|_| Some(0), &mut |_, _, _| {}), Err(ProtoError::Invalid));
        }
    }

    #[test]
    fn tick_zero_ignores_the_base() {
        let t = table(&[&[Some(3), Some(0)], &[Some(3), Some(1)]]);
        assert_eq!(round_trip(&t, 0, &[Some(9), Some(9)]), t);
    }

    #[test]
    fn a_value_repeated_after_a_gap_is_sent_again() {
        let t = table(&[&[Some(1)], &[None], &[Some(1)]]);
        let frames = encode_frames(7, 3, 1, &|tick, s| t[(tick - 7) as usize][s], &|_| Some(1));
        assert_eq!(frames[2].changed, 0b1);
        assert_eq!(round_trip(&t, 7, &[Some(1)]), t);
    }
}
