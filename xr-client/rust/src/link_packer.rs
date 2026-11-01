//! Godot face of the cross-volume link pack (ADR-2164 D8, ADR-2158 D6).
//!
//! `link_layer.gd` owns the link records, their order, the generation purge and the per-volume
//! and per-connection state. Each frame it hands this class the connections, the volumes'
//! content-to-world transforms and the layer's inverse transform, and uploads the buffer
//! [`LinkPacker::pack`] returns. The ends are resolved in Rust by
//! [`crate::render_store::pack_links`], through the same fold-aware answer the edge pack draws
//! with, so GDScript holds no second resolver and no per-link loop.

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

#[cfg(not(test))]
use godot::prelude::*;

#[cfg(not(test))]
use crate::binary_protocol::BinaryProtocolClient;
#[cfg(not(test))]
use crate::render_store::{pack_links, LinkFrame, LinkSpec, LinkVolume, RenderStore};

/// Transform3D rows → row-major 3×4 (basis rows, origin in the fourth column).
#[cfg(not(test))]
fn rows12(t: &Transform3D) -> [f32; 12] {
    let r = &t.basis.rows;
    let o = t.origin;
    [
        r[0].x, r[0].y, r[0].z, o.x, r[1].x, r[1].y, r[1].z, o.y, r[2].x, r[2].y, r[2].z, o.z,
    ]
}

/// Packs the link MultiMesh buffer from the connections' render stores. `LinkPacker.new()`.
#[cfg(not(test))]
#[derive(GodotClass)]
#[class(base = RefCounted)]
pub struct LinkPacker {
    base: Base<RefCounted>,
    specs: Vec<LinkSpec>,
    frame: LinkFrame,
}

#[cfg(not(test))]
#[godot_api]
impl IRefCounted for LinkPacker {
    fn init(base: Base<RefCounted>) -> Self {
        Self {
            base,
            specs: Vec::new(),
            frame: LinkFrame::default(),
        }
    }
}

#[cfg(not(test))]
#[godot_api]
impl LinkPacker {
    /// Replace the link table. `ends` holds six ints per link: source connection index, target
    /// connection index, source node id, target node id, source volume index, target volume index
    /// (the indices into the `clients` and volume arrays given to [`pack`](Self::pack));
    /// `colours` holds one base colour per link. Every link starts un-highlighted.
    #[func]
    fn set_links(&mut self, ends: PackedInt32Array, colours: PackedColorArray) {
        let (e, c) = (ends.as_slice(), colours.as_slice());
        self.specs = e
            .as_chunks::<6>()
            .0
            .iter()
            .zip(c.iter())
            .map(|(k, col)| LinkSpec {
                src_conn: k[0].max(0) as usize,
                dst_conn: k[1].max(0) as usize,
                src_id: k[2] as u32,
                dst_id: k[3] as u32,
                src_vol: k[4].max(0) as usize,
                dst_vol: k[5].max(0) as usize,
                colour: [col.r, col.g, col.b, col.a],
                lit: false,
            })
            .collect();
    }

    /// Mark which links are highlighted: one byte per link, non-zero = lit.
    #[func]
    fn set_lit(&mut self, lit: PackedByteArray) {
        let l = lit.as_slice();
        for (i, s) in self.specs.iter_mut().enumerate() {
            s.lit = l.get(i).copied().unwrap_or(0) != 0;
        }
    }

    /// Pack this frame. `clients[i]` is the `BinaryProtocolClient` of connection `i`, or null when
    /// that connection is unusable; `vol_state[j]` is volume `j`'s state (0 unmapped, 1 hidden,
    /// 2 drawable) and `vol_xf[j]` its content-to-world `Transform3D`; `inv` takes world space
    /// into the layer's local space. Returns the MultiMesh buffer: links in table order, then the
    /// highlight ring markers, 16 floats each.
    #[func]
    fn pack(
        &mut self,
        clients: VariantArray,
        vol_state: PackedByteArray,
        vol_xf: VariantArray,
        inv: Transform3D,
    ) -> PackedFloat32Array {
        let owners: Vec<Option<Gd<BinaryProtocolClient>>> = clients
            .iter_shared()
            .map(|v| v.try_to::<Gd<BinaryProtocolClient>>().ok())
            .collect();
        let binds: Vec<Option<GdRef<BinaryProtocolClient>>> = owners
            .iter()
            .map(|g| g.as_ref().map(|g| g.bind()))
            .collect();
        let stores: Vec<Option<&RenderStore>> = binds
            .iter()
            .map(|b| b.as_ref().map(|c| c.render_store()))
            .collect();
        let volumes: Vec<LinkVolume> = vol_state
            .as_slice()
            .iter()
            .enumerate()
            .map(|(j, &state)| LinkVolume {
                state,
                xf: vol_xf
                    .get(j)
                    .and_then(|v| v.try_to::<Transform3D>().ok())
                    .map(|t| rows12(&t))
                    .unwrap_or([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
            })
            .collect();
        pack_links(
            &stores,
            &volumes,
            &self.specs,
            &rows12(&inv),
            &mut self.frame,
        );
        PackedFloat32Array::from(self.frame.buf.as_slice())
    }

    /// Link instances drawn by the last pack.
    #[func]
    fn link_count(&self) -> i64 {
        self.frame.links as i64
    }

    /// Ring-marker instances drawn by the last pack.
    #[func]
    fn marker_count(&self) -> i64 {
        self.frame.markers as i64
    }

    /// Links the last pack could not resolve (volume unmapped, connection unusable, unknown node).
    #[func]
    fn unresolved_count(&self) -> i64 {
        self.frame.unresolved as i64
    }

    /// Source ends of the last pack's drawn links, world space, in draw order.
    #[func]
    fn segment_a(&self) -> PackedVector3Array {
        self.frame
            .segments
            .iter()
            .map(|s| Vector3::new(s.a[0], s.a[1], s.a[2]))
            .collect()
    }

    /// Target ends of the last pack's drawn links, world space, in draw order.
    #[func]
    fn segment_b(&self) -> PackedVector3Array {
        self.frame
            .segments
            .iter()
            .map(|s| Vector3::new(s.b[0], s.b[1], s.b[2]))
            .collect()
    }

    /// For each drawn link of the last pack, its index in the table given to `set_links`.
    #[func]
    fn segment_links(&self) -> PackedInt32Array {
        self.frame.segments.iter().map(|s| s.spec as i32).collect()
    }
}
