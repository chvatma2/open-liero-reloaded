//! First-divergence comparison of two traces (spec §6): the report names only
//! the first frame and field that differ, so failures stay short.

use std::fmt;

use crate::{Frame, Num, Tolerance, Trace};

/// The first difference between an expected (OLX) and an actual (Rust) trace.
#[derive(Clone, Debug, PartialEq)]
pub struct Divergence {
    pub frame: i64,
    /// Trace path such as `w[0].vy`, `p.len` or `frames`.
    pub path: String,
    pub expected: String,
    pub actual: String,
    /// How many other fields differ in the same frame.
    pub more: usize,
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "frame {}: {} expected {} got {}", self.frame, self.path, self.expected, self.actual)?;
        if self.more > 0 {
            write!(f, " ({} more fields differ in this frame)", self.more)?;
        }
        Ok(())
    }
}

/// Groups of trace fields, so a comparison can check only what the
/// simulation reproduces so far. The frame number, the worm count and worm
/// ids are always compared.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fields(pub u32);

impl Fields {
    pub const NONE: Fields = Fields(0);
    /// Worm `x`, `y`, `vx`, `vy`, `ang`, `angv`, `face`, `move`, `ground`, `keys`.
    pub const WORM_MOTION: Fields = Fields(1 << 0);
    /// `wpn` and `slots`.
    pub const SLOTS: Fields = Fields(1 << 1);
    /// `rope`.
    pub const ROPE: Fields = Fields(1 << 2);
    /// `mh` and the worms' `dirt`.
    pub const MATERIAL: Fields = Fields(1 << 3);
    /// `rc`, `mt` and `ents`.
    pub const RAND: Fields = Fields(1 << 4);
    /// `s`, the delivered shots.
    pub const SHOTS: Fields = Fields(1 << 5);
    /// `p`: slot, type, position, velocity, life.
    pub const PROJECTILES: Fields = Fields(1 << 6);
    /// The projectiles' hidden state: `own`, `rnd`, `ls`, `sp`, `ig`, `ws`,
    /// `tr`, `ox`, `oy`, `fr`, `col`.
    pub const PROJ_HIDDEN: Fields = Fields(1 << 7);
    /// Worm `hp` and `alive`.
    pub const HEALTH: Fields = Fields(1 << 8);
    /// Worm `lives`, `kills`, `deaths`.
    pub const SCORE: Fields = Fields(1 << 9);
    /// Worm `suicides`, `tod`, `respawn`, `blood`.
    pub const RESPAWN: Fields = Fields(1 << 10);
    /// `b` and `sb`, the client's and the server's bonuses.
    pub const BONUSES: Fields = Fields(1 << 11);
    /// `sf`, `st`, `ct`.
    pub const CLOCKS: Fields = Fields(1 << 12);
    pub const ALL: Fields = Fields((1 << 13) - 1);

    /// `self | other`, usable in constants.
    pub const fn union(self, other: Fields) -> Fields {
        Fields(self.0 | other.0)
    }

    /// `self` without the groups in `other`, usable in constants.
    pub const fn difference(self, other: Fields) -> Fields {
        Fields(self.0 & !other.0)
    }

    /// Whether every group in `other` is in `self`.
    pub const fn contains(self, other: Fields) -> bool {
        self.0 & other.0 == other.0
    }

    fn any(self, other: Fields) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for Fields {
    type Output = Fields;
    fn bitor(self, other: Fields) -> Fields {
        self.union(other)
    }
}

impl std::ops::BitOrAssign for Fields {
    fn bitor_assign(&mut self, other: Fields) {
        *self = self.union(other);
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Num(Num),
    Int(i64),
    Bool(bool),
    Str(String),
    Null,
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Num(n) => write!(f, "{n}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Str(s) => write!(f, "{s}"),
            Value::Null => write!(f, "null"),
        }
    }
}

fn opt_int<T: Into<i64>>(v: Option<T>) -> Value {
    v.map_or(Value::Null, |v| Value::Int(v.into()))
}

/// A `u64` counter as a value (counters never reach `i64::MAX`).
fn opt_count(v: Option<u64>) -> Value {
    v.map_or(Value::Null, |v| Value::Int(v as i64))
}

fn opt_num(v: Option<Num>) -> Value {
    v.map_or(Value::Null, Value::Num)
}

/// The checked fields of a frame, in a fixed order, with their trace paths.
/// A list's items are listed only if its length is, and lengths come before
/// items, so a missing projectile is reported as `p.len`, not as a confusing
/// field mismatch, and an unchecked list never shifts the fields after it.
fn fields(fr: &Frame, mask: Fields) -> Vec<(String, Value)> {
    use Value::*;
    let mut out: Vec<(String, Value)> = Vec::new();
    let mut add = |group: Fields, path: String, v: Value| {
        if group == Fields::NONE || mask.any(group) {
            out.push((path, v));
        }
    };
    let always = Fields::NONE;
    let (motion, slots, rope, material) =
        (Fields::WORM_MOTION, Fields::SLOTS, Fields::ROPE, Fields::MATERIAL);
    let (health, score, respawn) = (Fields::HEALTH, Fields::SCORE, Fields::RESPAWN);
    let (shots, bonuses, hidden) = (Fields::SHOTS, Fields::BONUSES, Fields::PROJ_HIDDEN);
    let projs = Fields::PROJECTILES | hidden;

    add(always, "f".into(), Int(fr.f));
    add(Fields::CLOCKS, "sf".into(), opt_int(fr.sf));
    add(Fields::CLOCKS, "st".into(), opt_int(fr.st));
    add(Fields::CLOCKS, "ct".into(), opt_int(fr.ct));
    add(Fields::RAND, "rc".into(), opt_count(fr.rc));
    add(Fields::RAND, "mt".into(), opt_count(fr.mt));
    add(Fields::RAND, "ents".into(), opt_int(fr.ents));
    add(always, "w.len".into(), Int(fr.w.len() as i64));
    add(shots, "s.len".into(), Int(fr.s.len() as i64));
    add(projs, "p.len".into(), Int(fr.p.len() as i64));
    add(bonuses, "b.len".into(), Int(fr.b.len() as i64));
    add(bonuses, "sb.len".into(), Int(fr.sb.len() as i64));
    for (i, s) in fr.s.iter().enumerate() {
        let p = |k: &str| format!("s[{i}].{k}");
        add(shots, p("w"), Int(s.w.into()));
        add(shots, p("wpn"), Int(s.wpn.into()));
        add(shots, p("x"), Num(s.x));
        add(shots, p("y"), Num(s.y));
        add(shots, p("vx"), Num(s.vx));
        add(shots, p("vy"), Num(s.vy));
        add(shots, p("ang"), Int(s.ang.into()));
        add(shots, p("rnd"), Int(s.rnd.into()));
        add(shots, p("spd"), Int(s.spd.into()));
        add(shots, p("rel"), Bool(s.rel));
        add(shots, p("t"), Int(s.t));
        add(shots, p("spawn"), Int(s.spawn));
    }
    for (i, w) in fr.w.iter().enumerate() {
        let p = |k: &str| format!("w[{i}].{k}");
        add(always, p("id"), Int(w.id.into()));
        add(health, p("alive"), Bool(w.alive));
        add(motion, p("x"), Num(w.x));
        add(motion, p("y"), Num(w.y));
        add(motion, p("vx"), Num(w.vx));
        add(motion, p("vy"), Num(w.vy));
        add(health, p("hp"), Num(w.hp));
        add(motion, p("ang"), Num(w.ang));
        add(motion, p("angv"), Num(w.angv));
        add(motion, p("face"), Int(w.face.into()));
        add(motion, p("move"), Int(w.move_dir.into()));
        add(motion, p("ground"), Bool(w.ground));
        add(motion, p("keys"), Int(w.keys.into()));
        add(slots, p("wpn"), Int(w.wpn.into()));
        add(slots, p("slots.len"), Int(w.slots.len() as i64));
        for (j, s) in w.slots.iter().enumerate() {
            let q = |k: &str| format!("w[{i}].slots[{j}].{k}");
            add(slots, q("id"), Int(s.id.into()));
            add(slots, q("ch"), Num(s.ch));
            add(slots, q("rl"), Bool(s.rl));
            add(slots, q("lf"), Num(s.lf));
        }
        let r = &w.rope;
        add(rope, p("rope.rel"), Bool(r.rel));
        add(rope, p("rope.sh"), Bool(r.sh));
        add(rope, p("rope.at"), Bool(r.at));
        add(rope, p("rope.pw"), Int(r.pw.into()));
        add(rope, p("rope.x"), Num(r.x));
        add(rope, p("rope.y"), Num(r.y));
        add(rope, p("rope.vx"), Num(r.vx));
        add(rope, p("rope.vy"), Num(r.vy));
        add(score, p("lives"), Int(w.lives.into()));
        add(score, p("kills"), Int(w.kills.into()));
        add(score, p("deaths"), Int(w.deaths.into()));
        add(material, p("dirt"), Int(w.dirt.into()));
        add(respawn, p("suicides"), opt_int(w.suicides));
        add(respawn, p("tod"), opt_int(w.tod));
        add(respawn, p("respawn"), opt_int(w.respawn));
        add(respawn, p("blood"), opt_int(w.blood));
    }
    let shown = Fields::PROJECTILES;
    for (i, pr) in fr.p.iter().enumerate() {
        let p = |k: &str| format!("p[{i}].{k}");
        add(projs, p("i"), Int(pr.i.into()));
        add(shown, p("t"), Int(pr.t.into()));
        add(shown, p("x"), Num(pr.x));
        add(shown, p("y"), Num(pr.y));
        add(shown, p("vx"), Num(pr.vx));
        add(shown, p("vy"), Num(pr.vy));
        add(shown, p("life"), Num(pr.life));
        add(hidden, p("own"), opt_int(pr.own));
        add(hidden, p("rnd"), opt_int(pr.rnd));
        add(hidden, p("ls"), opt_int(pr.ls));
        add(hidden, p("sp"), opt_int(pr.sp));
        add(hidden, p("ig"), opt_int(pr.ig));
        add(hidden, p("ws"), opt_num(pr.ws));
        add(hidden, p("tr"), opt_int(pr.tr));
        add(hidden, p("ox"), opt_num(pr.ox));
        add(hidden, p("oy"), opt_num(pr.oy));
        add(hidden, p("fr"), opt_num(pr.fr));
        add(hidden, p("col"), opt_int(pr.col));
    }
    for (i, b) in fr.b.iter().enumerate() {
        let p = |k: &str| format!("b[{i}].{k}");
        add(bonuses, p("i"), Int(b.i.into()));
        add(bonuses, p("t"), Int(b.t.into()));
        add(bonuses, p("x"), Num(b.x));
        add(bonuses, p("y"), Num(b.y));
        add(bonuses, p("vx"), opt_num(b.vx));
        add(bonuses, p("vy"), opt_num(b.vy));
        add(bonuses, p("life"), opt_num(b.life));
        add(bonuses, p("wpn"), opt_int(b.wpn));
    }
    for (i, b) in fr.sb.iter().enumerate() {
        let p = |k: &str| format!("sb[{i}].{k}");
        add(bonuses, p("i"), Int(b.i.into()));
        add(bonuses, p("t"), Int(b.t.into()));
        add(bonuses, p("x"), Num(b.x));
        add(bonuses, p("y"), Num(b.y));
        add(bonuses, p("wpn"), Int(b.wpn.into()));
        add(bonuses, p("spawn"), Int(b.spawn));
    }
    add(material, "mh".into(), Str(fr.mh.clone()));
    out
}

/// Whether `pattern` (where `[*]` matches any index) matches `path`.
fn path_matches(pattern: &str, path: &str) -> bool {
    let (mut p, mut s) = (pattern, path);
    while let Some(i) = p.find("[*]") {
        let Some(rest) = s.strip_prefix(&p[..i + 1]) else { return false };
        let Some(close) = rest.find(']') else { return false };
        if close == 0 || !rest[..close].bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        p = &p[i + 3..];
        s = &rest[close + 1..];
    }
    p == s
}

fn equal(path: &str, e: &Value, a: &Value, tol: &[Tolerance]) -> bool {
    if e == a {
        return true;
    }
    match (e, a) {
        (Value::Num(e), Value::Num(a)) => {
            tol.iter().any(|t| path_matches(&t.field, path) && (e.0 - a.0).abs() <= t.max_abs)
        }
        _ => false,
    }
}

/// Compares the `fields` groups of `actual` against `expected`, frame by
/// frame, and returns the first divergence. Header lines are not compared.
pub fn compare_fields(
    expected: &Trace,
    actual: &Trace,
    tol: &[Tolerance],
    fields: Fields,
) -> Option<Divergence> {
    for (e, a) in expected.frames.iter().zip(&actual.frames) {
        let (ef, af) = (self::fields(e, fields), self::fields(a, fields));
        let mut diffs = ef.iter().zip(&af).filter(|((p, ev), (_, av))| !equal(p, ev, av, tol));
        if let Some(((path, ev), (_, av))) = diffs.next() {
            return Some(Divergence {
                frame: e.f,
                path: path.clone(),
                expected: ev.to_string(),
                actual: av.to_string(),
                more: diffs.count(),
            });
        }
    }
    if expected.frames.len() != actual.frames.len() {
        let n = expected.frames.len().min(actual.frames.len());
        return Some(Divergence {
            frame: n as i64 - 1,
            path: "frames".into(),
            expected: expected.frames.len().to_string(),
            actual: actual.frames.len().to_string(),
            more: 0,
        });
    }
    None
}

/// Compares every field ([`Fields::ALL`]) and returns the number of frames
/// compared, or the first divergence.
pub fn compare(expected: &Trace, actual: &Trace, tol: &[Tolerance]) -> Result<usize, Divergence> {
    match compare_fields(expected, actual, tol, Fields::ALL) {
        Some(d) => Err(d),
        None => Ok(expected.frames.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjRec;
    use crate::trace::tests::{OLX, OLX_V2};

    fn olx() -> Trace {
        Trace::from_jsonl(OLX).unwrap()
    }

    #[test]
    fn identical_traces_match() {
        assert_eq!(compare(&olx(), &olx(), &[]), Ok(2));
    }

    #[test]
    fn reports_first_frame_and_field_only() {
        let mut a = olx();
        a.frames[1].w[0].vy = Num(31.249998);
        a.frames[1].w[0].x = Num(457.0);
        a.frames[1].mh = "0".into();
        let d = compare(&olx(), &a, &[]).unwrap_err();
        assert_eq!(
            d.to_string(),
            "frame 0: w[0].x expected 456 got 457 (2 more fields differ in this frame)"
        );
    }

    #[test]
    fn negative_zero_differs_from_zero() {
        let mut a = olx();
        a.frames[1].w[0].ang = Num(0.0);
        assert_eq!(compare(&olx(), &a, &[]).unwrap_err().path, "w[0].ang");
    }

    #[test]
    fn missing_projectile_is_a_length_divergence() {
        let mut a = olx();
        a.frames[1].p.clear();
        let d = compare(&olx(), &a, &[]).unwrap_err();
        assert_eq!((d.path.as_str(), d.expected.as_str(), d.actual.as_str()), ("p.len", "1", "0"));
        let mut b = olx();
        let extra = ProjRec { i: 4, ..b.frames[1].p[0].clone() };
        b.frames[1].p.push(extra);
        assert_eq!(compare(&olx(), &b, &[]).unwrap_err().path, "p.len");
    }

    #[test]
    fn missing_frames_are_reported() {
        let mut a = olx();
        a.frames.pop();
        let d = compare(&olx(), &a, &[]).unwrap_err();
        assert_eq!((d.path.as_str(), d.expected.as_str(), d.actual.as_str()), ("frames", "2", "1"));
    }

    #[test]
    fn tolerances_apply_to_matching_numeric_fields_only() {
        let mut a = olx();
        let vy = a.frames[1].w[0].vy.0;
        a.frames[1].w[0].vy = Num(vy + 0.0005);
        let tol = |field: &str| Tolerance { field: field.into(), max_abs: 0.001, reason: "test".into() };
        assert_eq!(compare(&olx(), &a, &[tol("w[*].vy")]), Ok(2));
        assert_eq!(compare(&olx(), &a, &[tol("w[0].vy")]), Ok(2));
        assert!(compare(&olx(), &a, &[tol("w[1].vy")]).is_err());
        assert!(compare(&olx(), &a, &[tol("w[*].vx")]).is_err());
    }

    fn olx_v2() -> Trace {
        Trace::from_jsonl(OLX_V2).unwrap()
    }

    #[test]
    fn fields_combine() {
        let f = Fields::SLOTS | Fields::ROPE;
        assert!(f.contains(Fields::ROPE) && f.contains(Fields::SLOTS) && !f.contains(Fields::HEALTH));
        assert_eq!(Fields::SLOTS.union(Fields::ROPE), f);
        assert_eq!(f.difference(Fields::ROPE), Fields::SLOTS);
        assert_eq!(f.difference(Fields::HEALTH), f);
        assert_eq!(Fields::ALL.difference(Fields::ALL), Fields::NONE);
        let mut g = Fields::NONE;
        g |= Fields::RAND;
        assert_eq!(g, Fields::RAND);
        for one in [
            Fields::WORM_MOTION,
            Fields::SLOTS,
            Fields::ROPE,
            Fields::MATERIAL,
            Fields::RAND,
            Fields::SHOTS,
            Fields::PROJECTILES,
            Fields::PROJ_HIDDEN,
            Fields::HEALTH,
            Fields::SCORE,
            Fields::RESPAWN,
            Fields::BONUSES,
            Fields::CLOCKS,
        ] {
            assert!(Fields::ALL.contains(one) && !Fields::NONE.contains(one));
        }
    }

    #[test]
    fn compare_fields_ignores_unchecked_fields() {
        let mut a = olx_v2();
        let f = &mut a.frames[1];
        f.sf = None;
        f.rc = Some(0);
        f.s.clear();
        f.p.clear(); // a length difference in an unchecked list must not shift later fields
        f.w[0].tod = None;
        f.w[0].slots[0].ch = Num(1.0);
        f.sb[0].spawn = 99;
        let checked = Fields::WORM_MOTION | Fields::ROPE | Fields::MATERIAL | Fields::HEALTH | Fields::SCORE;
        assert_eq!(compare_fields(&olx_v2(), &a, &[], checked), None);
        let d = compare_fields(&olx_v2(), &a, &[], Fields::ALL).unwrap();
        assert_eq!((d.frame, d.path.as_str()), (0, "sf"));
        assert_eq!(compare(&olx_v2(), &a, &[]).unwrap_err(), d, "compare means Fields::ALL");
    }

    #[test]
    fn compare_fields_reports_first_checked_divergence() {
        let mut a = olx_v2();
        let f = &mut a.frames[1];
        f.rc = Some(18);
        f.w[0].hp = Num(50.0);
        f.p[0].ls = Some(22);
        f.sb[0].wpn = 3;
        let e = olx_v2();
        let first = |fields| compare_fields(&e, &a, &[], fields).map(|d| (d.path, d.more));
        assert_eq!(first(Fields::HEALTH | Fields::PROJ_HIDDEN), Some(("w[0].hp".into(), 1)));
        assert_eq!(first(Fields::PROJ_HIDDEN), Some(("p[0].ls".into(), 0)));
        assert_eq!(first(Fields::RAND | Fields::BONUSES), Some(("rc".into(), 1)));
        assert_eq!(first(Fields::BONUSES), Some(("sb[0].wpn".into(), 0)));
        assert_eq!(first(Fields::SHOTS | Fields::SLOTS), None);
        let mut short = olx_v2();
        short.frames.pop();
        let d = compare_fields(&e, &short, &[], Fields::NONE).unwrap();
        assert_eq!((d.path.as_str(), d.expected.as_str(), d.actual.as_str()), ("frames", "2", "1"));
    }

    #[test]
    fn wildcard_paths() {
        assert!(path_matches("w[*].rope.x", "w[12].rope.x"));
        assert!(path_matches("p[*].x", "p[0].x"));
        assert!(!path_matches("p[*].x", "p[].x"));
        assert!(!path_matches("p[*].x", "p[a].x"));
        assert!(!path_matches("w[*].x", "w[0].vx"));
    }
}
