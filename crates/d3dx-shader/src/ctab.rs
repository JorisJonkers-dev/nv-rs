//! The constant table of d3dbc bytecode (the payload of the `CTAB` comment), read in pure Rust
//! (nv-linux#198). The layout is the public `D3DXSHADER_CONSTANTTABLE` of d3dx9shader.h; every
//! offset is relative to the start of the payload (the tag is already stripped, see
//! [`crate::constant_table`]). Nothing here panics on malformed bytes: a bad offset or count
//! answers `None`.

/// `D3DXREGISTER_SET`.
pub const RS_BOOL: u32 = 0;
pub const RS_INT4: u32 = 1;
pub const RS_FLOAT4: u32 = 2;
pub const RS_SAMPLER: u32 = 3;

/// `D3DXPARAMETER_CLASS`.
pub const CLASS_SCALAR: u32 = 0;
pub const CLASS_VECTOR: u32 = 1;
pub const CLASS_MATRIX_ROWS: u32 = 2;
pub const CLASS_MATRIX_COLUMNS: u32 = 3;
pub const CLASS_OBJECT: u32 = 4;
pub const CLASS_STRUCT: u32 = 5;

/// The most nodes (constants, elements, members) a table is walked to: a hostile table cannot make
/// the walk run away.
const MAX_NODES: usize = 1 << 16;

/// `D3DXCONSTANTTABLE_DESC` with the creator as an offset into the payload.
#[derive(Debug, PartialEq, Eq)]
pub struct TableDesc {
    pub creator: Option<u32>,
    pub version: u32,
    pub constants: u32,
}

/// What `D3DXCONSTANT_DESC` holds, with the name and the default as offsets into the payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstantDesc {
    pub name: u32,
    pub register_set: u32,
    pub register_index: u32,
    pub register_count: u32,
    pub class: u32,
    pub kind: u32,
    pub rows: u32,
    pub columns: u32,
    pub elements: u32,
    pub struct_members: u32,
    pub bytes: u32,
    pub default: Option<u32>,
}

/// One handle: a top-level constant, an element of an array, or a member of a structure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// The node it belongs to (an element or a member), `None` for a top-level constant.
    pub parent: Option<usize>,
    pub desc: ConstantDesc,
    /// The type's offset (members and elements are read from it).
    type_at: u32,
    /// Which element of the parent (`Some`) rather than which member (`None`).
    element: Option<u32>,
    /// The member's index in its parent's structure.
    member: u32,
}

pub struct Table<'a> {
    data: &'a [u8],
}

impl<'a> Table<'a> {
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        let t = Table { data };
        // Size (28), creator, version, constants, constant info, flags, target.
        if t.u32(0)? < 28 || t.u32(0)? as usize > data.len().max(28) {
            return None;
        }
        let count = t.u32(12)? as usize;
        let at = t.u32(16)? as usize;
        data.get(at..at.checked_add(count.checked_mul(20)?)?)?;
        Some(t)
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let c = self.data.get(at..at.checked_add(4)?)?;
        Some(u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
    }

    fn u16(&self, at: usize) -> Option<u32> {
        let c = self.data.get(at..at.checked_add(2)?)?;
        Some(u32::from(u16::from_le_bytes([c[0], c[1]])))
    }

    pub fn desc(&self) -> TableDesc {
        let creator = self.u32(4).filter(|o| self.cstr(*o).is_some());
        TableDesc {
            creator,
            version: self.u32(8).unwrap_or(0),
            constants: self.u32(12).unwrap_or(0),
        }
    }

    /// The text at `at` up to its nul, without it.
    pub fn cstr(&self, at: u32) -> Option<&'a [u8]> {
        let rest = self.data.get(at as usize..)?;
        Some(&rest[..rest.iter().position(|b| *b == 0)?])
    }

    /// The `index`th constant of the table.
    pub fn constant(&self, index: u32) -> Option<Node> {
        if index >= self.desc().constants {
            return None;
        }
        let at = self.u32(16)? as usize + 20 * index as usize;
        let type_at = self.u32(at + 12)?;
        let default = self.u32(at + 16)?;
        let mut desc = self.type_desc(type_at)?;
        desc.name = self.u32(at)?;
        self.cstr(desc.name)?;
        desc.register_set = self.u16(at + 4)?;
        desc.register_index = self.u16(at + 6)?;
        desc.register_count = self.u16(at + 8)?;
        desc.default = (default != 0).then_some(default);
        Some(Node {
            parent: None,
            desc,
            type_at,
            element: None,
            member: 0,
        })
    }

    /// The type fields of a `D3DXSHADER_TYPEINFO`, the rest zero.
    fn type_desc(&self, at: u32) -> Option<ConstantDesc> {
        let at = at as usize;
        let (class, kind) = (self.u16(at)?, self.u16(at + 2)?);
        let (rows, columns) = (self.u16(at + 4)?, self.u16(at + 6)?);
        let (elements, members) = (self.u16(at + 8)?.max(1), self.u16(at + 10)?);
        Some(ConstantDesc {
            name: 0,
            register_set: 0,
            register_index: 0,
            register_count: 0,
            class,
            kind,
            rows,
            columns,
            elements,
            struct_members: members,
            bytes: 0,
            default: None,
        })
        .map(|mut d| {
            d.bytes = self.bytes(&d, at as u32, 0);
            d
        })
    }

    /// `Bytes` of a type: four per component, a structure's members summed, times the elements.
    fn bytes(&self, d: &ConstantDesc, type_at: u32, depth: u32) -> u32 {
        let one = if d.class == CLASS_STRUCT && depth < 8 {
            let list = self.u32(type_at as usize + 12).unwrap_or(0) as usize;
            (0..d.struct_members as usize)
                .filter_map(|i| {
                    let t = self.u32(list + 8 * i + 4)?;
                    let m = self.type_desc_shallow(t)?;
                    Some(self.bytes(&m, t, depth + 1))
                })
                .fold(0u32, u32::saturating_add)
        } else {
            4 * d.rows * d.columns
        };
        one.saturating_mul(d.elements)
    }

    fn type_desc_shallow(&self, at: u32) -> Option<ConstantDesc> {
        let a = at as usize;
        Some(ConstantDesc {
            name: 0,
            register_set: 0,
            register_index: 0,
            register_count: 0,
            class: self.u16(a)?,
            kind: self.u16(a + 2)?,
            rows: self.u16(a + 4)?,
            columns: self.u16(a + 6)?,
            elements: self.u16(a + 8)?.max(1),
            struct_members: self.u16(a + 10)?,
            bytes: 0,
            default: None,
        })
    }

    /// Registers one element of the type takes: a matrix its rows (columns when column-major), a
    /// structure its members' sum, anything else one.
    fn registers(&self, d: &ConstantDesc, type_at: u32, depth: u32) -> u32 {
        match d.class {
            CLASS_MATRIX_ROWS => d.rows,
            CLASS_MATRIX_COLUMNS => d.columns,
            CLASS_STRUCT if depth < 8 => {
                let list = self.u32(type_at as usize + 12).unwrap_or(0) as usize;
                (0..d.struct_members as usize)
                    .filter_map(|i| {
                        let t = self.u32(list + 8 * i + 4)?;
                        let m = self.type_desc_shallow(t)?;
                        Some(self.registers(&m, t, depth + 1).saturating_mul(m.elements))
                    })
                    .fold(0u32, u32::saturating_add)
            }
            _ => 1,
        }
    }

    /// Every node (constants first, then each one's elements or members, depth first) in the
    /// order the handles are numbered. A node's number is its handle's index.
    pub fn nodes(&self) -> Vec<Node> {
        let mut nodes = Vec::new();
        for i in 0..self.desc().constants {
            let Some(top) = self.constant(i) else { break };
            let at = nodes.len();
            nodes.push(top);
            self.expand(&mut nodes, at, 0);
            if nodes.len() > MAX_NODES {
                nodes.truncate(MAX_NODES);
                break;
            }
        }
        nodes
    }

    fn expand(&self, nodes: &mut Vec<Node>, at: usize, depth: u32) {
        if depth >= 8 || nodes.len() > MAX_NODES {
            return;
        }
        let parent = nodes[at].clone();
        let d = &parent.desc;
        if parent.element.is_none() && d.elements > 1 {
            // An array: its elements (a structure's members hang below each element).
            let per = self.registers(d, parent.type_at, 0);
            for e in 0..d.elements {
                let mut c = d.clone();
                c.elements = 1;
                c.register_index = d.register_index + e * per;
                c.register_count = per;
                c.bytes = d.bytes / d.elements;
                c.default = d.default.map(|x| x + e * c.bytes);
                let n = nodes.len();
                nodes.push(Node {
                    parent: Some(at),
                    desc: c,
                    type_at: parent.type_at,
                    element: Some(e),
                    member: 0,
                });
                self.expand(nodes, n, depth + 1);
                if nodes.len() > MAX_NODES {
                    return;
                }
            }
        } else if d.class == CLASS_STRUCT {
            let list = self.u32(parent.type_at as usize + 12).unwrap_or(0) as usize;
            let mut register = d.register_index;
            let mut bytes = 0;
            for i in 0..d.struct_members as usize {
                let (Some(name), Some(t)) = (self.u32(list + 8 * i), self.u32(list + 8 * i + 4))
                else {
                    return;
                };
                let (Some(mut c), Some(_)) = (self.type_desc(t), self.cstr(name)) else {
                    return;
                };
                let regs = self.registers(&c, t, 0);
                c.name = name;
                c.register_set = d.register_set;
                c.register_index = register;
                c.register_count = regs.saturating_mul(c.elements);
                c.default = d.default.map(|x| x + bytes);
                register += c.register_count;
                bytes += c.bytes;
                let n = nodes.len();
                nodes.push(Node {
                    parent: Some(at),
                    desc: c.clone(),
                    type_at: t,
                    element: None,
                    member: i as u32,
                });
                self.expand(nodes, n, depth + 1);
                if nodes.len() > MAX_NODES {
                    return;
                }
            }
        }
    }

    /// The children of node `at` directly below it, in order.
    fn children(nodes: &[Node], at: usize) -> impl Iterator<Item = usize> + '_ {
        (0..nodes.len()).filter(move |i| nodes[*i].parent == Some(at))
    }

    /// `GetConstant(handle, index)`: top-level constant `index` (no parent) or member `index` of a
    /// structure. Elements are reached with [`Table::element`].
    pub fn get_constant(&self, nodes: &[Node], parent: Option<usize>, index: u32) -> Option<usize> {
        match parent {
            None => (index < self.desc().constants).then(|| {
                (0..nodes.len())
                    .filter(|i| nodes[*i].parent.is_none())
                    .nth(index as usize)
            })?,
            Some(p) => Self::children(nodes, p)
                .filter(|c| nodes[*c].element.is_none())
                .nth(index as usize),
        }
    }

    /// `GetConstantElement(handle, index)`.
    pub fn element(&self, nodes: &[Node], of: usize, index: u32) -> Option<usize> {
        Self::children(nodes, of).find(|c| nodes[*c].element == Some(index))
    }

    /// `GetConstantByName(handle, name)`: `name` is `a`, `a[2]`, `a.b`, `a[1].b[0]`, ... below the
    /// node `parent` (the whole table when `None`).
    pub fn by_name(&self, nodes: &[Node], parent: Option<usize>, name: &[u8]) -> Option<usize> {
        let mut at = parent;
        let mut rest = name;
        let mut first = true;
        while !rest.is_empty() {
            if !first {
                rest = rest.strip_prefix(b".")?;
            }
            first = false;
            let end = rest
                .iter()
                .position(|b| *b == b'.' || *b == b'[')
                .unwrap_or(rest.len());
            let (segment, tail) = rest.split_at(end);
            rest = tail;
            let hit = (0..nodes.len()).find(|i| {
                nodes[*i].parent == at
                    && nodes[*i].element.is_none()
                    && self.cstr(nodes[*i].desc.name) == Some(segment)
            })?;
            at = Some(hit);
            while let Some(r) = rest.strip_prefix(b"[") {
                let close = r.iter().position(|b| *b == b']')?;
                let n: u32 = std::str::from_utf8(&r[..close]).ok()?.parse().ok()?;
                at = Some(self.element(nodes, at?, n)?);
                rest = &r[close + 1..];
            }
        }
        at
    }
}

fn put16(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 2].copy_from_slice(&(v as u16).to_le_bytes());
}
fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// A hand-built constant table (a fixture for tests, no shader or game data) of `float4 tint (c4, default 1,2,3,4)`, `float4x4 world (c0..c3, row major)`,
/// `float4 lights[2] (c8, c9)`, `struct { float4 a; float4x4 m } s (c10..c14)` and
/// `sampler2D diffuse (s1)`.
pub fn synthetic_table() -> Vec<u8> {
    let mut b = vec![0u8; 28 + 5 * 20];
    let text = |b: &mut Vec<u8>, s: &str| {
        let at = b.len() as u32;
        b.extend_from_slice(s.as_bytes());
        b.push(0);
        at
    };
    let creator = text(&mut b, "Microsoft (R) HLSL Shader Compiler 9.29");
    let target = text(&mut b, "ps_3_0");
    let names: Vec<u32> = ["tint", "world", "lights", "s", "diffuse", "a", "m"]
        .iter()
        .map(|n| text(&mut b, n))
        .collect();
    while b.len() % 4 != 0 {
        b.push(0);
    }
    let types = |b: &mut Vec<u8>, class, kind, rows, cols, elems, members, list| {
        let at = b.len() as u32;
        let mut t = vec![0u8; 16];
        put16(&mut t, 0, class);
        put16(&mut t, 2, kind);
        put16(&mut t, 4, rows);
        put16(&mut t, 6, cols);
        put16(&mut t, 8, elems);
        put16(&mut t, 10, members);
        put32(&mut t, 12, list);
        b.extend(t);
        at
    };
    let float4 = types(&mut b, 1, 3, 1, 4, 1, 0, 0);
    let matrix = types(&mut b, 2, 3, 4, 4, 1, 0, 0);
    let lights = types(&mut b, 1, 3, 1, 4, 2, 0, 0);
    let sampler = types(&mut b, 4, 5, 1, 1, 1, 0, 0);
    let list = b.len() as u32;
    for (n, t) in [(names[5], float4), (names[6], matrix)] {
        b.extend(n.to_le_bytes());
        b.extend(t.to_le_bytes());
    }
    let strukt = types(&mut b, 5, 0, 1, 20, 1, 2, list);
    let default = b.len() as u32;
    for v in [1.0f32, 2.0, 3.0, 4.0] {
        b.extend(v.to_le_bytes());
    }
    let info: [(u32, u32, u32, u32, u32, u32); 5] = [
        (names[0], 2, 4, 1, float4, default),
        (names[1], 2, 0, 4, matrix, 0),
        (names[2], 2, 8, 2, lights, 0),
        (names[3], 2, 10, 5, strukt, 0),
        (names[4], 3, 1, 1, sampler, 0),
    ];
    for (i, (n, set, idx, count, t, d)) in info.iter().enumerate() {
        let at = 28 + 20 * i;
        put32(&mut b, at, *n);
        put16(&mut b, at + 4, *set);
        put16(&mut b, at + 6, *idx);
        put16(&mut b, at + 8, *count);
        put32(&mut b, at + 12, *t);
        put32(&mut b, at + 16, *d);
    }
    put32(&mut b, 0, 28);
    put32(&mut b, 4, creator);
    put32(&mut b, 8, 0xffff_0300);
    put32(&mut b, 12, 5);
    put32(&mut b, 16, 28);
    put32(&mut b, 20, 0);
    put32(&mut b, 24, target);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Vec<u8> {
        synthetic_table()
    }

    #[test]
    fn the_table_description() {
        let bytes = table();
        let t = Table::parse(&bytes).unwrap();
        let d = t.desc();
        assert_eq!((d.version, d.constants), (0xffff_0300, 5));
        assert_eq!(
            t.cstr(d.creator.unwrap()).unwrap(),
            b"Microsoft (R) HLSL Shader Compiler 9.29"
        );
    }

    #[test]
    fn constants_by_index_and_name() {
        let bytes = table();
        let t = Table::parse(&bytes).unwrap();
        let nodes = t.nodes();
        let tint = &nodes[t.by_name(&nodes, None, b"tint").unwrap()].desc;
        assert_eq!(
            (tint.register_set, tint.register_index, tint.register_count),
            (RS_FLOAT4, 4, 1)
        );
        assert_eq!(
            (tint.class, tint.rows, tint.columns, tint.bytes),
            (1, 1, 4, 16)
        );
        let d = tint.default.unwrap() as usize;
        assert_eq!(bytes[d..d + 4], 1.0f32.to_le_bytes());
        let world = &nodes[t.get_constant(&nodes, None, 1).unwrap()].desc;
        assert_eq!(t.cstr(world.name).unwrap(), b"world");
        assert_eq!((world.register_count, world.bytes), (4, 64));
        assert_eq!(t.get_constant(&nodes, None, 5), None);
        assert_eq!(t.by_name(&nodes, None, b"nope"), None);
        assert_eq!(t.by_name(&nodes, None, b"TINT"), None);
        let diffuse = &nodes[t.by_name(&nodes, None, b"diffuse").unwrap()].desc;
        assert_eq!(
            (diffuse.register_set, diffuse.register_index),
            (RS_SAMPLER, 1)
        );
    }

    #[test]
    fn arrays_and_structures() {
        let bytes = table();
        let t = Table::parse(&bytes).unwrap();
        let nodes = t.nodes();
        let lights = t.by_name(&nodes, None, b"lights").unwrap();
        assert_eq!(nodes[lights].desc.elements, 2);
        let second = t.element(&nodes, lights, 1).unwrap();
        assert_eq!(second, t.by_name(&nodes, None, b"lights[1]").unwrap());
        assert_eq!(nodes[second].desc.register_index, 9);
        assert_eq!(
            (nodes[second].desc.elements, nodes[second].desc.bytes),
            (1, 16)
        );
        assert_eq!(t.element(&nodes, lights, 2), None);
        let m = t.by_name(&nodes, None, b"s.m").unwrap();
        assert_eq!(nodes[m].desc.register_index, 11);
        assert_eq!(nodes[m].desc.register_count, 4);
        let s = t.by_name(&nodes, None, b"s").unwrap();
        assert_eq!(nodes[s].desc.bytes, 80);
        assert_eq!(t.get_constant(&nodes, Some(s), 1), Some(m));
        assert_eq!(
            t.by_name(&nodes, Some(s), b"a"),
            t.get_constant(&nodes, Some(s), 0)
        );
        assert_eq!(t.get_constant(&nodes, Some(s), 2), None);
    }

    #[test]
    fn malformed_tables_answer_none() {
        let bytes = table();
        assert!(Table::parse(&bytes[..20]).is_none());
        assert!(Table::parse(&[]).is_none());
        let mut bad = bytes.clone();
        put32(&mut bad, 12, 0x00ff_ffff);
        assert!(Table::parse(&bad).is_none());
        let mut bad = bytes.clone();
        put32(&mut bad, 28, 0xffff_0000);
        let t = Table::parse(&bad).unwrap();
        assert!(t.nodes().is_empty());
        assert_eq!(t.by_name(&t.nodes(), None, b"tint"), None);
    }
}
