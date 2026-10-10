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

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Num(Num),
    Int(i64),
    Bool(bool),
    Str(String),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Num(n) => write!(f, "{n}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Str(s) => write!(f, "{s}"),
        }
    }
}

/// Every comparable field of a frame, in a fixed order, with its trace path.
/// List lengths come before list items, so a missing projectile is reported
/// as `p.len`, not as a confusing field mismatch.
fn fields(fr: &Frame) -> Vec<(String, Value)> {
    use Value::*;
    let mut out = vec![
        ("f".to_string(), Int(fr.f)),
        ("w.len".into(), Int(fr.w.len() as i64)),
        ("p.len".into(), Int(fr.p.len() as i64)),
        ("b.len".into(), Int(fr.b.len() as i64)),
    ];
    for (i, w) in fr.w.iter().enumerate() {
        let p = |k: &str| format!("w[{i}].{k}");
        out.extend([
            (p("id"), Int(w.id.into())),
            (p("alive"), Bool(w.alive)),
            (p("x"), Num(w.x)),
            (p("y"), Num(w.y)),
            (p("vx"), Num(w.vx)),
            (p("vy"), Num(w.vy)),
            (p("hp"), Num(w.hp)),
            (p("ang"), Num(w.ang)),
            (p("angv"), Num(w.angv)),
            (p("face"), Int(w.face.into())),
            (p("move"), Int(w.move_dir.into())),
            (p("ground"), Bool(w.ground)),
            (p("keys"), Int(w.keys.into())),
            (p("wpn"), Int(w.wpn.into())),
            (p("slots.len"), Int(w.slots.len() as i64)),
        ]);
        for (j, s) in w.slots.iter().enumerate() {
            let q = |k: &str| format!("w[{i}].slots[{j}].{k}");
            out.extend([
                (q("id"), Int(s.id.into())),
                (q("ch"), Num(s.ch)),
                (q("rl"), Bool(s.rl)),
                (q("lf"), Num(s.lf)),
            ]);
        }
        let r = &w.rope;
        out.extend([
            (p("rope.rel"), Bool(r.rel)),
            (p("rope.sh"), Bool(r.sh)),
            (p("rope.at"), Bool(r.at)),
            (p("rope.pw"), Int(r.pw.into())),
            (p("rope.x"), Num(r.x)),
            (p("rope.y"), Num(r.y)),
            (p("rope.vx"), Num(r.vx)),
            (p("rope.vy"), Num(r.vy)),
            (p("lives"), Int(w.lives.into())),
            (p("kills"), Int(w.kills.into())),
            (p("deaths"), Int(w.deaths.into())),
            (p("dirt"), Int(w.dirt.into())),
        ]);
    }
    for (i, pr) in fr.p.iter().enumerate() {
        let p = |k: &str| format!("p[{i}].{k}");
        out.extend([
            (p("i"), Int(pr.i.into())),
            (p("t"), Int(pr.t.into())),
            (p("x"), Num(pr.x)),
            (p("y"), Num(pr.y)),
            (p("vx"), Num(pr.vx)),
            (p("vy"), Num(pr.vy)),
            (p("life"), Num(pr.life)),
        ]);
    }
    for (i, b) in fr.b.iter().enumerate() {
        let p = |k: &str| format!("b[{i}].{k}");
        out.extend([
            (p("i"), Int(b.i.into())),
            (p("t"), Int(b.t.into())),
            (p("x"), Num(b.x)),
            (p("y"), Num(b.y)),
        ]);
    }
    out.push(("mh".into(), Str(fr.mh.clone())));
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

/// Compares `actual` against `expected` frame by frame and returns the number
/// of frames compared, or the first divergence. Header lines are not compared.
pub fn compare(expected: &Trace, actual: &Trace, tol: &[Tolerance]) -> Result<usize, Divergence> {
    for (e, a) in expected.frames.iter().zip(&actual.frames) {
        let (ef, af) = (fields(e), fields(a));
        let mut diffs = ef.iter().zip(&af).filter(|((p, ev), (_, av))| !equal(p, ev, av, tol));
        if let Some(((path, ev), (_, av))) = diffs.next() {
            return Err(Divergence {
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
        return Err(Divergence {
            frame: n as i64 - 1,
            path: "frames".into(),
            expected: expected.frames.len().to_string(),
            actual: actual.frames.len().to_string(),
            more: 0,
        });
    }
    Ok(expected.frames.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjRec;
    use crate::trace::tests::OLX;

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

    #[test]
    fn wildcard_paths() {
        assert!(path_matches("w[*].rope.x", "w[12].rope.x"));
        assert!(path_matches("p[*].x", "p[0].x"));
        assert!(!path_matches("p[*].x", "p[].x"));
        assert!(!path_matches("p[*].x", "p[a].x"));
        assert!(!path_matches("w[*].x", "w[0].vx"));
    }
}
