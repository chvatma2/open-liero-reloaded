//! The shot list's network round trip, byte for byte: `CShootList::writePacket`
//! and the client's `readSingle`/`readMulti` (`src/common/CShootList.cpp:119-643`)
//! over `CBytestream`'s encodings (`src/common/CBytestream.cpp`).
//!
//! The encoding is lossy and has bugs that change what the client sees
//! (`docs/porting/olx-notes/weapons-firing.md` §4), so it is ported as bytes
//! rather than as a formula: a run of shots by the same worm and weapon goes
//! out as one `S2C_MULTISHOOT` whose later shots keep the first shot's
//! position and random seed, lose velocity increases, and get a corrupted
//! `nSpeed`. Encoding then decoding needs no allocation.

use rl_math::{TimeDiff, Vec2};

use crate::shots::{MAX_SHOOTINGS, Shot};

const S2C_SINGLESHOOT: u8 = 16;
const S2C_MULTISHOOT: u8 = 17;

// Header flags (`include/CShootList.h:26-46`).
const SMF_LARGEANGLE: u8 = 0x01;
const SMF_LARGESPEED: u8 = 0x02;
const SMF_NEGSPEED: u8 = 0x04;
// Small shot flags.
const SHF_TIMEOFF: u8 = 0x01;
const SHF_XPOSOFF: u8 = 0x02;
const SHF_YPOSOFF: u8 = 0x04;
const SHF_ANGLEOFF: u8 = 0x08;
const SHF_SPEEDOFF: u8 = 0x10;
const SHF_NG_XPOSOFF: u8 = 0x20;
const SHF_NG_YPOSOFF: u8 = 0x40;
const SHF_EXTRAFLAGS: u8 = 0x80;
// Small shot extra flags.
const SHF_NG_ANGLEOFF: u8 = 0x01;
const SHF_NG_SPEEDOFF: u8 = 0x02;
const SHF_LARGEXOFF: u8 = 0x04;
const SHF_LARGEYOFF: u8 = 0x08;
const SHF_XWRMVEL: u8 = 0x10;
const SHF_YWRMVEL: u8 = 0x20;
const SHF_NG_XWRMVEL: u8 = 0x40;
const SHF_NG_YWRMVEL: u8 = 0x80;

/// A packet is at most 20 bytes per shot (a single shot is 19, a multi
/// header 20, a small shot at most 13).
const WIRE_CAP: usize = MAX_SHOOTINGS * 20;

/// `CBytestream` over a fixed buffer. Reading past the end gives 0, as OLX does.
struct Wire {
    data: [u8; WIRE_CAP],
    len: usize,
    pos: usize,
}

impl Wire {
    fn write_byte(&mut self, b: u8) {
        if self.len < WIRE_CAP {
            self.data[self.len] = b;
            self.len += 1;
        }
    }

    /// `writeInt16`: big endian.
    fn write_i16(&mut self, v: i16) {
        let [a, b] = v.to_be_bytes();
        self.write_byte(a);
        self.write_byte(b);
    }

    /// `writeFloat`: little endian.
    fn write_f32(&mut self, v: f32) {
        for b in v.to_le_bytes() {
            self.write_byte(b);
        }
    }

    /// `write2Int12`: 12 bits each, in 3 bytes.
    fn write_2int12(&mut self, x: i16, y: i16) {
        let (x, y) = (x as u16, y as u16);
        self.write_byte((x & 0xff) as u8);
        self.write_byte((((x & 0xf00) >> 8) + ((y & 0xf) << 4)) as u8);
        self.write_byte(((y & 0xff0) >> 4) as u8);
    }

    /// `write2Int4`: 4 bits each, in 1 byte.
    fn write_2int4(&mut self, x: i32, y: i32) {
        self.write_byte(((x as u16 & 0xf) + ((y as u16 & 0xf) << 4)) as u8);
    }

    /// `ResetBitPos(); writeBit(b); SkipRestBits()`: one byte.
    fn write_release(&mut self, release: bool) {
        self.write_byte(release as u8);
    }

    fn at_end(&self) -> bool {
        self.pos >= self.len
    }

    fn read_byte(&mut self) -> u8 {
        if self.at_end() {
            return 0;
        }
        self.pos += 1;
        self.data[self.pos - 1]
    }

    fn read_i16(&mut self) -> i16 {
        let a = self.read_byte();
        let b = self.read_byte();
        i16::from_be_bytes([a, b])
    }

    fn read_f32(&mut self) -> f32 {
        let b = [self.read_byte(), self.read_byte(), self.read_byte(), self.read_byte()];
        f32::from_le_bytes(b)
    }

    fn read_2int12(&mut self) -> (i16, i16) {
        let d = [self.read_byte() as u16, self.read_byte() as u16, self.read_byte() as u16];
        ((d[0] + ((d[1] & 0xf) << 8)) as i16, (((d[1] & 0xf0) >> 4) + (d[2] << 4)) as i16)
    }

    fn read_2int4(&mut self) -> (i32, i32) {
        let b = self.read_byte();
        ((b & 0xf) as i32, ((b & 0xf0) >> 4) as i32)
    }

    /// `ResetBitPos(); readBit(); SkipRestBits()`.
    fn read_release(&mut self) -> bool {
        if self.at_end() {
            return false;
        }
        let r = self.data[self.pos] & 1 != 0;
        self.pos += 1;
        r
    }
}

/// C's `(short)f` for a coordinate (truncation; out-of-range values wrap
/// through `int` as x86-64 code does in practice).
fn short(f: f32) -> i16 {
    crate::projectile::c_int(f) as i16
}

/// `(int)f`, as the writer compares coordinates and velocities.
fn int(f: f32) -> i32 {
    crate::projectile::c_int(f)
}

/// The header both packet kinds share after `num` (`writeSingle` 146-196,
/// `writeMulti` 252-298).
fn write_header(w: &mut Wire, s: &Shot, num: Option<u8>) {
    let mut flags = 0;
    if s.angle > 255 {
        flags |= SMF_LARGEANGLE;
    }
    if s.speed > 255 || s.speed < -255 {
        flags |= SMF_LARGESPEED;
    }
    if s.speed < 0 {
        flags |= SMF_NEGSPEED;
    }
    w.write_byte(if num.is_some() { S2C_MULTISHOOT } else { S2C_SINGLESHOOT });
    w.write_byte(flags);
    w.write_byte(s.worm as u8);
    w.write_f32(s.time.seconds());
    w.write_byte(s.weapon as u8);
    if let Some(n) = num {
        w.write_byte(n);
    }
    w.write_2int12(short(s.pos.x), short(s.pos.y));
    w.write_i16(short(s.worm_vel.x));
    w.write_i16(short(s.worm_vel.y));
    w.write_byte(s.random as u8);
    w.write_byte(if flags & SMF_LARGEANGLE != 0 { s.angle - 255 } else { s.angle } as u8);
    let mut speed = s.speed;
    if flags & SMF_NEGSPEED != 0 {
        speed = speed.wrapping_neg();
    }
    if flags & SMF_LARGESPEED != 0 {
        speed = speed.wrapping_sub(255);
    }
    w.write_byte(speed as u8);
    w.write_release(s.release);
}

/// `writeSmallShot` (`CShootList.cpp:311-431`), including its misplaced
/// worm-velocity flags (`SHF_XWRMVEL`/`SHF_YWRMVEL` set in `flags`, where
/// they mean `SHF_SPEEDOFF`/`SHF_NG_XPOSOFF`).
fn write_small(w: &mut Wire, first: &Shot, s: &Shot) {
    let mut flags = 0u8;
    let mut extra = 0u8;
    let xoff = int(s.pos.x - first.pos.x).wrapping_abs();
    let yoff = int(s.pos.y - first.pos.y).wrapping_abs();
    if s.time != first.time {
        flags |= SHF_TIMEOFF;
    }
    if int(s.pos.x) != int(first.pos.x) {
        flags |= SHF_XPOSOFF;
    }
    if int(s.pos.y) != int(first.pos.y) {
        flags |= SHF_YPOSOFF;
    }
    if s.angle != first.angle {
        flags |= SHF_ANGLEOFF;
    }
    if s.speed != first.speed {
        flags |= SHF_SPEEDOFF;
    }
    if int(s.pos.x) < int(first.pos.x) {
        flags |= SHF_NG_XPOSOFF;
        flags &= !SHF_XPOSOFF;
    }
    if int(s.pos.y) < int(first.pos.y) {
        flags |= SHF_NG_YPOSOFF;
        flags &= !SHF_YPOSOFF;
    }
    if s.angle < first.angle {
        extra |= SHF_NG_ANGLEOFF;
        flags &= !SHF_ANGLEOFF;
    }
    if s.speed < first.speed {
        extra |= SHF_NG_SPEEDOFF;
        flags &= !SHF_SPEEDOFF;
    }
    if xoff >= 32 {
        extra |= SHF_LARGEXOFF;
    }
    if yoff >= 32 {
        extra |= SHF_LARGEYOFF;
    }
    if int(s.worm_vel.x) != int(first.worm_vel.x) {
        flags |= SHF_XWRMVEL;
    }
    if int(s.worm_vel.y) != int(first.worm_vel.y) {
        flags |= SHF_YWRMVEL;
    }
    if int(s.worm_vel.x) < int(first.worm_vel.x) {
        extra &= !SHF_XWRMVEL;
        extra |= SHF_NG_XWRMVEL;
    }
    if int(s.worm_vel.y) < int(first.worm_vel.y) {
        extra &= !SHF_YWRMVEL;
        extra |= SHF_NG_YWRMVEL;
    }
    if extra != 0 {
        flags |= SHF_EXTRAFLAGS;
    }

    w.write_byte(flags);
    if flags & SHF_EXTRAFLAGS != 0 {
        w.write_byte(extra);
    }
    if flags & SHF_TIMEOFF != 0 {
        w.write_byte(s.time.0.wrapping_sub(first.time.0) as u8);
    }
    let x_off = flags & (SHF_XPOSOFF | SHF_NG_XPOSOFF) != 0;
    let y_off = flags & (SHF_YPOSOFF | SHF_NG_YPOSOFF) != 0;
    if x_off && extra & SHF_LARGEXOFF != 0 {
        w.write_byte(xoff as u8);
    }
    if y_off && extra & SHF_LARGEYOFF != 0 {
        w.write_byte(yoff as u8);
    }
    if x_off && y_off && extra & SHF_LARGEXOFF == 0 && extra & SHF_LARGEYOFF == 0 {
        w.write_2int4(xoff, yoff);
    }
    if flags & SHF_ANGLEOFF != 0 {
        w.write_byte(s.angle.wrapping_sub(first.angle) as u8);
    }
    if flags & SHF_SPEEDOFF != 0 {
        w.write_byte(s.speed.wrapping_sub(first.speed) as u8);
    }
    if extra & SHF_NG_ANGLEOFF != 0 {
        w.write_byte(first.angle.wrapping_sub(s.angle) as u8);
    }
    if extra & SHF_NG_SPEEDOFF != 0 {
        w.write_byte(first.speed.wrapping_sub(s.speed) as u8);
    }
    if extra & SHF_XWRMVEL != 0 {
        w.write_byte(int(s.worm_vel.x).wrapping_sub(int(first.worm_vel.x)) as u8);
    }
    if extra & SHF_YWRMVEL != 0 {
        w.write_byte(int(s.worm_vel.y).wrapping_sub(int(first.worm_vel.y)) as u8);
    }
    if extra & SHF_NG_XWRMVEL != 0 {
        w.write_byte(int(first.worm_vel.x).wrapping_sub(int(s.worm_vel.x)) as u8);
    }
    if extra & SHF_NG_YWRMVEL != 0 {
        w.write_byte(int(first.worm_vel.y).wrapping_sub(int(s.worm_vel.y)) as u8);
    }
    w.write_release(s.release);
}

/// How many shots from `index` on go into one packet (`writeMulti` 216-238).
fn run_length(shots: &[Shot], index: usize) -> usize {
    let (wmid, wpid) = (shots[index].worm, shots[index].weapon);
    let mut num = 1;
    for i in index + 1..shots.len() {
        let (s, p) = (&shots[i], &shots[i - 1]);
        if s.worm != wmid
            || s.weapon != wpid
            || s.time.0.wrapping_sub(p.time.0) > 255
            || s.angle.wrapping_sub(p.angle).wrapping_abs() > 255
            || s.speed.wrapping_sub(p.speed).wrapping_abs() > 255
            || int(s.worm_vel.x).wrapping_sub(int(p.worm_vel.x)).wrapping_abs() > 255
            || int(s.worm_vel.y).wrapping_sub(int(p.worm_vel.y)).wrapping_abs() > 255
            || num >= 255
        {
            break;
        }
        num += 1;
    }
    num
}

/// `CShootList::writePacket` (`CShootList.cpp:119-141`) with `writeMulti`'s
/// recursion (`:201-306`) as a loop.
fn write_packet(w: &mut Wire, shots: &[Shot]) {
    let mut index = 0;
    if shots.len() == 1 {
        write_header(w, &shots[0], None);
        return;
    }
    while index < shots.len() {
        if index == shots.len() - 1 {
            write_header(w, &shots[index], None);
            return;
        }
        let num = run_length(shots, index);
        if num == 1 {
            write_header(w, &shots[index], None);
            index += 1;
            continue;
        }
        write_header(w, &shots[index], Some(num as u8));
        for i in 1..num {
            write_small(w, &shots[index + i - 1], &shots[index + i]);
        }
        index += num;
    }
}

/// The shots as the client decodes them, in list order.
pub struct RoundTrip {
    wire: Wire,
    max_weapon_id: i32,
    /// The previous decoded shot of the current multi-shot packet.
    prev: Shot,
    /// Small shots left in the current packet.
    left: usize,
}

/// Encodes `shots` as the server sends them and decodes them as the client
/// reads them. `max_weapon_id` is `NumWeapons - 1` (the reader clamps to it).
/// At most [`MAX_SHOOTINGS`] shots are encoded.
pub fn encode_decode(shots: &[Shot], max_weapon_id: i32) -> RoundTrip {
    let mut wire = Wire { data: [0; WIRE_CAP], len: 0, pos: 0 };
    if !shots.is_empty() {
        write_packet(&mut wire, &shots[..shots.len().min(MAX_SHOOTINGS)]);
    }
    RoundTrip { wire, max_weapon_id, prev: Shot::default(), left: 0 }
}

impl RoundTrip {
    /// The header `readSingle` (437-484) and `readMulti` (489-542) share.
    fn read_header(&mut self, multi: bool) -> Shot {
        let w = &mut self.wire;
        let flags = w.read_byte();
        let worm = w.read_byte() as i32;
        let t = w.read_f32();
        // MAX(readFloat(), 0.0f) assigned through TimeDiff(float): (Uint64)(f * 1000.0f).
        let t = if t > 0.0 { t } else { 0.0 };
        let time = TimeDiff((t * 1000.0) as u64);
        let weapon = (w.read_byte() as i32).clamp_olx(0, self.max_weapon_id);
        let num = if multi { w.read_byte().min((MAX_SHOOTINGS - 1) as u8) as usize } else { 1 };
        let (x, y) = w.read_2int12();
        let (vx, vy) = (w.read_i16(), w.read_i16());
        let random = w.read_byte() as i32;
        let mut angle = w.read_byte() as i32;
        let mut speed = w.read_byte() as i32;
        if flags & SMF_LARGEANGLE != 0 {
            angle += 255;
        }
        if flags & SMF_LARGESPEED != 0 {
            speed += 255;
        }
        if flags & SMF_NEGSPEED != 0 {
            speed = -speed;
        }
        let release = w.read_release();
        self.left = num.saturating_sub(1);
        Shot {
            worm,
            weapon,
            pos: Vec2::new(x as f32, y as f32),
            worm_vel: Vec2::new(vx as f32, vy as f32),
            angle,
            random,
            speed,
            time,
            release,
            ..Shot::default()
        }
    }

    /// `readSmallShot` (547-643): a copy of the previous decoded shot with
    /// the offsets applied; position offsets are read and dropped.
    fn read_small(&mut self) -> Shot {
        let w = &mut self.wire;
        let mut s = self.prev;
        let flags = w.read_byte();
        let extra = if flags & SHF_EXTRAFLAGS != 0 { w.read_byte() } else { 0 };
        if flags & SHF_TIMEOFF != 0 {
            let t = w.read_byte() as f32;
            s.time = TimeDiff(s.time.0.wrapping_add((t / 1000.0 * 1000.0) as u64));
        }
        let x_off = flags & (SHF_XPOSOFF | SHF_NG_XPOSOFF) != 0;
        let y_off = flags & (SHF_YPOSOFF | SHF_NG_YPOSOFF) != 0;
        if x_off && extra & SHF_LARGEXOFF != 0 {
            w.read_byte();
        }
        if y_off && extra & SHF_LARGEYOFF != 0 {
            w.read_byte();
        }
        if x_off && y_off && extra & SHF_LARGEXOFF == 0 && extra & SHF_LARGEYOFF == 0 {
            w.read_2int4();
        }
        if flags & SHF_ANGLEOFF != 0 {
            s.angle += w.read_byte() as i32;
        }
        if flags & SHF_SPEEDOFF != 0 {
            s.speed += w.read_byte() as i32;
        }
        if extra & SHF_NG_ANGLEOFF != 0 {
            s.angle -= w.read_byte() as i32;
        }
        if extra & SHF_NG_SPEEDOFF != 0 {
            s.speed -= w.read_byte() as i32;
        }
        if extra & SHF_XWRMVEL != 0 {
            s.worm_vel.x += w.read_byte() as f32;
        }
        if extra & SHF_YWRMVEL != 0 {
            s.worm_vel.y += w.read_byte() as f32;
        }
        if extra & SHF_NG_XWRMVEL != 0 {
            s.worm_vel.x -= w.read_byte() as f32;
        }
        if extra & SHF_NG_YWRMVEL != 0 {
            s.worm_vel.y -= w.read_byte() as f32;
        }
        s.release = w.read_release();
        self.left -= 1;
        s
    }
}

impl Iterator for RoundTrip {
    type Item = Shot;

    fn next(&mut self) -> Option<Shot> {
        let s = if self.left > 0 {
            self.read_small()
        } else {
            if self.wire.at_end() {
                return None;
            }
            match self.wire.read_byte() {
                S2C_SINGLESHOOT => self.read_header(false),
                S2C_MULTISHOOT => self.read_header(true),
                _ => return None,
            }
        };
        self.prev = s;
        Some(s)
    }
}

/// OLX's `CLAMP` (`include/MathLib.h:69-70`): the lower bound wins only if
/// the value is below it, so an empty range gives the upper bound.
trait ClampOlx {
    fn clamp_olx(self, lo: Self, hi: Self) -> Self;
}

impl ClampOlx for i32 {
    fn clamp_olx(self, lo: i32, hi: i32) -> i32 {
        if self < lo {
            lo
        } else if self > hi {
            hi
        } else {
            self
        }
    }
}
