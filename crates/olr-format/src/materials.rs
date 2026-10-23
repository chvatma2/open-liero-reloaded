//! Surface materials (spec `2026-10-22-materials-and-heat-design.md`,
//! issue #37): what a level's pixels are made of for the look (stone,
//! wood, metal, glass, …), presentation only and apart from the collision
//! kind (Rock, Dirt, Background), which stays the simulation's. Heat is
//! their first effect: explosions leave a material that takes heat hot,
//! glowing by its colour ramp, then cooling.
//!
//! A material is one RON file, `<id>.ron`: the game's built-in ones in
//! `content/materials/` (read-only, shipped beside the packs), the user's
//! in `materials/` of the game's data directory. A level holds copies of
//! the materials it uses in its look (`level.ron`'s `look.materials`), so
//! it plays wherever it goes. Every value is an integer: the heat values
//! are percent of the engine's reference material, stone (100), and
//! temperatures run on the engine's scale from 0 (cold) to 1000 (a fresh
//! crater's rim at its hottest).

use serde::{Deserialize, Serialize};

use crate::defaults as d;
use crate::ron_io::{from_canonical, to_canonical};
use crate::{Colour, FormatError};

pub use olr_look::{
    MAX_CAPACITY, MAX_CONDUCTIVITY, MAX_COOLING, MAX_GLOW, MAX_MATERIALS, MAX_RAMP_KEYS, MAX_SENSITIVITY,
    MAX_SOAK, MAX_TEMPERATURE, MIN_CAPACITY, MIN_COOLING,
};

/// The longest material name, in characters.
pub const MAX_MATERIAL_NAME: usize = 64;
/// The longest material id, in bytes.
pub const MAX_MATERIAL_ID: usize = 64;

fn hundred_u8() -> u8 {
    100
}

fn is_hundred_u8(v: &u8) -> bool {
    *v == 100
}

/// A surface material: a file `<id>.ron` of a material library, or one of
/// a level's `look.materials`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialSource {
    /// 1 to 64 bytes of `[a-z0-9_-]`, starting with a letter or digit;
    /// unique in a level, and a library file's name (`<id>.ron`). A level's
    /// elements name it.
    pub id: String,
    /// Shown in the editors, 1 to 64 characters.
    pub name: String,
    /// Free text for authors; ignored by the game.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// How deep light soaks into its rock and dirt from the air beside
    /// them, world pixels, 0 to 12 (issue #57): thin for concrete and stone,
    /// deeper for furniture. Default none: the level's `look.light_soak`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_soak: Option<u8>,
    /// How it takes heat, spreads it, loses it and shows it. Default none:
    /// it never heats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heat: Option<HeatSource>,
}

/// A material's heat. Explosions deposit heat; it spreads between
/// neighbouring pixels, soaks in and cools to the air, by these values.
/// Each is percent of the engine's reference material, stone (100).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeatSource {
    /// How much of the deposited heat a pixel takes, percent, 0 to 100 (0:
    /// it never heats, as without `heat`). Default 100.
    #[serde(default = "hundred_u8", skip_serializing_if = "is_hundred_u8")]
    pub sensitivity: u8,
    /// How much heat it takes to warm a pixel, percent of stone's, 10 to
    /// 1000: higher warms less from the same blast and holds its heat
    /// longer.
    pub capacity: u16,
    /// How fast heat spreads to neighbouring pixels of materials that
    /// conduct, and so how deep it soaks in, percent of stone's, 0 (none)
    /// to 1000.
    pub conductivity: u16,
    /// How fast a pixel beside air loses its heat to it, percent of
    /// stone's, 1 to 1000.
    pub cooling: u16,
    /// How it looks hot. Default: the engine's ramp, every glow cast as
    /// light.
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub look: HeatLookSource,
}

/// How a hot material looks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeatLookSource {
    /// Temperature to colour and glow: 1 to 16 keys, their temperatures
    /// rising. Below the first key a pixel shows its art; between keys the
    /// colour and glow are linear; above the last, the last. Default none:
    /// the engine's lava ramp (a faint dull red from 100, orange from
    /// about 450, yellow-white at 1000).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ramp: Vec<RampKeySource>,
    /// Percent of its glow cast as light into the level, 0 (it shows hot
    /// but lights nothing) to 100. Default 100.
    #[serde(default = "hundred_u8", skip_serializing_if = "is_hundred_u8")]
    pub light: u8,
}

impl Default for HeatLookSource {
    fn default() -> HeatLookSource {
        HeatLookSource { ramp: Vec::new(), light: 100 }
    }
}

/// One key of a colour ramp.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RampKeySource {
    /// The engine's temperature scale, 1 to 1000.
    pub temperature: u16,
    /// What a pixel this hot shows over its art; opaque.
    pub colour: Colour,
    /// How strongly the colour shows over the art and glows whatever the
    /// darkness, percent, 0 to 100.
    pub glow: u8,
}

/// Whether `id` is a material's id: 1 to [`MAX_MATERIAL_ID`] bytes of
/// `[a-z0-9_-]`, starting with a letter or digit, and `<id>.ron` a file
/// name portable everywhere.
pub fn valid_material_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_MATERIAL_ID
        && !id.starts_with(['-', '_'])
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
        && olr_vfs::non_portable(&material_file(id)).is_none()
}

/// A library's file of material `id`: `<id>.ron`.
pub fn material_file(id: &str) -> String {
    format!("{id}.ron")
}

fn at(path: &str, field: &str) -> String {
    if path.is_empty() { field.to_string() } else { format!("{path}.{field}") }
}

fn within(v: u32, lo: u32, hi: u32, file: &str, path: &str) -> Result<(), FormatError> {
    if (lo..=hi).contains(&v) {
        Ok(())
    } else {
        Err(FormatError::invalid(file, path, format!("{v}, not within {lo} to {hi}")))
    }
}

/// Material `m` checked, as `olr-look`'s runtime material; errors name
/// `file` and the field under `path` (`""` for a library file,
/// `look.materials[i]` in a level).
pub fn compile_material(
    m: &MaterialSource,
    file: &str,
    path: &str,
) -> Result<olr_look::SurfaceMaterial, FormatError> {
    if !valid_material_id(&m.id) {
        return Err(FormatError::invalid(
            file,
            &at(path, "id"),
            format!(
                "{:?} is not 1 to {MAX_MATERIAL_ID} bytes of [a-z0-9_-] starting with a letter or digit",
                m.id
            ),
        ));
    }
    let chars = m.name.chars().count();
    if !(1..=MAX_MATERIAL_NAME).contains(&chars) {
        return Err(FormatError::invalid(
            file,
            &at(path, "name"),
            format!("{chars} characters, not 1 to {MAX_MATERIAL_NAME}"),
        ));
    }
    if let Some(soak) = m.light_soak {
        within(soak.into(), 0, MAX_SOAK.into(), file, &at(path, "light_soak"))?;
    }
    let heat = m.heat.as_ref().map(|h| compile_heat(h, file, &at(path, "heat"))).transpose()?;
    Ok(olr_look::SurfaceMaterial { id: m.id.clone(), heat, light_soak: m.light_soak })
}

fn compile_heat(h: &HeatSource, file: &str, path: &str) -> Result<olr_look::Heat, FormatError> {
    within(h.sensitivity.into(), 0, MAX_SENSITIVITY.into(), file, &at(path, "sensitivity"))?;
    within(h.capacity.into(), MIN_CAPACITY.into(), MAX_CAPACITY.into(), file, &at(path, "capacity"))?;
    within(h.conductivity.into(), 0, MAX_CONDUCTIVITY.into(), file, &at(path, "conductivity"))?;
    within(h.cooling.into(), MIN_COOLING.into(), MAX_COOLING.into(), file, &at(path, "cooling"))?;
    let look = at(path, "look");
    within(h.look.light.into(), 0, MAX_GLOW.into(), file, &at(&look, "light"))?;
    let keys = &h.look.ramp;
    if keys.len() > MAX_RAMP_KEYS {
        return Err(FormatError::invalid(
            file,
            &at(&look, "ramp"),
            format!("{} keys, above {MAX_RAMP_KEYS}", keys.len()),
        ));
    }
    let mut ramp = Vec::with_capacity(keys.len());
    for (i, k) in keys.iter().enumerate() {
        let p = format!("{look}.ramp[{i}]");
        within(k.temperature.into(), 1, MAX_TEMPERATURE.into(), file, &at(&p, "temperature"))?;
        if i > 0 && k.temperature <= keys[i - 1].temperature {
            return Err(FormatError::invalid(
                file,
                &at(&p, "temperature"),
                format!("{}: not above the key before it ({})", k.temperature, keys[i - 1].temperature),
            ));
        }
        within(k.glow.into(), 0, MAX_GLOW.into(), file, &at(&p, "glow"))?;
        let c = k.colour.0;
        if c.a != 255 {
            return Err(FormatError::invalid(
                file,
                &at(&p, "colour"),
                format!("{} must be opaque", k.colour.to_hex()),
            ));
        }
        ramp.push(olr_look::RampKey { temperature: k.temperature, colour: [c.r, c.g, c.b], glow: k.glow });
    }
    Ok(olr_look::Heat {
        sensitivity: h.sensitivity,
        capacity: h.capacity,
        conductivity: h.conductivity,
        cooling: h.cooling,
        ramp,
        light: h.look.light,
    })
}

/// A level's materials checked (each one, at most [`MAX_MATERIALS`], ids
/// unique), compiled in order; errors name `file` and `path[i]`.
pub fn compile_materials(
    list: &[MaterialSource],
    file: &str,
    path: &str,
) -> Result<Vec<olr_look::SurfaceMaterial>, FormatError> {
    if list.len() > MAX_MATERIALS {
        return Err(FormatError::invalid(
            file,
            path,
            format!("{} materials, above {MAX_MATERIALS}", list.len()),
        ));
    }
    let mut out = Vec::with_capacity(list.len());
    for (i, m) in list.iter().enumerate() {
        let p = format!("{path}[{i}]");
        if list[..i].iter().any(|o| o.id == m.id) {
            return Err(FormatError::invalid(file, &at(&p, "id"), format!("{:?} is used twice", m.id)));
        }
        out.push(compile_material(m, file, &p)?);
    }
    Ok(out)
}

/// A material file's text (`file` names it in errors) read and checked:
/// the id must be the file's (`<id>.ron`) when `file` is one.
pub fn read_material(file: &str, text: &str) -> Result<MaterialSource, FormatError> {
    let m: MaterialSource = from_canonical(file, text)?;
    compile_material(&m, file, "")?;
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
    if let Some(stem) = name.strip_suffix(".ron")
        && stem != m.id
    {
        return Err(FormatError::invalid(file, "id", format!("{:?}, the file is {name}", m.id)));
    }
    Ok(m)
}

/// Material `m` checked and written as its file's canonical text.
pub fn material_text(m: &MaterialSource) -> Result<String, FormatError> {
    let file = material_file(&m.id);
    compile_material(m, &file, "")?;
    to_canonical(&file, m)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn stone() -> MaterialSource {
        MaterialSource {
            id: "stone".into(),
            name: "Stone".into(),
            notes: String::new(),
            light_soak: None,
            heat: Some(HeatSource {
                sensitivity: 100,
                capacity: 100,
                conductivity: 100,
                cooling: 100,
                look: HeatLookSource::default(),
            }),
        }
    }

    #[test]
    fn a_material_round_trips_canonically() {
        let mut m = stone();
        m.notes = "Grey rock.".into();
        let text = material_text(&m).unwrap();
        assert_eq!(
            text,
            "(\n    id: \"stone\",\n    name: \"Stone\",\n    notes: \"Grey rock.\",\n    heat: (\n        capacity: 100,\n        conductivity: 100,\n        cooling: 100,\n    ),\n)\n"
        );
        assert_eq!(read_material("stone.ron", &text).unwrap(), m);
        let mut glass = stone();
        glass.id = "glass".into();
        glass.heat.as_mut().unwrap().look = HeatLookSource {
            ramp: vec![RampKeySource {
                temperature: 300,
                colour: Colour::parse("#ff8000").unwrap(),
                glow: 60,
            }],
            light: 20,
        };
        let text = material_text(&glass).unwrap();
        assert_eq!(read_material("glass.ron", &text).unwrap(), glass);
        assert_eq!(material_text(&read_material("glass.ron", &text).unwrap()).unwrap(), text);
        let cold = MaterialSource { heat: None, ..stone() };
        assert_eq!(material_text(&cold).unwrap(), "(\n    id: \"stone\",\n    name: \"Stone\",\n)\n");
    }

    #[test]
    fn bounds_are_refused() {
        #[allow(clippy::type_complexity)]
        let cases: Vec<(&str, Box<dyn Fn(&mut MaterialSource)>)> = vec![
            ("id", Box::new(|m| m.id = "Stone".into())),
            ("id", Box::new(|m| m.id = "-stone".into())),
            ("id", Box::new(|m| m.id = "a.b".into())),
            ("id", Box::new(|m| m.id = "con".into())),
            ("id", Box::new(|m| m.id = "x".repeat(65))),
            ("name", Box::new(|m| m.name.clear())),
            ("name", Box::new(|m| m.name = "n".repeat(65))),
            ("light_soak", Box::new(|m| m.light_soak = Some(13))),
            ("heat.sensitivity", Box::new(|m| m.heat.as_mut().unwrap().sensitivity = 101)),
            ("heat.capacity", Box::new(|m| m.heat.as_mut().unwrap().capacity = 9)),
            ("heat.capacity", Box::new(|m| m.heat.as_mut().unwrap().capacity = 1001)),
            ("heat.conductivity", Box::new(|m| m.heat.as_mut().unwrap().conductivity = 1001)),
            ("heat.cooling", Box::new(|m| m.heat.as_mut().unwrap().cooling = 0)),
            ("heat.cooling", Box::new(|m| m.heat.as_mut().unwrap().cooling = 1001)),
            ("heat.look.light", Box::new(|m| m.heat.as_mut().unwrap().look.light = 101)),
            (
                "heat.look.ramp",
                Box::new(|m| {
                    let key =
                        RampKeySource { temperature: 1, colour: Colour::parse("#000000").unwrap(), glow: 0 };
                    m.heat.as_mut().unwrap().look.ramp =
                        (0..17).map(|i| RampKeySource { temperature: 1 + i, ..key }).collect();
                }),
            ),
            ("heat.look.ramp[0].temperature", Box::new(|m| set_ramp(m, &[(0, "#000000", 0)]))),
            ("heat.look.ramp[0].temperature", Box::new(|m| set_ramp(m, &[(1001, "#000000", 0)]))),
            (
                "heat.look.ramp[1].temperature",
                Box::new(|m| set_ramp(m, &[(5, "#000000", 0), (5, "#000000", 0)])),
            ),
            ("heat.look.ramp[0].glow", Box::new(|m| set_ramp(m, &[(5, "#000000", 101)]))),
            ("heat.look.ramp[0].colour", Box::new(|m| set_ramp(m, &[(5, "#00000080", 0)]))),
        ];
        for (field, edit) in cases {
            let mut m = stone();
            edit(&mut m);
            let e = compile_material(&m, "x.ron", "").unwrap_err().to_string();
            assert!(e.contains(field), "{field}: {e}");
            let e = compile_materials(&[m], "level.ron", "look.materials").unwrap_err().to_string();
            assert!(e.contains(&format!("look.materials[0].{field}")), "{field}: {e}");
        }
        // The limits themselves pass.
        let mut m = stone();
        let h = m.heat.as_mut().unwrap();
        (h.sensitivity, h.capacity, h.conductivity, h.cooling, h.look.light) = (0, 10, 0, 1, 0);
        compile_material(&m, "x.ron", "").unwrap();
        let h = m.heat.as_mut().unwrap();
        (h.sensitivity, h.capacity, h.conductivity, h.cooling, h.look.light) = (100, 1000, 1000, 1000, 100);
        set_ramp(&mut m, &[(1, "#000000", 0), (1000, "#ffffff", 100)]);
        compile_material(&m, "x.ron", "").unwrap();
    }

    /// Issue #57: a material's own light soak round-trips, 0 to 12; none
    /// is the level's.
    #[test]
    fn light_soak_round_trips() {
        let mut m = MaterialSource { heat: None, ..stone() };
        assert_eq!(compile_material(&m, "stone.ron", "").unwrap().light_soak, None);
        let text = material_text(&m).unwrap();
        assert!(!text.contains("light_soak"), "{text}");
        assert_eq!(read_material("stone.ron", &text).unwrap().light_soak, None, "absent is none");
        for soak in [0, 2, MAX_SOAK] {
            m.light_soak = Some(soak);
            let text = material_text(&m).unwrap();
            assert!(text.contains(&format!("light_soak: {soak},")), "{text}");
            assert_eq!(read_material("stone.ron", &text).unwrap(), m);
            assert_eq!(compile_material(&m, "stone.ron", "").unwrap().light_soak, Some(soak));
        }
        let e = read_material("stone.ron", "(id: \"stone\", name: \"Stone\", light_soak: 13)")
            .unwrap_err()
            .to_string();
        assert!(e.contains("light_soak") && e.contains("13"), "{e}");
    }

    fn set_ramp(m: &mut MaterialSource, keys: &[(u16, &str, u8)]) {
        m.heat.as_mut().unwrap().look.ramp = keys
            .iter()
            .map(|&(temperature, c, glow)| RampKeySource {
                temperature,
                colour: Colour::parse(c).unwrap(),
                glow,
            })
            .collect();
    }

    #[test]
    fn a_level_holds_at_most_255_unique_materials() {
        let mut other = stone();
        other.id = "brick".into();
        compile_materials(&[stone(), other], "level.ron", "look.materials").unwrap();
        let e =
            compile_materials(&[stone(), stone()], "level.ron", "look.materials").unwrap_err().to_string();
        assert!(e.contains("look.materials[1].id") && e.contains("twice"), "{e}");
        let many: Vec<MaterialSource> =
            (0..256).map(|i| MaterialSource { id: format!("m{i}"), ..stone() }).collect();
        let e = compile_materials(&many, "level.ron", "look.materials").unwrap_err().to_string();
        assert!(e.contains("256 materials, above 255"), "{e}");
        compile_materials(&many[..255], "level.ron", "look.materials").unwrap();
    }

    #[test]
    fn a_files_id_is_its_name_and_hostile_text_is_an_error() {
        let text = material_text(&stone()).unwrap();
        let e = read_material("rock.ron", &text).unwrap_err().to_string();
        assert!(e.contains("rock.ron") && e.contains("\"stone\""), "{e}");
        for bad in
            ["", "(", "(id: \"stone\")", "(id: \"stone\", name: \"S\", heat: (capacity: 70000))", "[[[[[[[["]
        {
            assert!(read_material("stone.ron", bad).is_err(), "{bad}");
        }
        let deep = "(".repeat(10_000);
        assert!(read_material("stone.ron", &deep).is_err());
    }
}
