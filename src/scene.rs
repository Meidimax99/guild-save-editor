//! `.ed3` scenes and `.ogr` object groups — the engine's serialized object trees.
//!
//! Both start with a version word `0x3a6c00XX`; shipped files use `XX` in
//! `0xaa..=0xbb`. Readers:
//!
//! * `FUN_0043fae0` — scene (`.ed3`): header, object list, floor, properties
//! * `FUN_00440060` — group (`.ogr`): object list only
//! * `FUN_0043e890` — `rd_object`, one node of the recursive object tree
//! * `FUN_0043f5b0` — floor (terrain heightfield + water regions)
//! * `FUN_00434560` / `FUN_00434600` — key/value property lists
//! * `FUN_00401380` — `bio_rd_array_quick`: `u32 count, u32 elem_size, bytes`
//!
//! Version-dependent fields are noted inline with the threshold the engine checks.

use crate::reader::Reader;
use crate::error::{Error, Result};
use glam::{Vec3, Vec4};

pub const VERSION_BASE: u32 = 0x3a6c_0000;

/// Minor version (the low 16 bits); comparisons below use these.
type Ver = u32;

/// Number of light-set / keyframe slots for a version (`FUN_0043e6c0`).
fn key_slots(v: Ver) -> usize {
    match v {
        v if v > 0xb9 => 7,
        v if v > 0xa4 => 6,
        v if v >= 0xa1 => 4,
        _ => 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ObjectKind {
    Camera,
    Mesh,
    Dummy,
    Group,
    /// Type 4: a mesh whose geometry the engine treats specially (e.g. floor-aligned).
    Mesh4,
    Light(u32),
    /// `present == 0` node emitted by the exporter ("STRANGEFUCK" in the engine).
    Placeholder,
}

impl ObjectKind {
    fn from_raw(t: u32) -> Option<Self> {
        Some(match t {
            0 => Self::Camera,
            1 => Self::Mesh,
            2 => Self::Dummy,
            3 => Self::Group,
            4 => Self::Mesh4,
            5..=8 => Self::Light(t),
            _ => return None,
        })
    }

    pub fn is_mesh(self) -> bool {
        matches!(self, Self::Mesh | Self::Mesh4)
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct LightKey {
    pub a: u32,
    pub b: u32,
    pub c: u32,
    pub v0: Vec3,
    pub v1: Vec3,
    pub v2: Vec3,
    pub extra: [u32; 2],
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SceneObject {
    pub name: String,
    pub kind: ObjectKind,
    pub field_204: u32,
    pub field_220: u32,
    pub flag_216: u8,
    pub flags: Vec<u8>,
    /// Position (object + 0x4c).
    pub position: Vec3,
    /// Rotation, Euler angles (object + 0x84).
    pub rotation: Vec3,
    /// Pivot / secondary transform (object + 0x5c, + 0x90).
    pub pivot: Option<(Vec3, Vec3)>,
    /// Mesh LOD file names (without `.bgf`).
    pub lods: Vec<String>,
    pub lod_param: u32,
    /// 10 × (vec3, vec3) for version > 0xae dummies and meshes.
    pub anchors: Vec<(Vec3, Vec3)>,
    pub light_keys: Vec<LightKey>,
    pub light_params: Vec<u32>,
    pub properties: Vec<(String, String)>,
    pub children: Vec<SceneObject>,
}

impl SceneObject {
    fn new(name: String, kind: ObjectKind) -> Self {
        Self {
            name,
            kind,
            field_204: 0,
            field_220: 0,
            flag_216: 0,
            flags: Vec::new(),
            position: Vec3::ZERO,
            rotation: Vec3::ZERO,
            pivot: None,
            lods: Vec::new(),
            lod_param: 0,
            anchors: Vec::new(),
            light_keys: Vec::new(),
            light_params: Vec::new(),
            properties: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Depth-first iterator over this object and all descendants.
    pub fn walk(&self) -> Box<dyn Iterator<Item = &SceneObject> + '_> {
        Box::new(std::iter::once(self).chain(self.children.iter().flat_map(|c| c.walk())))
    }

    pub fn find(&self, name: &str) -> Option<&SceneObject> {
        self.walk().find(|o| o.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WaterRegion {
    pub texture: String,
    pub bytes: [u8; 7],
    pub speed: f32,
    pub a: u32,
    pub b: u32,
    pub v0: Vec4,
    pub v1: Vec4,
    pub c: u32,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Floor {
    pub name: String,
    /// Element size of the per-cell arrays (the grid's cells-per-row in practice).
    pub stride: u32,
    pub heights: Vec<u8>,
    pub heights2: Vec<u8>,
    pub water_mask: Vec<u8>,
    pub water: Vec<WaterRegion>,
    pub name2: String,
    pub map2: Vec<u8>,
    pub textures: Vec<String>,
    pub params: [u32; 2],
    pub offset: Option<Vec3>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct LightSet {
    pub ambient: Vec3,
    pub ambient2: Option<Vec3>,
    pub entries: Vec<[u32; 3]>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Scene {
    pub version: u32,
    pub camera: String,
    pub field_124: u32,
    pub ambient: Vec3,
    pub ambient2: Option<Vec3>,
    pub fog: Option<[u32; 4]>,
    pub light_sets: Vec<LightSet>,
    pub objects: Vec<SceneObject>,
    pub floor: Option<Floor>,
    pub properties: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Group {
    pub version: u32,
    pub objects: Vec<SceneObject>,
}

fn read_version(r: &mut Reader) -> Result<Ver> {
    let v = r.u32()?;
    if v & 0xffff_0000 != VERSION_BASE || v <= VERSION_BASE {
        return Err(Error::Invalid(format!("bad scene version {v:#x}")));
    }
    Ok(v & 0xffff)
}

fn read_array(r: &mut Reader, expected_elem: Option<u32>) -> Result<Vec<u8>> {
    let count = r.u32()?;
    let size = r.u32()?;
    if let Some(e) = expected_elem {
        if e != size {
            return Err(Error::Invalid(format!("rd_array: elem size {size} != expected {e}")));
        }
    }
    Ok(r.bytes(count as usize * size as usize)?.to_vec())
}

fn read_props(r: &mut Reader) -> Result<Vec<(String, String)>> {
    let n = r.u32()?;
    (0..n).map(|_| Ok((r.cstr()?, r.cstr()?))).collect()
}

/// `rd_object`: reads one node and, recursively, its children and following siblings.
/// Siblings are appended to `out`.
fn read_object(r: &mut Reader, v: Ver, out: &mut Vec<SceneObject>) -> Result<()> {
    let present = r.u8()?;
    let mut obj = if present == 0 {
        SceneObject::new(String::new(), ObjectKind::Placeholder)
    } else {
        let mut name = r.cstr()?;
        let field_204 = if v > 0xb1 { r.u32()? } else { 0 };
        let field_220 = if v > 0xaa { r.u32()? } else { 0 };
        let raw = r.u32()?;
        let (raw, flag_216) = if v < 0xa6 {
            let t = match raw {
                0 => 0,
                1 => 1,
                2 => 3,
                3 | 4 => 4,
                5..=8 => raw,
                _ => 2,
            };
            (t, 0)
        } else {
            let f = r.u8()?;
            if f == 0 && name.starts_with('!') {
                name.remove(0);
            }
            (raw, f)
        };
        let kind = ObjectKind::from_raw(raw)
            .ok_or_else(|| Error::Invalid(format!("unknown object type {raw} for {name:?}")))?;
        let mut o = SceneObject::new(name, kind);
        o.field_204 = field_204;
        o.field_220 = field_220;
        o.flag_216 = flag_216;
        read_object_body(r, v, &mut o)?;
        o
    };

    if r.u8()? != 0 {
        read_object(r, v, &mut obj.children)?;
    }
    let has_sibling = r.u8()? != 0;
    // Properties follow the sibling flag but belong to this node (FUN_00434560 is
    // called after the sibling chain has been read).
    let mut siblings = Vec::new();
    if has_sibling {
        read_object(r, v, &mut siblings)?;
    }
    if v > 0xa6 {
        obj.properties = read_props(r)?;
    }
    out.push(obj);
    out.extend(siblings);
    Ok(())
}

fn read_object_body(r: &mut Reader, v: Ver, o: &mut SceneObject) -> Result<()> {
    match o.kind {
        ObjectKind::Camera => {
            o.flags.push(r.u8()?);
            o.light_params.push(r.u32()?);
            o.position = r.vec3()?;
            o.pivot = Some((r.vec3()?, r.vec3()?));
        }
        ObjectKind::Mesh | ObjectKind::Mesh4 | ObjectKind::Dummy | ObjectKind::Group => {
            if o.kind.is_mesh() {
                o.flags.push(r.u8()?);
                o.flags.push(r.u8()?);
                if v > 0x0c {
                    o.flags.push(r.u8()?);
                    if v > 0xa5 {
                        o.flags.push(r.u8()?);
                        o.flags.push(r.u8()?);
                        if v > 0xb3 {
                            o.flags.push(r.u8()?);
                        }
                        if v > 0xb8 {
                            o.flags.push(r.u8()?);
                        }
                    }
                }
                o.lod_param = r.u32()?;
                let lod_count = r.u32()?;
                if v < 0xa4 {
                    for _ in 0..lod_count {
                        o.lods.push(r.cstr()?);
                    }
                    if v >= 0xa3 && r.u8()? != 0 {
                        o.lods.push(r.cstr()?);
                    }
                } else if lod_count > 0 {
                    o.lods.push(r.cstr()?);
                }
            }
            o.position = r.vec3()?;
            o.rotation = r.vec3()?;
            if r.u8()? != 0 {
                o.pivot = Some((r.vec3()?, r.vec3()?));
            }
            if v > 0xae {
                for _ in 0..10 {
                    o.anchors.push((r.vec3()?, r.vec3()?));
                }
            }
        }
        ObjectKind::Light(_) => {
            o.flags.push(r.u8()?);
            let slots = key_slots(v);
            if v < 0xa8 {
                o.light_params.push(r.u32()?);
                o.light_params.push(r.u32()?);
                if v >= 0x0c {
                    o.light_params.push(r.u32()?);
                }
                let p5c = r.vec3()?;
                o.position = r.vec3()?;
                o.pivot = Some((p5c, Vec3::ZERO));
                if v >= 0xa1 {
                    for _ in 0..slots {
                        let (a, b, c) = (r.u32()?, r.u32()?, r.u32()?);
                        let (v0, v1) = (r.vec3()?, r.vec3()?);
                        o.light_keys.push(LightKey { a, b, c, v0, v1, ..Default::default() });
                    }
                }
            } else {
                if v > 0xa8 {
                    o.flags.push(r.u8()?);
                }
                for _ in 0..3 {
                    o.light_params.push(r.u32()?);
                }
                let p5c = r.vec3()?;
                o.position = r.vec3()?;
                o.rotation = r.vec3()?;
                o.pivot = Some((p5c, Vec3::ZERO));
                if v > 0xab {
                    o.light_params.push(r.u32()?);
                    o.light_params.push(r.u32()?);
                }
                for _ in 0..slots {
                    let (a, b, c) = (r.u32()?, r.u32()?, r.u32()?);
                    let (v0, v1, v2) = (r.vec3()?, r.vec3()?, r.vec3()?);
                    let extra = if v > 0xab { [r.u32()?, r.u32()?] } else { [0; 2] };
                    o.light_keys.push(LightKey { a, b, c, v0, v1, v2, extra });
                }
            }
        }
        ObjectKind::Placeholder => unreachable!(),
    }
    Ok(())
}

fn read_floor(r: &mut Reader, v: Ver) -> Result<Floor> {
    let mut f = Floor { name: r.cstr()?, ..Default::default() };
    if !f.name.is_empty() && v > 0xac {
        f.stride = r.u32()?;
        f.heights = read_array(r, Some(f.stride))?;
        if v > 0xba {
            f.heights2 = read_array(r, Some(f.stride * 4))?;
        }
    }
    if v > 0xa9 && r.u8()? != 0 {
        let count = r.u32()?;
        if count > 0 {
            if v > 0xac {
                f.water_mask = read_array(r, Some(f.stride))?;
            }
            for _ in 0..count {
                let texture = r.cstr()?;
                let bytes: [u8; 7] = r.bytes(7)?.try_into().unwrap();
                let speed = if v < 0xb6 { 63.0 } else { r.f32()? };
                let (a, b) = (r.u32()?, r.u32()?);
                let mut v0 = Vec4::new(r.f32()?, r.f32()?, r.f32()?, r.f32()?);
                if v < 0xac {
                    v0 *= 1.0 / 3.0;
                }
                let v1 = Vec4::new(r.f32()?, r.f32()?, r.f32()?, r.f32()?);
                let c = if v < 0xb9 { 0 } else { r.u32()? };
                f.water.push(WaterRegion { texture, bytes, speed, a, b, v0, v1, c });
            }
        }
    }
    f.name2 = r.cstr()?;
    if v > 0xac && !f.name2.is_empty() {
        f.map2 = read_array(r, Some(f.stride))?;
    }
    if v < 0x10 {
        for _ in 0..5 {
            r.cstr()?;
        }
    }
    let ntex = if v < 0xb0 { 6 } else { 8 };
    for _ in 0..ntex {
        f.textures.push(r.cstr()?);
    }
    f.params = [r.u32()?, r.u32()?];
    if v < 0x10 {
        for _ in 0..256 {
            r.cstr()?;
        }
    }
    if v > 0x0d {
        f.offset = Some(r.vec3()?);
    }
    Ok(f)
}

impl Scene {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let (s, used) = Self::parse_prefix(data)?;
        if used != data.len() {
            return Err(Error::Invalid(format!("scene: {} trailing bytes", data.len() - used)));
        }
        Ok(s)
    }

    /// Parse a scene embedded at the start of `data`; returns the scene and
    /// the number of bytes it occupies.
    pub fn parse_prefix(data: &[u8]) -> Result<(Self, usize)> {
        let mut r = Reader::new(data);
        let v = read_version(&mut r)?;
        if v < 0x0b {
            return Err(Error::Invalid("Scene too old".into()));
        }
        let mut s = Scene { version: v, ..Default::default() };
        s.camera = r.cstr()?;
        s.field_124 = r.u32()?;
        s.ambient = r.vec3()?;
        if v > 0xb4 {
            s.ambient2 = Some(r.vec3()?);
        }
        if v > 0xb2 {
            let a = if v > 0xb6 { r.u32()? } else { 0 };
            s.fog = Some([a, r.u32()?, r.u32()?, r.u32()?]);
        }
        if v > 0xa1 {
            for _ in 0..key_slots(v) {
                let mut ls = LightSet { ambient: r.vec3()?, ..Default::default() };
                if v > 0xb2 {
                    if v > 0xb4 {
                        ls.ambient2 = Some(r.vec3()?);
                    }
                    for _ in 0..6 {
                        ls.entries.push([r.u32()?, r.u32()?, r.u32()?]);
                    }
                }
                s.light_sets.push(ls);
            }
        }
        let n = r.u32()?;
        for _ in 0..n {
            read_object(&mut r, v, &mut s.objects)?;
        }
        if r.u8()? != 0 {
            s.floor = Some(read_floor(&mut r, v)?);
        }
        if v > 0xa6 {
            s.properties = read_props(&mut r)?;
        }
        Ok((s, r.pos()))
    }

    pub fn walk(&self) -> impl Iterator<Item = &SceneObject> {
        self.objects.iter().flat_map(|o| o.walk())
    }

    pub fn find(&self, name: &str) -> Option<&SceneObject> {
        self.walk().find(|o| o.name.eq_ignore_ascii_case(name))
    }
}

impl Group {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let v = read_version(&mut r)?;
        let mut g = Group { version: v, objects: Vec::new() };
        if v < 0xae {
            read_object(&mut r, v, &mut g.objects)?;
        } else {
            let n = r.u32()?;
            for _ in 0..n {
                read_object(&mut r, v, &mut g.objects)?;
            }
        }
        if !r.at_end() {
            return Err(Error::Invalid(format!("group: {} trailing bytes", r.remaining())));
        }
        Ok(g)
    }

    pub fn walk(&self) -> impl Iterator<Item = &SceneObject> {
        self.objects.iter().flat_map(|o| o.walk())
    }
}
