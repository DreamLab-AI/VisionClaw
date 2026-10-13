//! GPU semantic forces: DAG layering, type / physicality / role clustering, collision,
//! attribute-weighted springs, maturity layering and dynamic ontology relationships.
//!
//! These kernels (`semantic_forces.cu`) existed for a long time but nothing launched them:
//! `SemanticForcesEngine::apply_semantic_forces` had no callers and the FFI entry points
//! were no-op stubs in PTX-only builds. This module is the one launch path. It runs from the
//! live physics step (`UnifiedGPUCompute::execute`), loads the semantic PTX through `cust`
//! like every other live kernel, and is owned by that step, so it is never a hidden fallback:
//! it is either constructed and applied, or absent.
//!
//! # Data flow
//!
//! 1. The per-node and per-edge metadata arrays are DERIVED on the host by
//!    `SemanticForcesEngine::initialize` (node type, DAG level, physicality, role, maturity,
//!    cross-domain links, edge type). [`SemanticInputs::from_engine`] copies them out once
//!    per graph upload.
//! 2. Each physics step: gather the live SoA positions into float3, zero a scratch force
//!    array, run the enabled kernels (they only ADD to the scratch forces), then add the
//!    scratch forces into the live SoA force buffers before integration. The kernels cannot
//!    overwrite live state, and the sum is applied exactly as computed.
//! 3. Configuration is the central settings store's `physics.semantic` group
//!    ([`SemanticForces`]): no scale, cap or default is added here. It lives in `__constant__`
//!    memory of THIS module (not the statically linked object's), written from the
//!    explicit-width images below. `sf_config_probe` reads the
//!    values back through the device's own struct layout so a padding mistake is a test
//!    failure, not silently wrong forces.

use crate::gpu::semantic_forces::SemanticForcesEngine;
use visionclaw_domain::types::SemanticForces;
use crate::services::semantic_type_registry::RelationshipForceConfig;
use anyhow::{anyhow, Result};
use cust::launch;
use cust::memory::{CopyDestination, DeviceBuffer, DevicePointer};
use cust::module::Module;
use cust::stream::Stream;
use cust_core::DeviceCopy;
use std::collections::HashMap;
use visionclaw_domain::models::graph::GraphData;

const BLOCK: u32 = 256;
/// Slots for the per-family centroid tables. `node_type_to_int` yields 0..=8; physicality
/// has 4 values and role 5, so 16 covers all with room for custom types.
const TYPE_SLOTS: usize = 16;
const MAX_RELATIONSHIP_TYPES: usize = 256;

/// Words in the device `SemanticConfig`: eight sub-structs, each its floats plus one 4-byte
/// slot for the trailing `bool enabled` (the struct is 4-byte aligned, so it pads to a word).
const CONFIG_WORDS: usize = 35;
/// `DynamicRelationshipBuffer`: 256 x {f32, f32, i32, u32} + num_types + buffer_version +
/// enabled (bool padded to a word).
const DYNAMIC_WORDS: usize = MAX_RELATIONSHIP_TYPES * 4 + 3;

#[derive(Clone, Copy)]
#[repr(C)]
struct ConfigImage([u32; CONFIG_WORDS]);
// SAFETY: plain-old-data array of u32; no pointers, no padding.
unsafe impl DeviceCopy for ConfigImage {}

#[derive(Clone, Copy)]
#[repr(C)]
struct DynamicImage([u32; DYNAMIC_WORDS]);
// SAFETY: as above.
unsafe impl DeviceCopy for DynamicImage {}

fn f(v: f32) -> u32 {
    v.to_bits()
}

fn b(v: bool) -> u32 {
    u32::from(v)
}

/// Serialise the stored config into the device struct's exact word layout.
fn pack_config(c: &SemanticForces) -> ConfigImage {
    let w = [
        // dag
        f(c.dag.vertical_spacing),
        f(c.dag.horizontal_spacing),
        f(c.dag.level_attraction),
        f(c.dag.sibling_repulsion),
        b(c.dag.enabled),
        // type_cluster
        f(c.type_cluster.cluster_attraction),
        f(c.type_cluster.cluster_radius),
        f(c.type_cluster.inter_cluster_repulsion),
        b(c.type_cluster.enabled),
        // collision
        f(c.collision.min_distance),
        f(c.collision.collision_strength),
        f(c.collision.node_radius),
        b(c.collision.enabled),
        // attribute_spring
        f(c.attribute_spring.base_spring_k),
        f(c.attribute_spring.weight_multiplier),
        f(c.attribute_spring.rest_length_min),
        f(c.attribute_spring.rest_length_max),
        b(c.attribute_spring.enabled),
        // physicality_cluster
        f(c.physicality_cluster.cluster_attraction),
        f(c.physicality_cluster.cluster_radius),
        f(c.physicality_cluster.inter_physicality_repulsion),
        b(c.physicality_cluster.enabled),
        // role_cluster
        f(c.role_cluster.cluster_attraction),
        f(c.role_cluster.cluster_radius),
        f(c.role_cluster.inter_role_repulsion),
        b(c.role_cluster.enabled),
        // maturity_layout
        f(c.maturity_layout.vertical_spacing),
        f(c.maturity_layout.level_attraction),
        f(c.maturity_layout.stage_separation),
        b(c.maturity_layout.enabled),
        // cross_domain
        f(c.cross_domain.base_strength),
        f(c.cross_domain.link_count_multiplier),
        f(c.cross_domain.max_strength_boost),
        f(c.cross_domain.rest_length),
        b(c.cross_domain.enabled),
    ];
    ConfigImage(w)
}

/// Serialise relationship configs into `DynamicRelationshipBuffer`. `is_directional` is an
/// `int` on the device but a 1-byte `bool` in `RelationshipForceConfig`, so copying that
/// struct's bytes would put padding garbage in the high bytes; each word is built explicitly.
fn pack_dynamic(configs: &[RelationshipForceConfig], enabled: bool, version: i32) -> DynamicImage {
    let mut w = [0u32; DYNAMIC_WORDS];
    let n = configs.len().min(MAX_RELATIONSHIP_TYPES);
    for (i, c) in configs.iter().take(n).enumerate() {
        w[i * 4] = f(c.strength);
        w[i * 4 + 1] = f(c.rest_length);
        w[i * 4 + 2] = b(c.is_directional);
        w[i * 4 + 3] = c.force_type;
    }
    w[MAX_RELATIONSHIP_TYPES * 4] = n as u32;
    w[MAX_RELATIONSHIP_TYPES * 4 + 1] = version as u32;
    w[MAX_RELATIONSHIP_TYPES * 4 + 2] = b(enabled);
    DynamicImage(w)
}

/// Host-side per-node / per-edge metadata the kernels read. All node arrays have one entry
/// per node, in `graph.nodes` order; all edge arrays one entry per edge whose endpoints both
/// exist, in `graph.edges` order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SemanticInputs {
    pub node_types: Vec<i32>,
    pub hierarchy_levels: Vec<i32>,
    pub physicality: Vec<i32>,
    pub role: Vec<i32>,
    pub maturity: Vec<i32>,
    pub cross_domain: Vec<i32>,
    pub edge_src: Vec<i32>,
    pub edge_dst: Vec<i32>,
    pub edge_type: Vec<i32>,
    pub edge_weight: Vec<f32>,
}

impl SemanticInputs {
    /// Copy the arrays an initialised engine derived from `graph`'s metadata.
    pub fn from_engine(engine: &SemanticForcesEngine, graph: &GraphData) -> Result<Self> {
        let n = graph.nodes.len();
        let fit = |name: &str, v: &[i32]| -> Result<Vec<i32>> {
            if v.len() == n {
                Ok(v.to_vec())
            } else {
                Err(anyhow!("semantic engine produced {} {} entries for {} nodes", v.len(), name, n))
            }
        };
        let index: HashMap<u32, i32> =
            graph.nodes.iter().enumerate().map(|(i, node)| (node.id, i as i32)).collect();
        let types = engine.edge_types();
        let (mut src, mut dst, mut ty, mut wt) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (i, e) in graph.edges.iter().enumerate() {
            let (Some(&s), Some(&t)) = (index.get(&e.source), index.get(&e.target)) else {
                continue; // edge to a node that is not in this graph: nothing to pull on
            };
            src.push(s);
            dst.push(t);
            ty.push(types.get(i).copied().unwrap_or(0));
            wt.push(e.weight);
        }
        Ok(Self {
            node_types: fit("node type", engine.node_types())?,
            hierarchy_levels: fit("hierarchy level", engine.hierarchy_levels())?,
            physicality: fit("physicality", engine.node_physicality())?,
            role: fit("role", engine.node_role())?,
            maturity: fit("maturity", engine.node_maturity())?,
            cross_domain: fit("cross-domain", engine.node_cross_domain_count())?,
            edge_src: src,
            edge_dst: dst,
            edge_type: ty,
            edge_weight: wt,
        })
    }

    pub fn num_nodes(&self) -> usize {
        self.node_types.len()
    }

    pub fn num_edges(&self) -> usize {
        self.edge_src.len()
    }
}

/// One centroid table: per-slot float3 sums and counts.
struct CentroidTable {
    centroids: DeviceBuffer<f32>,
    counts: DeviceBuffer<i32>,
}

impl CentroidTable {
    fn new() -> Result<Self> {
        Ok(Self {
            centroids: DeviceBuffer::from_slice(&vec![0f32; TYPE_SLOTS * 3])?,
            counts: DeviceBuffer::from_slice(&vec![0i32; TYPE_SLOTS])?,
        })
    }
}

/// Device-resident semantic force state for one graph.
pub struct SemanticGpu {
    module: Module,
    n: usize,
    e: usize,
    pos: DeviceBuffer<f32>,
    forces: DeviceBuffer<f32>,
    node_types: DeviceBuffer<i32>,
    levels: DeviceBuffer<i32>,
    physicality: DeviceBuffer<i32>,
    role: DeviceBuffer<i32>,
    maturity: DeviceBuffer<i32>,
    cross_domain: DeviceBuffer<i32>,
    edge_src: DeviceBuffer<i32>,
    edge_dst: DeviceBuffer<i32>,
    edge_type: DeviceBuffer<i32>,
    edge_weight: DeviceBuffer<f32>,
    type_table: CentroidTable,
    phys_table: CentroidTable,
    role_table: CentroidTable,
    dynamic_enabled: bool,
}

fn grid(count: usize) -> u32 {
    (count as u32).div_ceil(BLOCK).max(1)
}

fn upload_i32(v: &[i32]) -> Result<DeviceBuffer<i32>> {
    // cust rejects zero-length allocations; one dummy word keeps empty edge sets valid.
    if v.is_empty() {
        Ok(DeviceBuffer::from_slice(&[0i32])?)
    } else {
        Ok(DeviceBuffer::from_slice(v)?)
    }
}

impl SemanticGpu {
    /// Load the semantic PTX and upload `inputs`. `config` is applied immediately.
    pub fn new(ptx: &str, inputs: &SemanticInputs, config: &SemanticForces) -> Result<Self> {
        let n = inputs.num_nodes();
        let e = inputs.num_edges();
        if n == 0 {
            return Err(anyhow!("semantic forces need at least one node"));
        }
        let module = Module::from_ptx(ptx, &[])?;
        let weights = if inputs.edge_weight.is_empty() { vec![0f32] } else { inputs.edge_weight.clone() };
        let mut me = Self {
            module,
            n,
            e,
            pos: DeviceBuffer::from_slice(&vec![0f32; n * 3])?,
            forces: DeviceBuffer::from_slice(&vec![0f32; n * 3])?,
            node_types: upload_i32(&inputs.node_types)?,
            levels: upload_i32(&inputs.hierarchy_levels)?,
            physicality: upload_i32(&inputs.physicality)?,
            role: upload_i32(&inputs.role)?,
            maturity: upload_i32(&inputs.maturity)?,
            cross_domain: upload_i32(&inputs.cross_domain)?,
            edge_src: upload_i32(&inputs.edge_src)?,
            edge_dst: upload_i32(&inputs.edge_dst)?,
            edge_type: upload_i32(&inputs.edge_type)?,
            edge_weight: DeviceBuffer::from_slice(&weights)?,
            type_table: CentroidTable::new()?,
            phys_table: CentroidTable::new()?,
            role_table: CentroidTable::new()?,
            dynamic_enabled: false,
        };
        me.set_config(config)?;
        Ok(me)
    }

    pub fn num_nodes(&self) -> usize {
        self.n
    }

    /// Write the semantic configuration into this module's constant memory.
    pub fn set_config(&mut self, config: &SemanticForces) -> Result<()> {
        let mut g = self.module.get_global::<ConfigImage>(c"c_semantic_config")?;
        g.copy_from(&pack_config(config))?;
        Ok(())
    }

    /// Write the ontology relationship table (edge type id -> spring parameters).
    pub fn set_relationships(
        &mut self,
        configs: &[RelationshipForceConfig],
        enabled: bool,
        version: i32,
    ) -> Result<()> {
        let mut g = self.module.get_global::<DynamicImage>(c"c_dynamic_relationships")?;
        g.copy_from(&pack_dynamic(configs, enabled, version))?;
        self.dynamic_enabled = enabled;
        Ok(())
    }

    /// Read selected values back through the device's own view of both config structs.
    /// Used by the layout self-check test; cheap enough to call at startup as well.
    pub fn probe(&self, stream: &Stream) -> Result<[f32; 15]> {
        let out = DeviceBuffer::from_slice(&[0f32; 15])?;
        let k = self.module.get_function("sf_config_probe")?;
        // SAFETY: single-thread kernel writing 15 floats into a 15-float buffer.
        unsafe { launch!(k<<<1, 1, 0, stream>>>(out.as_device_ptr()))? };
        stream.synchronize()?;
        let mut host = [0f32; 15];
        out.copy_to(&mut host[..])?;
        Ok(host)
    }

    fn zero_words(&self, stream: &Stream, ptr: DevicePointer<u32>, words: usize) -> Result<()> {
        let k = self.module.get_function("sf_zero_words")?;
        // SAFETY: `ptr` addresses at least `words` u32s owned by this struct.
        unsafe { launch!(k<<<grid(words), BLOCK, 0, stream>>>(ptr, words as i32))? };
        Ok(())
    }

    /// Per-family centroid pass: zero the table, accumulate, finalise.
    fn centroids(&self, stream: &Stream, family: &DeviceBuffer<i32>, table: &CentroidTable) -> Result<()> {
        self.zero_words(stream, table.centroids.as_device_ptr().cast::<u32>(), TYPE_SLOTS * 3)?;
        self.zero_words(stream, table.counts.as_device_ptr().cast::<u32>(), TYPE_SLOTS)?;
        let calc = self.module.get_function("calculate_type_centroids")?;
        let fin = self.module.get_function("finalize_type_centroids")?;
        // SAFETY: family has n entries; positions has 3n floats; tables hold TYPE_SLOTS entries.
        unsafe {
            launch!(calc<<<grid(self.n), BLOCK, 0, stream>>>(
                family.as_device_ptr(),
                self.pos.as_device_ptr(),
                table.centroids.as_device_ptr(),
                table.counts.as_device_ptr(),
                self.n as i32,
                TYPE_SLOTS as i32
            ))?;
            launch!(fin<<<grid(TYPE_SLOTS), BLOCK, 0, stream>>>(
                table.centroids.as_device_ptr(),
                table.counts.as_device_ptr(),
                TYPE_SLOTS as i32
            ))?;
        }
        Ok(())
    }

    /// Compute every enabled semantic force from the live positions and ADD the result,
    /// unscaled, into the live force buffers. Returns after enqueueing on `stream`.
    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        &self,
        stream: &Stream,
        pos_x: DevicePointer<f32>,
        pos_y: DevicePointer<f32>,
        pos_z: DevicePointer<f32>,
        force_x: DevicePointer<f32>,
        force_y: DevicePointer<f32>,
        force_z: DevicePointer<f32>,
    ) -> Result<()> {
        let n = self.n as i32;
        let m = &self.module;
        let (gather, scatter) = (m.get_function("sf_gather_positions")?, m.get_function("sf_scatter_add_forces")?);
        let zero_f = m.get_function("sf_zero_forces")?;
        // SAFETY (whole block): every pointer addresses a live device buffer sized for the
        // launch's node/edge count; kernels only read `pos` and only add into `forces`.
        unsafe {
            launch!(gather<<<grid(self.n), BLOCK, 0, stream>>>(pos_x, pos_y, pos_z, self.pos.as_device_ptr(), n))?;
            launch!(zero_f<<<grid(self.n), BLOCK, 0, stream>>>(self.forces.as_device_ptr(), n))?;
        }
        self.centroids(stream, &self.node_types, &self.type_table)?;
        self.centroids(stream, &self.physicality, &self.phys_table)?;
        self.centroids(stream, &self.role, &self.role_table)?;

        let k_dag = m.get_function("apply_dag_force")?;
        let k_type = m.get_function("apply_type_cluster_force")?;
        let k_collision = m.get_function("apply_collision_force")?;
        let k_phys = m.get_function("apply_physicality_cluster_force")?;
        let k_role = m.get_function("apply_role_cluster_force")?;
        let k_maturity = m.get_function("apply_maturity_layout_force")?;
        let k_spring = m.get_function("apply_attribute_spring_force")?;
        let k_dynamic = m.get_function("apply_dynamic_relationship_force")?;
        let (pos, forces) = (self.pos.as_device_ptr(), self.forces.as_device_ptr());
        unsafe {
            launch!(k_dag<<<grid(self.n), BLOCK, 0, stream>>>(
                self.levels.as_device_ptr(), self.node_types.as_device_ptr(), pos, forces, n))?;
            launch!(k_type<<<grid(self.n), BLOCK, 0, stream>>>(
                self.node_types.as_device_ptr(), self.type_table.centroids.as_device_ptr(), pos, forces, n,
                TYPE_SLOTS as i32))?;
            launch!(k_collision<<<grid(self.n), BLOCK, 0, stream>>>(
                DevicePointer::<f32>::null(), pos, forces, n))?;
            launch!(k_phys<<<grid(self.n), BLOCK, 0, stream>>>(
                self.physicality.as_device_ptr(), self.phys_table.centroids.as_device_ptr(), pos, forces, n))?;
            launch!(k_role<<<grid(self.n), BLOCK, 0, stream>>>(
                self.role.as_device_ptr(), self.role_table.centroids.as_device_ptr(), pos, forces, n))?;
            launch!(k_maturity<<<grid(self.n), BLOCK, 0, stream>>>(
                self.maturity.as_device_ptr(), pos, forces, n))?;
            if self.e > 0 {
                let e = self.e as i32;
                launch!(k_spring<<<grid(self.e), BLOCK, 0, stream>>>(
                    self.edge_src.as_device_ptr(), self.edge_dst.as_device_ptr(),
                    self.edge_weight.as_device_ptr(), self.edge_type.as_device_ptr(), pos, forces, e))?;
                if self.dynamic_enabled {
                    launch!(k_dynamic<<<grid(self.e), BLOCK, 0, stream>>>(
                        self.edge_src.as_device_ptr(), self.edge_dst.as_device_ptr(),
                        self.edge_type.as_device_ptr(), self.cross_domain.as_device_ptr(), pos, forces, e))?;
                }
            }
            launch!(scatter<<<grid(self.n), BLOCK, 0, stream>>>(forces, force_x, force_y, force_z, n))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_image_matches_the_device_struct_size() {
        // 8 sub-structs: (4+1) + (3+1) + (3+1) + (4+1) + (3+1) + (3+1) + (3+1) + (4+1) words.
        assert_eq!(CONFIG_WORDS, 5 + 4 + 4 + 5 + 4 + 4 + 4 + 5);
        assert_eq!(std::mem::size_of::<ConfigImage>(), CONFIG_WORDS * 4);
        assert_eq!(std::mem::size_of::<DynamicImage>(), 4108);
    }

    #[test]
    fn pack_config_places_each_field_at_its_device_offset() {
        let mut c = SemanticForces::default();
        c.dag.vertical_spacing = 11.0;
        c.dag.enabled = true;
        c.type_cluster.cluster_radius = 22.0;
        c.collision.node_radius = 33.0;
        c.attribute_spring.rest_length_max = 44.0;
        c.physicality_cluster.inter_physicality_repulsion = 55.0;
        c.maturity_layout.stage_separation = 66.0;
        c.cross_domain.rest_length = 77.0;
        c.cross_domain.enabled = false;
        let w = pack_config(&c).0;
        assert_eq!(f32::from_bits(w[0]), 11.0, "dag.vertical_spacing");
        assert_eq!(w[4], 1, "dag.enabled");
        assert_eq!(f32::from_bits(w[6]), 22.0, "type_cluster.cluster_radius");
        assert_eq!(f32::from_bits(w[11]), 33.0, "collision.node_radius");
        assert_eq!(f32::from_bits(w[16]), 44.0, "attribute_spring.rest_length_max");
        assert_eq!(f32::from_bits(w[20]), 55.0, "physicality_cluster.inter_physicality_repulsion");
        assert_eq!(f32::from_bits(w[28]), 66.0, "maturity_layout.stage_separation");
        assert_eq!(f32::from_bits(w[33]), 77.0, "cross_domain.rest_length");
        assert_eq!(w[34], 0, "cross_domain.enabled");
    }

    #[test]
    fn dynamic_image_widens_bools_and_ignores_struct_padding() {
        let cfgs = [
            RelationshipForceConfig { strength: 0.5, rest_length: 10.0, is_directional: false, force_type: 0 },
            RelationshipForceConfig { strength: -1.0, rest_length: 99.0, is_directional: true, force_type: 3 },
        ];
        let w = pack_dynamic(&cfgs, true, 7).0;
        assert_eq!(w[2], 0);
        assert_eq!(w[6], 1, "is_directional is a full int on the device, not a byte");
        assert_eq!(f32::from_bits(w[5]), 99.0);
        assert_eq!(w[7], 3);
        assert_eq!(w[1024], 2, "num_types");
        assert_eq!(w[1025], 7, "buffer_version");
        assert_eq!(w[1026], 1, "enabled");
    }

    #[test]
    fn more_than_the_device_capacity_is_truncated_not_overrun() {
        let cfgs = vec![
            RelationshipForceConfig { strength: 1.0, rest_length: 1.0, is_directional: false, force_type: 0 };
            MAX_RELATIONSHIP_TYPES + 40
        ];
        assert_eq!(pack_dynamic(&cfgs, true, 1).0[1024] as usize, MAX_RELATIONSHIP_TYPES);
    }

    // ---- real-device tests (skip, loudly, where there is no CUDA device or PTX) ----

    use cust::stream::StreamFlags;

    fn device() -> Option<(cust::context::Context, Stream, String)> {
        let ctx = match cust::quick_init() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[semantic gpu tests] SKIPPED, CUDA init failed: {e}");
                return None;
            }
        };
        let ptx = match visionclaw_gpu::ptx_loader::load_ptx_module_sync(visionclaw_gpu::ptx_loader::PTXModule::SemanticForces) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[semantic gpu tests] SKIPPED, no semantic PTX: {e}");
                return None;
            }
        };
        let stream = Stream::new(StreamFlags::NON_BLOCKING, None).expect("stream");
        Some((ctx, stream, ptx))
    }

    fn only_type_cluster() -> SemanticForces {
        let mut c = SemanticForces::default();
        c.dag.enabled = false;
        c.collision.enabled = false;
        c.attribute_spring.enabled = false;
        c.physicality_cluster.enabled = false;
        c.role_cluster.enabled = false;
        c.maturity_layout.enabled = false;
        c.cross_domain.enabled = false;
        c.type_cluster.enabled = true;
        c.type_cluster.cluster_attraction = 0.4;
        c.type_cluster.cluster_radius = 150.0;
        c.type_cluster.inter_cluster_repulsion = 0.0;
        c
    }

    fn four_nodes() -> SemanticInputs {
        SemanticInputs {
            node_types: vec![1, 1, 2, 2],
            hierarchy_levels: vec![-1; 4],
            physicality: vec![0; 4],
            role: vec![0; 4],
            maturity: vec![0; 4],
            cross_domain: vec![0; 4],
            edge_src: vec![],
            edge_dst: vec![],
            edge_type: vec![],
            edge_weight: vec![],
        }
    }

    #[test]
    fn the_device_reads_back_exactly_what_the_host_wrote() {
        let Some((_ctx, stream, ptx)) = device() else { return };
        let mut c = SemanticForces::default();
        c.dag.vertical_spacing = 11.0;
        c.dag.enabled = true;
        c.type_cluster.cluster_radius = 22.0;
        c.collision.node_radius = 33.0;
        c.attribute_spring.rest_length_max = 44.0;
        c.physicality_cluster.inter_physicality_repulsion = 55.0;
        c.maturity_layout.stage_separation = 66.0;
        c.cross_domain.rest_length = 77.0;
        c.cross_domain.enabled = true;
        let mut g = SemanticGpu::new(&ptx, &four_nodes(), &c).expect("gpu init");
        let rel = [
            RelationshipForceConfig { strength: 0.1, rest_length: 1.0, is_directional: false, force_type: 0 },
            RelationshipForceConfig { strength: 0.2, rest_length: 123.0, is_directional: true, force_type: 2 },
        ];
        g.set_relationships(&rel, true, 3).expect("relationships");
        let p = g.probe(&stream).expect("probe");
        assert_eq!(p[0], (CONFIG_WORDS * 4) as f32, "device sizeof(SemanticConfig) must equal the host image");
        assert_eq!(&p[1..10], &[11.0, 1.0, 22.0, 33.0, 44.0, 55.0, 66.0, 77.0, 1.0], "config fields at device offsets");
        assert_eq!(p[10], 4108.0, "device sizeof(DynamicRelationshipBuffer)");
        assert_eq!(&p[11..15], &[2.0, 123.0, 2.0, 1.0], "dynamic table: num_types, configs[1].rest_length/force_type, enabled");
    }

    #[test]
    fn type_clustering_pulls_nodes_toward_their_type_centroid_and_adds_to_existing_forces() {
        let Some((_ctx, stream, ptx)) = device() else { return };
        let g = SemanticGpu::new(&ptx, &four_nodes(), &only_type_cluster()).expect("gpu init");
        // Type 1 at x=-500/-1500 (centroid -1000), type 2 at x=+500/+1500 (centroid +1000).
        let x = DeviceBuffer::from_slice(&[-500f32, -1500.0, 500.0, 1500.0]).unwrap();
        let y = DeviceBuffer::from_slice(&[0f32; 4]).unwrap();
        let z = DeviceBuffer::from_slice(&[0f32; 4]).unwrap();
        let run = || -> [f32; 4] {
            let fx = DeviceBuffer::from_slice(&[5f32; 4]).unwrap(); // pre-existing live force
            let fy = DeviceBuffer::from_slice(&[0f32; 4]).unwrap();
            let fz = DeviceBuffer::from_slice(&[0f32; 4]).unwrap();
            g.apply(&stream, x.as_device_ptr(), y.as_device_ptr(), z.as_device_ptr(),
                fx.as_device_ptr(), fy.as_device_ptr(), fz.as_device_ptr()).expect("apply");
            stream.synchronize().unwrap();
            let mut out = [0f32; 4];
            fx.copy_to(&mut out[..]).unwrap();
            out
        };
        let out = run();
        // |dist to centroid| = 500, radius 150, attraction 0.4 -> 0.4 * 350 = 140 toward the centroid,
        // added to the 5.0 already in the live buffer and applied exactly as stored (no scale).
        let expect = [5.0 - 140.0, 5.0 + 140.0, 5.0 + 140.0, 5.0 - 140.0];
        for i in 0..4 {
            assert!((out[i] - expect[i]).abs() < 0.5, "node {i}: got {} want {}", out[i], expect[i]);
        }
        // Applying twice adds the semantic force twice (the buffer is not reset by apply).
        let again = {
            let fx = DeviceBuffer::from_slice(&out).unwrap();
            let fy = DeviceBuffer::from_slice(&[0f32; 4]).unwrap();
            let fz = DeviceBuffer::from_slice(&[0f32; 4]).unwrap();
            g.apply(&stream, x.as_device_ptr(), y.as_device_ptr(), z.as_device_ptr(),
                fx.as_device_ptr(), fy.as_device_ptr(), fz.as_device_ptr()).expect("apply");
            stream.synchronize().unwrap();
            let mut r = [0f32; 4];
            fx.copy_to(&mut r[..]).unwrap();
            r
        };
        for i in 0..4 {
            assert!((again[i] - (out[i] + (out[i] - 5.0))).abs() < 0.5, "node {i}: forces must accumulate additively");
        }
    }

    /// The whole chain, for real: a settings document as a client would PUT it and the store
    /// would hold it -> parsed -> converted to the actor's parameters -> validated -> uploaded
    /// -> read back through the DEVICE's view of its struct. Nothing in between scales, clamps
    /// or substitutes a default.
    #[test]
    fn the_stored_settings_document_is_what_the_device_holds() {
        use visionclaw_domain::models::simulation_params::SimulationParams;
        use visionclaw_domain::types::PhysicsSettings;
        let Some((_ctx, stream, ptx)) = device() else { return };

        let mut doc = serde_json::to_value(PhysicsSettings::default()).unwrap();
        doc["semantic"]["dag"]["verticalSpacing"] = serde_json::json!(321.0);
        doc["semantic"]["dag"]["enabled"] = serde_json::json!(false);
        doc["semantic"]["typeCluster"]["clusterRadius"] = serde_json::json!(222.5);
        doc["semantic"]["collision"]["nodeRadius"] = serde_json::json!(33.25);
        doc["semantic"]["attributeSpring"]["restLengthMax"] = serde_json::json!(444.0);
        doc["semantic"]["physicalityCluster"]["interPhysicalityRepulsion"] = serde_json::json!(5.5);
        doc["semantic"]["maturityLayout"]["stageSeparation"] = serde_json::json!(66.0);
        doc["semantic"]["crossDomain"]["restLength"] = serde_json::json!(77.0);
        doc["semantic"]["crossDomain"]["enabled"] = serde_json::json!(true);

        let stored: PhysicsSettings = serde_json::from_value(doc).expect("the stored document parses");
        let sim: SimulationParams = (&stored).into();
        sim.validate().expect("the actor's validation accepts it");

        let g = SemanticGpu::new(&ptx, &four_nodes(), &sim.semantic).expect("upload");
        let p = g.probe(&stream).expect("probe");
        assert_eq!(&p[1..10], &[321.0, 0.0, 222.5, 33.25, 444.0, 5.5, 66.0, 77.0, 1.0], "device == stored document");
    }

    #[test]
    fn a_settings_update_that_changes_one_force_changes_only_that_force_on_the_device() {
        let Some((_ctx, stream, ptx)) = device() else { return };
        let mut cfg = SemanticForces::default();
        let mut g = SemanticGpu::new(&ptx, &four_nodes(), &cfg).expect("upload");
        let before = g.probe(&stream).expect("probe");
        cfg.type_cluster.cluster_radius = 999.0;
        g.set_config(&cfg).expect("set_config");
        let after = g.probe(&stream).expect("probe");
        assert_eq!(after[3], 999.0);
        for i in (1..10).filter(|&i| i != 3) {
            assert_eq!(after[i], before[i], "field {i} must not move");
        }
    }

    #[test]
    fn a_disabled_force_adds_nothing() {
        let Some((_ctx, stream, ptx)) = device() else { return };
        let mut c = only_type_cluster();
        c.type_cluster.enabled = false;
        let g = SemanticGpu::new(&ptx, &four_nodes(), &c).expect("gpu init");
        let x = DeviceBuffer::from_slice(&[-500f32, -1500.0, 500.0, 1500.0]).unwrap();
        let zero = DeviceBuffer::from_slice(&[0f32; 4]).unwrap();
        let fx = DeviceBuffer::from_slice(&[7f32; 4]).unwrap();
        let fy = DeviceBuffer::from_slice(&[0f32; 4]).unwrap();
        let fz = DeviceBuffer::from_slice(&[0f32; 4]).unwrap();
        g.apply(&stream, x.as_device_ptr(), zero.as_device_ptr(), zero.as_device_ptr(),
            fx.as_device_ptr(), fy.as_device_ptr(), fz.as_device_ptr()).expect("apply");
        stream.synchronize().unwrap();
        let mut out = [0f32; 4];
        fx.copy_to(&mut out[..]).unwrap();
        assert_eq!(out, [7.0; 4]);
    }
}
