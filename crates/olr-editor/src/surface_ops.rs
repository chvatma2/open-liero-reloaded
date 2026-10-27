//! The level's surface materials in the editor (issue #43; spec
//! `2026-10-22-materials-and-heat-design.md` §Editors): the level's
//! materials (added from the library as copies, removed when unused), an
//! element's surfaces, and the Surface brush, which paints the selected
//! element's map at world resolution. Each checks first, then goes
//! through `Document::apply`, as every operation does.

use std::sync::Arc;

use olr_edit::compose::Rect;
use olr_edit::level::{Shape, shape_bounds};
use olr_edit::look::names_material;
use olr_format::levels::{
    Element, ElementKind, element_box, surface_back_file, surface_file, surface_map_size,
};
use olr_format::materials::{MAX_MATERIALS, MaterialSource};

use crate::document::{DocError, Document};
use crate::history::Edit;
use crate::ops::NOTHING;

/// An element's box: its world origin and size.
type ElementBox = ((i32, i32), (u32, u32));

/// What a Surface brush stroke starts from: the element and its map (its
/// back art's for `back`).
#[derive(Clone, Debug)]
pub struct SurfaceStrokeStart {
    pub back: bool,
    pub element: Element,
    pub map: Option<Arc<Vec<u8>>>,
}

/// The painted map file of element `id`: its own pixels', or its back art's.
pub fn map_file(id: &str, back: bool) -> String {
    if back { surface_back_file(id) } else { surface_file(id) }
}

/// Whether element `e` paints map `back` (its flag).
fn painted(e: &Element, back: bool) -> bool {
    if back { e.back_surface_map } else { e.surface_map }
}

impl Document {
    /// The level's surface materials (its look's), in order: the value of
    /// `materials()[i]` in a map is `i + 1`.
    pub fn materials(&self) -> &[MaterialSource] {
        self.level.look.as_ref().map_or(&[], |l| &l.materials[..])
    }

    /// The map value of material `id` (1 + its index), when the level has it.
    pub fn material_value(&self, id: &str) -> Option<u8> {
        self.materials().iter().position(|m| m.id == id).and_then(|i| u8::try_from(i + 1).ok())
    }

    /// Element `e`'s painted map `back` as it bakes: flagged, and of its size.
    pub fn surface_map(&self, e: &Element, back: bool) -> Option<&Arc<Vec<u8>>> {
        let (w, h) = surface_map_size(&e.kind, self.level.size);
        self.surface_maps
            .get(&map_file(&e.id, back))
            .filter(|m| painted(e, back) && m.len() as u64 == u64::from(w) * u64::from(h))
    }

    /// The elements using material `id` (naming it as a surface, or
    /// painting its value), by name, bottom to top: a material in use is
    /// not removed (`olr_edit::look::material_users`'s rule).
    pub fn material_users(&self, id: &str) -> Vec<String> {
        let value = self.material_value(id);
        self.items
            .iter()
            .map(|i| &i.element)
            .filter(|e| {
                names_material(e, id)
                    || [false, true].into_iter().any(|back| {
                        value.is_some_and(|v| self.surface_map(e, back).is_some_and(|m| m.contains(&v)))
                    })
            })
            .map(|e| if e.name.is_empty() { e.id.clone() } else { e.name.clone() })
            .collect()
    }

    /// Adds a copy of `material` (a library's) to the level's materials;
    /// a level without a look gets the default one (it plays lit). Its id
    /// must be new: the level keeps its own copy. One step.
    pub fn add_material(&mut self, material: MaterialSource) -> Result<(Vec<Edit>, Rect), DocError> {
        if self.material_value(&material.id).is_some() {
            return Err(DocError::Bad(format!("the level has the material {:?} already", material.name)));
        }
        if self.materials().len() >= MAX_MATERIALS {
            return Err(DocError::Bad(format!("a level holds at most {MAX_MATERIALS} materials")));
        }
        let mut look = self.level.look.clone().unwrap_or_default();
        look.materials.push(material);
        self.set_look(Some(look))
    }

    /// Removes material `id`, refused while an element uses it (named).
    /// The materials after it move down one, the painted maps with them.
    pub fn remove_material(&mut self, id: &str) -> Result<(Vec<Edit>, Rect), DocError> {
        if self.material_value(id).is_none() {
            return Err(DocError::Bad(format!("no material {id:?}")));
        }
        let users = self.material_users(id);
        if !users.is_empty() {
            let name = self.materials().iter().find(|m| m.id == id).map_or(id, |m| m.name.as_str());
            return Err(DocError::Bad(format!(
                "{name} is used by {}: change their surfaces first",
                users.join(", ")
            )));
        }
        let mut look = self.level.look.clone().unwrap_or_default();
        look.materials.retain(|m| m.id != id);
        self.set_look(Some(look))
    }

    /// The edits keeping the painted maps naming the same materials when
    /// the look's materials change from `before` to `after`: each map's
    /// values renumbered by id, a material gone 0 (the element's default).
    /// (`look_change` clears the surfaces naming a material gone.)
    pub(crate) fn surface_maps_change(
        &self,
        before: &[MaterialSource],
        after: &[MaterialSource],
    ) -> Vec<Edit> {
        let mut renumber = [0u8; 256];
        for (i, m) in before.iter().enumerate().take(255) {
            let to = after.iter().position(|x| x.id == m.id);
            renumber[i + 1] = to.and_then(|j| u8::try_from(j + 1).ok()).unwrap_or(0);
        }
        if (1..=before.len().min(255)).all(|v| usize::from(renumber[v]) == v) {
            return Vec::new();
        }
        let mut edits = Vec::new();
        for (file, map) in &self.surface_maps {
            let to: Vec<u8> = map.iter().map(|v| renumber[usize::from(*v)]).collect();
            if to == **map {
                continue;
            }
            let id = self
                .items
                .iter()
                .find(|i| [false, true].into_iter().any(|b| map_file(&i.element.id, b) == *file))
                .map_or_else(String::new, |i| i.element.id.clone());
            edits.push(Edit::SurfaceMap {
                id,
                file: file.clone(),
                before: Some(map.clone()),
                after: Some(Arc::new(to)),
            });
        }
        edits
    }

    /// What a Surface brush stroke into element `id`'s map (`back`: its
    /// back art's) starts from. Refused for a locked element, a distant
    /// one (it takes one surface for the whole layer) and a back map
    /// without back art.
    pub fn surface_stroke_before(&self, id: &str, back: bool) -> Result<SurfaceStrokeStart, DocError> {
        let index = self.unlocked(id)?;
        self.refuse_surface_in_state()?;
        let e = &self.items[index].element;
        if e.is_distant() {
            return Err(DocError::Bad(
                "a distant layer takes one surface for the whole layer: choose it in its properties".into(),
            ));
        }
        if back && !e.has_back_art() {
            return Err(DocError::WrongKind { id: id.into(), want: "an element with back art" });
        }
        Ok(SurfaceStrokeStart {
            back,
            element: e.clone(),
            map: self.surface_maps.get(&map_file(id, back)).cloned(),
        })
    }

    /// Element `id`'s map `back` ready to paint (no step: the stroke's
    /// records it): a map not flagged, or not of its size, starts at 0
    /// (the element's default). Returns its box (a raster's the level)
    /// and whether all of it changed.
    fn ready_surface(&mut self, id: &str, back: bool) -> Result<(ElementBox, bool), DocError> {
        self.surface_stroke_before(id, back)?;
        let index = self.find(id)?;
        let e = &self.items[index].element;
        let size = surface_map_size(&e.kind, self.level.size);
        let at = match &e.kind {
            ElementKind::Raster => (0, 0),
            kind => element_box(kind, self.level.size).map_or((0, 0), |(at, _)| at),
        };
        let n = size.0 as usize * size.1 as usize;
        let mut reset = false;
        if self.surface_map(&self.items[index].element, back).is_none() {
            self.surface_maps.insert(map_file(id, back), Arc::new(vec![0; n]));
            reset = true;
        }
        let e = &mut self.items[index].element;
        if back {
            e.back_surface_map = true;
        } else {
            e.surface_map = true;
        }
        Ok(((at, size), reset))
    }

    /// Paints `value` (a material's, 0 the element's default) into
    /// element `id`'s map `back` under world `shape`, within a stroke (no
    /// step: `surface_stroke_edits` records it). Returns the rectangle
    /// rebaked.
    pub fn paint_surface(
        &mut self,
        id: &str,
        back: bool,
        shape: &Shape,
        value: u8,
    ) -> Result<Rect, DocError> {
        if usize::from(value) > self.materials().len() {
            return Err(DocError::Bad(format!("no material {value}")));
        }
        let ((at, size), reset) = self.ready_surface(id, back)?;
        if let Some(map) = self.surface_maps.get_mut(&map_file(id, back)) {
            olr_edit::glow::paint_mask(Arc::make_mut(map).as_mut_slice(), at, size, 1, shape, value);
        }
        let index = self.find(id)?;
        let rect = if reset {
            self.element_rect(&self.items[index].element)
        } else {
            shape_bounds(shape, self.level.size).unwrap_or(NOTHING)
        };
        if rect.w > 0 && rect.h > 0 {
            self.rebake(rect);
        }
        Ok(rect)
    }

    /// The step a Surface brush stroke made, from what it started from:
    /// the map, then the element's flag. A map all 0 is no map (the
    /// element's default everywhere). Empty when what bakes did not
    /// change (anything the stroke set up is taken back).
    pub fn surface_stroke_edits(&mut self, id: &str, before: SurfaceStrokeStart) -> Vec<Edit> {
        let Some(index) = self.index_of(id) else { return Vec::new() };
        let file = map_file(id, before.back);
        let was = self.surface_map(&before.element, before.back).cloned();
        let mut now = self.surface_map(&self.items[index].element, before.back).cloned();
        if now.as_ref().is_some_and(|m| m.iter().all(|v| *v == 0)) {
            now = None;
        }
        if now == was {
            match before.map.clone() {
                Some(m) => self.surface_maps.insert(file, m),
                None => self.surface_maps.remove(&file),
            };
            self.items[index].element = before.element;
            return Vec::new();
        }
        // A map all 0 bakes as none: kept as none.
        let mut after = self.items[index].element.clone();
        if now.is_none() {
            self.surface_maps.remove(&file);
            if before.back {
                after.back_surface_map = false;
            } else {
                after.surface_map = false;
            }
            self.items[index].element = after.clone();
        }
        let mut edits = vec![Edit::SurfaceMap { id: id.into(), file, before: before.map, after: now }];
        if after != before.element {
            edits.push(Edit::Element { index, before: before.element, after });
        }
        edits
    }
}
