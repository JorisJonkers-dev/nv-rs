//! THROWAWAY: a raw Vulkan (`ash`) prototype of the draw, state and bind path, to measure what a
//! draw costs when nothing sits between the game's commands and the driver.
//!
//! It is a measurement tool and not a backend: it will never draw a correct picture. It carries
//! out the commands of a recorded stream the way a lean native backend would and nothing more:
//!
//! - Vulkan 1.3 with dynamic rendering and synchronisation 2, push descriptors for the textures,
//!   dynamic uniform buffer offsets into one persistently mapped ring for the registers, buffers
//!   in host-visible device memory written in place, one command buffer a frame on one thread;
//! - the game's own shaders (the converter's WGSL, naga to SPIR-V), a pipeline for each
//!   combination of shaders, declaration, strides, targets and render states, made the first
//!   time it is drawn with and kept;
//! - state shadowing: the commands only store; a draw binds what changed since the last one;
//! - textures and targets are real images of the right size and format, but their contents are
//!   never uploaded, and `UploadSurface`, queries, `StretchRect` and `ColorFill` are not carried
//!   out; multisampling, the user clip planes, the alpha test and fixed-function draws (no
//!   shaders) are left out.
//!
//! So the GPU time it reports is the work of the same draws without those; the CPU time is the
//! cost of the hot path. Each part of the hot path is the same as the wgpu backend's where there
//! is one (the register blocks, the sampler keys, the pipeline's state from `d3d9_wgpu::state`),
//! so the two are given the same work.

// A throwaway measurement tool: fields kept for the next change to it, and a few long signatures.
#![allow(
    dead_code,
    unused_assignments,
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::clone_on_copy
)]

use crate::replay::Target;
use ash::{khr, vk};
use d3d9_stream::{Backend, Cmd, Data, Handle, Reply};
use d3d9_wgpu::format::{self, Format};
use d3d9_wgpu::sm::{self, wgsl};
use d3d9_wgpu::state::{self, rs, ss};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// A fast hasher for the pipeline and sampler keys (FxHash's multiply and rotate).
#[derive(Default)]
pub struct Fx(u64);

impl Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for c in bytes.chunks(8) {
            let mut w = [0u8; 8];
            w[..c.len()].copy_from_slice(c);
            self.write_u64(u64::from_le_bytes(w));
        }
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    fn write_u32(&mut self, v: u32) {
        self.write_u64(u64::from(v));
    }
    fn write_u8(&mut self, v: u8) {
        self.write_u64(u64::from(v));
    }
    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }
}

type Map<K, V> = HashMap<K, V, BuildHasherDefault<Fx>>;

/// Size of each memory block carved up for resources.
const BLOCK: u64 = 256 << 20;
/// The registers' ring, for each frame in flight, and the frames in flight.
const RING: u64 = 24 << 20;
const FRAMES: usize = 2;
/// A stage's register header and its float constants, as the converted shaders read them.
const HEADER: usize = wgsl::REGS_C_OFFSET as usize;
const SLOTS: usize = 20;

/// The render states a pipeline depends on.
const PIPE_STATES: [usize; 31] = [
    rs::ZENABLE,
    rs::ZWRITEENABLE,
    rs::ZFUNC,
    rs::CULLMODE,
    rs::ALPHABLENDENABLE,
    rs::SRCBLEND,
    rs::DESTBLEND,
    rs::BLENDOP,
    rs::SEPARATEALPHABLENDENABLE,
    rs::SRCBLENDALPHA,
    rs::DESTBLENDALPHA,
    rs::BLENDOPALPHA,
    rs::COLORWRITEENABLE,
    rs::COLORWRITEENABLE1,
    rs::COLORWRITEENABLE2,
    rs::COLORWRITEENABLE3,
    rs::STENCILENABLE,
    rs::STENCILFAIL,
    rs::STENCILZFAIL,
    rs::STENCILPASS,
    rs::STENCILFUNC,
    rs::STENCILMASK,
    rs::STENCILWRITEMASK,
    rs::TWOSIDEDSTENCILMODE,
    rs::CCW_STENCILFAIL,
    rs::CCW_STENCILZFAIL,
    rs::CCW_STENCILPASS,
    rs::CCW_STENCILFUNC,
    rs::DEPTHBIAS,
    rs::SLOPESCALEDEPTHBIAS,
    rs::FILLMODE,
];

#[derive(Clone, PartialEq, Eq, Hash)]
struct PipeKey {
    vs: u32,
    ps: u32,
    decl: u32,
    strides: [u32; 8],
    topology: u8,
    colours: [i32; 4],
    depth: i32,
    states: [u32; PIPE_STATES.len()],
}

struct Image {
    image: vk::Image,
    /// The view a shader samples (2D or cube, every level).
    view: vk::ImageView,
    dim: wgsl::Dim,
    format: vk::Format,
    width: u32,
    height: u32,
    aspect: vk::ImageAspectFlags,
    /// How sampling fills the format's missing components (the register header's `fix`).
    fix: u32,
    /// Rendered to: kept in the general layout.
    target: bool,
    depth: bool,
    opaque: bool,
    levels: u32,
}

struct Buf {
    buffer: vk::Buffer,
    ptr: *mut u8,
    size: u32,
    index32: bool,
}

struct Shader {
    program: sm::Program,
    sig: wgsl::Signature,
    id: u32,
    fregs: u32,
    varyings: Vec<wgsl::Semantic>,
    fixed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Element {
    stream: u16,
    offset: u16,
    ty: u8,
    usage: u8,
    index: u8,
}

struct Decl {
    elements: Vec<Element>,
    id: u32,
}

struct VertexLink {
    inputs: Vec<wgsl::Input>,
    /// By stream: format, offset, shader location.
    attributes: [Vec<(vk::Format, u32, u32)>; 8],
}

#[derive(Clone, Copy)]
enum Obj {
    Image(usize),
    /// A level of a texture: the image and the level.
    Level(usize, u32),
    Buffer(usize),
    Shader(usize),
    Decl(usize),
}

/// A texture bound to a sampler slot: what the descriptor needs.
#[derive(Clone, Copy)]
struct Bound {
    view: vk::ImageView,
    dim: wgsl::Dim,
    fix: u32,
}

struct Pipeline {
    pipeline: vk::Pipeline,
    ps_samplers: Vec<(u32, wgsl::Dim)>,
    vs_samplers: Vec<(u32, wgsl::Dim)>,
}

struct FrameSlot {
    pool: vk::CommandPool,
    cb: vk::CommandBuffer,
    fence: vk::Fence,
    /// Whether a submit of this slot's command buffer may still run.
    submitted: bool,
}

struct Block {
    memory: vk::DeviceMemory,
    used: u64,
    ptr: *mut u8,
    type_index: u32,
}

/// What a run did that is not a cost: what was left out.
#[derive(Default, Debug, Clone, Copy)]
pub struct Counts {
    pub draws: u64,
    pub skipped: u64,
    pub pipelines: u64,
    pub modules: u64,
    pub bad_shaders: u64,
    pub pushes: u64,
    pub binds: u64,
    pub passes: u64,
    pub ring_wraps: u64,
}

/// The prototype as a target.
pub struct VulkanTarget {
    // Vulkan.
    _entry: ash::Entry,
    instance: ash::Instance,
    device: ash::Device,
    queue: vk::Queue,
    family: u32,
    push: khr::push_descriptor::Device,
    mem_props: vk::PhysicalDeviceMemoryProperties,
    timestamp_period: f32,
    gpu_name: String,
    // Objects.
    objects: Map<Handle, Obj>,
    images: Vec<Image>,
    buffers: Vec<Buf>,
    shaders: Vec<Shader>,
    decls: Vec<Decl>,
    attach_views: Map<(usize, u32), vk::ImageView>,
    blocks: Vec<Block>,
    next_id: u32,
    // Layouts and the things every draw uses.
    set0_layout: vk::DescriptorSetLayout,
    set1_layout: vk::DescriptorSetLayout,
    set2_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pool: vk::DescriptorPool,
    set0: vk::DescriptorSet,
    set2: vk::DescriptorSet,
    dummies: [(vk::ImageView, vk::Image); 3],
    dummy_sampler: vk::Sampler,
    samplers: Map<state::SamplerKey, vk::Sampler>,
    pipeline_cache: vk::PipelineCache,
    // Frames.
    slots: Vec<FrameSlot>,
    slot: usize,
    open: bool,
    ring: vk::Buffer,
    ring_ptr: *mut u8,
    ring_at: u64,
    ring_base: u64,
    query_pool: vk::QueryPool,
    queries: u32,
    frames_begun: u32,
    init_pool: vk::CommandPool,
    pending: Vec<(vk::Image, vk::ImageAspectFlags, u32, u32, vk::ImageLayout)>,
    // State.
    st: State,
    // Cache.
    pipelines: Map<PipeKey, Option<Pipeline>>,
    vs_modules: Map<(u32, Vec<wgsl::Input>, Vec<wgsl::Semantic>), Option<vk::ShaderModule>>,
    ps_modules: Map<(u32, Vec<wgsl::Semantic>), Option<vk::ShaderModule>>,
    links: Map<(u32, u32), std::sync::Arc<VertexLink>>,
    // Recording state.
    pass: Option<([Option<(usize, u32)>; 4], Option<(usize, u32)>)>,
    cur_pipeline: Option<PipeKey>,
    bound_pipeline: vk::Pipeline,
    pub counts: Counts,
    gpu_gate: bool,
}

struct State {
    rs: Box<[u32; 256]>,
    samplers: [[u32; 14]; SLOTS],
    textures: [Option<Bound>; SLOTS],
    vs: Handle,
    ps: Handle,
    decl: Handle,
    /// Buffer handle, offset, stride.
    streams: [(Handle, u32, u32); 8],
    indices: Handle,
    rt: [Handle; 4],
    ds: Handle,
    viewport: [u32; 6],
    scissor: [i32; 4],
    vsf: Box<[u8; 4096]>,
    psf: Box<[u8; 4096]>,
    vsi: [[u32; 4]; 16],
    vsb: [u32; 16],
    psi: [[u32; 4]; 16],
    psb: [u32; 16],
    // What changed since the last draw.
    pipe_dirty: bool,
    header_dirty: [bool; 2],
    consts_dirty: [bool; 2],
    tex_dirty: bool,
    vertex_dirty: u8,
    index_dirty: bool,
    viewport_dirty: bool,
    // What is bound now.
    fregs: [u32; 2],
    offsets: [u32; 4],
    centre: [f32; 2],
    stencil_ref: Option<u32>,
    index_bound: Option<(Handle, bool)>,
    last_stencil: u32,
}

impl State {
    fn new() -> State {
        State {
            rs: state::default_render_states(false),
            samplers: [state::default_sampler_states(); SLOTS],
            textures: [None; SLOTS],
            vs: 0,
            ps: 0,
            decl: 0,
            streams: [(0, 0, 0); 8],
            indices: 0,
            rt: [0; 4],
            ds: 0,
            viewport: [0, 0, 1, 1, 0, 1.0f32.to_bits()],
            scissor: [0; 4],
            vsf: Box::new([0; 4096]),
            psf: Box::new([0; 4096]),
            vsi: [[0; 4]; 16],
            vsb: [0; 16],
            psi: [[0; 4]; 16],
            psb: [0; 16],
            pipe_dirty: true,
            header_dirty: [true; 2],
            consts_dirty: [true; 2],
            tex_dirty: true,
            vertex_dirty: 0xFF,
            index_dirty: true,
            viewport_dirty: true,
            fregs: [0; 2],
            offsets: [0; 4],
            centre: [0.0; 2],
            stencil_ref: None,
            index_bound: None,
            last_stencil: u32::MAX,
        }
    }
}

fn vk_format(f: wgpu::TextureFormat) -> vk::Format {
    use wgpu::TextureFormat as T;
    match f {
        T::Bgra8Unorm => vk::Format::B8G8R8A8_UNORM,
        T::Rgba8Unorm => vk::Format::R8G8B8A8_UNORM,
        T::Rgba8Snorm => vk::Format::R8G8B8A8_SNORM,
        T::R8Unorm => vk::Format::R8_UNORM,
        T::Rg8Unorm => vk::Format::R8G8_UNORM,
        T::Rg8Snorm => vk::Format::R8G8_SNORM,
        T::R16Unorm => vk::Format::R16_UNORM,
        T::Rg16Unorm => vk::Format::R16G16_UNORM,
        T::Rgba16Unorm => vk::Format::R16G16B16A16_UNORM,
        T::R16Float => vk::Format::R16_SFLOAT,
        T::Rg16Float => vk::Format::R16G16_SFLOAT,
        T::Rgba16Float => vk::Format::R16G16B16A16_SFLOAT,
        T::R32Float => vk::Format::R32_SFLOAT,
        T::Rg32Float => vk::Format::R32G32_SFLOAT,
        T::Rgba32Float => vk::Format::R32G32B32A32_SFLOAT,
        T::Rgb10a2Unorm => vk::Format::A2B10G10R10_UNORM_PACK32,
        T::Bc1RgbaUnorm => vk::Format::BC1_RGBA_UNORM_BLOCK,
        T::Bc2RgbaUnorm => vk::Format::BC2_UNORM_BLOCK,
        T::Bc3RgbaUnorm => vk::Format::BC3_UNORM_BLOCK,
        T::Bc4RUnorm => vk::Format::BC4_UNORM_BLOCK,
        T::Bc5RgUnorm => vk::Format::BC5_UNORM_BLOCK,
        T::Depth16Unorm => vk::Format::D16_UNORM,
        T::Depth24Plus | T::Depth32Float => vk::Format::D32_SFLOAT,
        T::Depth24PlusStencil8 => vk::Format::D32_SFLOAT_S8_UINT,
        _ => vk::Format::R8G8B8A8_UNORM,
    }
}

fn has_stencil(f: vk::Format) -> bool {
    f == vk::Format::D32_SFLOAT_S8_UINT || f == vk::Format::D24_UNORM_S8_UINT
}

fn input_of(ty: u8) -> Option<(vk::Format, wgsl::Input)> {
    use wgsl::Input as I;
    Some(match ty {
        0 => (vk::Format::R32_SFLOAT, I::Float(4)),
        1 => (vk::Format::R32G32_SFLOAT, I::Float(4)),
        2 => (vk::Format::R32G32B32_SFLOAT, I::Float(4)),
        3 => (vk::Format::R32G32B32A32_SFLOAT, I::Float(4)),
        4 => (vk::Format::B8G8R8A8_UNORM, I::Float(4)),
        5 => (vk::Format::R8G8B8A8_UINT, I::Uint4),
        6 => (vk::Format::R16G16_SINT, I::Sint(4)),
        7 => (vk::Format::R16G16B16A16_SINT, I::Sint(4)),
        8 => (vk::Format::R8G8B8A8_UNORM, I::Float(4)),
        9 => (vk::Format::R16G16_SNORM, I::Float(4)),
        10 => (vk::Format::R16G16B16A16_SNORM, I::Float(4)),
        11 => (vk::Format::R16G16_UNORM, I::Float(4)),
        12 => (vk::Format::R16G16B16A16_UNORM, I::Float(4)),
        15 => (vk::Format::R16G16_SFLOAT, I::Float(4)),
        16 => (vk::Format::R16G16B16A16_SFLOAT, I::Float(4)),
        _ => return None,
    })
}

fn topology(kind: u32) -> Option<vk::PrimitiveTopology> {
    Some(match kind {
        1 => vk::PrimitiveTopology::POINT_LIST,
        2 => vk::PrimitiveTopology::LINE_LIST,
        3 => vk::PrimitiveTopology::LINE_STRIP,
        4 => vk::PrimitiveTopology::TRIANGLE_LIST,
        5 => vk::PrimitiveTopology::TRIANGLE_STRIP,
        _ => return None,
    })
}

fn vertex_count(kind: u32, count: u32) -> u32 {
    match kind {
        1 => count,
        2 => count * 2,
        3 => count + 1,
        4 => count * 3,
        5 | 6 => count + 2,
        _ => 0,
    }
}

fn own_varyings(vs: &Shader) -> Vec<wgsl::Semantic> {
    let mut v: Vec<wgsl::Semantic> = vs
        .sig
        .outputs
        .iter()
        .map(|(_, s)| *s)
        .filter(|s| wgsl::fixed_slot(*s).is_some())
        .collect();
    if vs.program.major < 3 {
        v.extend((0..2).map(|index| wgsl::Semantic {
            usage: sm::usage::COLOR,
            index,
        }));
    }
    v.sort_by_key(|s| wgsl::fixed_slot(*s));
    v.dedup();
    v
}

fn pair_varyings(vs: &Shader, ps: &Shader) -> Vec<wgsl::Semantic> {
    let mut v = own_varyings(vs);
    for s in &ps.varyings {
        if !v.contains(s) {
            v.push(*s);
        }
    }
    v.sort_by_key(|s| wgsl::fixed_slot(*s));
    v.dedup();
    v
}

fn words(d: &Data<'_>) -> Vec<u32> {
    d.to_words().into_owned()
}

/// The register header (everything before the float constants) of a stage, as the shaders read it.
fn header(
    i: &[[u32; 4]; 16],
    b: &[u32; 16],
    sys: [f32; 4],
    fix: [u32; 16],
    bias: [f32; 16],
    out: &mut [u8],
) {
    let w = i
        .iter()
        .flatten()
        .copied()
        .chain(b.iter().copied())
        .chain(sys.iter().map(|v| v.to_bits()))
        .chain([0u32; 12])
        .chain(fix)
        .chain(bias.iter().map(|v| v.to_bits()))
        .chain([0u32; 24]);
    for (d, w) in out[..HEADER].chunks_exact_mut(4).zip(w) {
        d.copy_from_slice(&w.to_le_bytes());
    }
}

macro_rules! vk_try {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(e) => return Err(format!("{}: {e:?}", stringify!($e))),
        }
    };
}

impl VulkanTarget {
    /// A device on the first hardware adapter (`NV_REPLAY_VK_DEVICE` names a substring of the one
    /// wanted, `llvmpipe` for the software one).
    pub fn new(gpu_frames: Option<usize>) -> Result<VulkanTarget, String> {
        // SAFETY: loading the system's Vulkan library.
        let entry = unsafe { ash::Entry::load() }.map_err(|e| format!("no Vulkan loader: {e}"))?;
        let app = vk::ApplicationInfo::default()
            .application_name(c"d3d9-replay")
            .api_version(vk::API_VERSION_1_3);
        // SAFETY: plain creation with valid structures.
        let instance = vk_try!(unsafe {
            entry.create_instance(
                &vk::InstanceCreateInfo::default().application_info(&app),
                None,
            )
        });
        let phys_all = vk_try!(unsafe { instance.enumerate_physical_devices() });
        let want = std::env::var("NV_REPLAY_VK_DEVICE").ok();
        let mut chosen = None;
        for &p in &phys_all {
            let props = unsafe { instance.get_physical_device_properties(p) };
            let name = props
                .device_name_as_c_str()
                .map(|c| c.to_string_lossy().into_owned())
                .unwrap_or_default();
            let ok = match &want {
                Some(w) => name.to_lowercase().contains(&w.to_lowercase()),
                None => props.device_type != vk::PhysicalDeviceType::CPU,
            };
            if ok && chosen.is_none() {
                chosen = Some((p, name, props));
            }
        }
        // Without a hardware adapter (CI), the software one.
        if chosen.is_none() && want.is_none() {
            if let Some(&p) = phys_all.first() {
                let props = unsafe { instance.get_physical_device_properties(p) };
                let name = props
                    .device_name_as_c_str()
                    .map(|c| c.to_string_lossy().into_owned())
                    .unwrap_or_default();
                chosen = Some((p, name, props));
            }
        }
        let Some((phys, gpu_name, props)) = chosen else {
            return Err("no Vulkan adapter".into());
        };
        let family = unsafe { instance.get_physical_device_queue_family_properties(phys) }
            .iter()
            .position(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .ok_or("no graphics queue")? as u32;
        let supported = unsafe { instance.get_physical_device_features(phys) };
        let features = vk::PhysicalDeviceFeatures::default()
            .sampler_anisotropy(supported.sampler_anisotropy != 0)
            .texture_compression_bc(supported.texture_compression_bc != 0)
            .independent_blend(supported.independent_blend != 0)
            .shader_clip_distance(supported.shader_clip_distance != 0)
            .depth_clamp(supported.depth_clamp != 0)
            .fill_mode_non_solid(supported.fill_mode_non_solid != 0);
        let mut v13 = vk::PhysicalDeviceVulkan13Features::default()
            .dynamic_rendering(true)
            .synchronization2(true);
        let mut v12 = vk::PhysicalDeviceVulkan12Features::default().host_query_reset(true);
        let prio = [1.0f32];
        let queue_info = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(family)
            .queue_priorities(&prio)];
        let exts = [khr::push_descriptor::NAME.as_ptr()];
        let info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_info)
            .enabled_extension_names(&exts)
            .enabled_features(&features)
            .push_next(&mut v12)
            .push_next(&mut v13);
        let device = vk_try!(unsafe { instance.create_device(phys, &info, None) });
        let queue = unsafe { device.get_device_queue(family, 0) };
        let push = khr::push_descriptor::Device::new(&instance, &device);
        let mem_props = unsafe { instance.get_physical_device_memory_properties(phys) };

        let mut t = VulkanTarget {
            _entry: entry,
            instance,
            device,
            queue,
            family,
            push,
            mem_props,
            timestamp_period: props.limits.timestamp_period,
            gpu_name,
            objects: Map::default(),
            images: Vec::new(),
            buffers: Vec::new(),
            shaders: Vec::new(),
            decls: Vec::new(),
            attach_views: Map::default(),
            blocks: Vec::new(),
            next_id: 0,
            set0_layout: vk::DescriptorSetLayout::null(),
            set1_layout: vk::DescriptorSetLayout::null(),
            set2_layout: vk::DescriptorSetLayout::null(),
            layout: vk::PipelineLayout::null(),
            pool: vk::DescriptorPool::null(),
            set0: vk::DescriptorSet::null(),
            set2: vk::DescriptorSet::null(),
            dummies: [(vk::ImageView::null(), vk::Image::null()); 3],
            dummy_sampler: vk::Sampler::null(),
            samplers: Map::default(),
            pipeline_cache: vk::PipelineCache::null(),
            slots: Vec::new(),
            slot: 0,
            open: false,
            ring: vk::Buffer::null(),
            ring_ptr: std::ptr::null_mut(),
            ring_at: 0,
            ring_base: 0,
            query_pool: vk::QueryPool::null(),
            queries: 0,
            frames_begun: 0,
            init_pool: vk::CommandPool::null(),
            pending: Vec::new(),
            st: State::new(),
            pipelines: Map::default(),
            vs_modules: Map::default(),
            ps_modules: Map::default(),
            links: Map::default(),
            pass: None,
            cur_pipeline: None,
            bound_pipeline: vk::Pipeline::null(),
            counts: Counts::default(),
            gpu_gate: gpu_frames.is_some() && props.limits.timestamp_compute_and_graphics != 0,
        };
        t.init(gpu_frames.unwrap_or(0))?;
        Ok(t)
    }

    fn init(&mut self, gpu_frames: usize) -> Result<(), String> {
        let d = &self.device;
        // Set 0: the four register blocks, dynamic offsets. Set 1: the pixel shader's textures and
        // samplers (pushed). Set 2: the vertex shader's (a fixed dummy: vertex textures are not
        // replayed).
        let all = vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT;
        let b0: Vec<_> = (0..4)
            .map(|b| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(b)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
                    .descriptor_count(1)
                    .stage_flags(all)
            })
            .collect();
        let tex = |n: u32, stage| -> Vec<vk::DescriptorSetLayoutBinding<'static>> {
            (0..2 * n)
                .map(|b| {
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(b)
                        .descriptor_type(if b % 2 == 0 {
                            vk::DescriptorType::SAMPLED_IMAGE
                        } else {
                            vk::DescriptorType::SAMPLER
                        })
                        .descriptor_count(1)
                        .stage_flags(stage)
                })
                .collect()
        };
        let b1 = tex(16, vk::ShaderStageFlags::FRAGMENT);
        let b2 = tex(4, vk::ShaderStageFlags::VERTEX);
        // SAFETY (the block): creation calls with valid structures; handles kept in `self`.
        unsafe {
            self.set0_layout = vk_try!(d.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&b0),
                None
            ));
            self.set1_layout = vk_try!(d.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default()
                    .bindings(&b1)
                    .flags(vk::DescriptorSetLayoutCreateFlags::PUSH_DESCRIPTOR_KHR),
                None
            ));
            self.set2_layout = vk_try!(d.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&b2),
                None
            ));
            let layouts = [self.set0_layout, self.set1_layout, self.set2_layout];
            self.layout = vk_try!(d.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts),
                None
            ));
            let sizes = [
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
                    .descriptor_count(4),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(4),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::SAMPLER)
                    .descriptor_count(4),
            ];
            self.pool = vk_try!(d.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(2)
                    .pool_sizes(&sizes),
                None
            ));
            let sets = vk_try!(d.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(self.pool)
                    .set_layouts(&[self.set0_layout, self.set2_layout]),
            ));
            (self.set0, self.set2) = (sets[0], sets[1]);
            self.pipeline_cache =
                vk_try!(d.create_pipeline_cache(&vk::PipelineCacheCreateInfo::default(), None));
            self.dummy_sampler = vk_try!(d.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::NEAREST)
                    .min_filter(vk::Filter::NEAREST),
                None
            ));
        }
        // The ring of registers and vertices (a slice for each frame in flight).
        let (ring, ptr, _) = self.buffer_memory(
            RING * FRAMES as u64 + 8192,
            vk::BufferUsageFlags::UNIFORM_BUFFER
                | vk::BufferUsageFlags::VERTEX_BUFFER
                | vk::BufferUsageFlags::INDEX_BUFFER,
        )?;
        (self.ring, self.ring_ptr) = (ring, ptr);
        let d = &self.device;
        let ranges = [
            (0u32, HEADER as u64),
            (1, HEADER as u64),
            (2, wgsl::CONSTS_SIZE),
            (3, wgsl::CONSTS_SIZE),
        ];
        let infos: Vec<[vk::DescriptorBufferInfo; 1]> = ranges
            .iter()
            .map(|&(_, r)| {
                [vk::DescriptorBufferInfo::default()
                    .buffer(self.ring)
                    .offset(0)
                    .range(r)]
            })
            .collect();
        let writes: Vec<_> = ranges
            .iter()
            .zip(&infos)
            .map(|(&(b, _), i)| {
                vk::WriteDescriptorSet::default()
                    .dst_set(self.set0)
                    .dst_binding(b)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
                    .buffer_info(i)
            })
            .collect();
        unsafe { d.update_descriptor_sets(&writes, &[]) };
        // Frames in flight, and the one-time pool.
        for _ in 0..FRAMES {
            unsafe {
                let pool = vk_try!(d.create_command_pool(
                    &vk::CommandPoolCreateInfo::default()
                        .queue_family_index(self.family)
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                    None
                ));
                let cb = vk_try!(d.allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1)
                ))[0];
                let fence = vk_try!(d.create_fence(&vk::FenceCreateInfo::default(), None));
                self.slots.push(FrameSlot {
                    pool,
                    cb,
                    fence,
                    submitted: false,
                });
            }
        }
        unsafe {
            self.init_pool = vk_try!(d.create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(self.family),
                None
            ));
        }
        if self.gpu_gate && gpu_frames > 0 {
            self.queries = (gpu_frames * 2) as u32;
            unsafe {
                self.query_pool = vk_try!(d.create_query_pool(
                    &vk::QueryPoolCreateInfo::default()
                        .query_type(vk::QueryType::TIMESTAMP)
                        .query_count(self.queries),
                    None
                ));
                d.reset_query_pool(self.query_pool, 0, self.queries);
            }
        }
        // Dummies for a sampler whose texture is of another kind or missing: 2D, cube, volume.
        for (k, dim) in [wgsl::Dim::D2, wgsl::Dim::Cube, wgsl::Dim::D3]
            .into_iter()
            .enumerate()
        {
            let (view, image) = self.make_dummy(dim)?;
            self.dummies[k] = (view, image);
        }
        let dummy_view = self.dummies[0].0;
        let (di, ds) = (
            [vk::DescriptorImageInfo::default()
                .image_view(dummy_view)
                .image_layout(vk::ImageLayout::GENERAL)],
            [vk::DescriptorImageInfo::default().sampler(self.dummy_sampler)],
        );
        let writes: Vec<_> = (0..4u32)
            .flat_map(|n| {
                [
                    vk::WriteDescriptorSet::default()
                        .dst_set(self.set2)
                        .dst_binding(2 * n)
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .image_info(&di),
                    vk::WriteDescriptorSet::default()
                        .dst_set(self.set2)
                        .dst_binding(2 * n + 1)
                        .descriptor_type(vk::DescriptorType::SAMPLER)
                        .image_info(&ds),
                ]
            })
            .collect();
        unsafe { self.device.update_descriptor_sets(&writes, &[]) };
        Ok(())
    }

    fn make_dummy(&mut self, dim: wgsl::Dim) -> Result<(vk::ImageView, vk::Image), String> {
        let (ty, view_ty, layers, flags, depth) = match dim {
            wgsl::Dim::D2 => (
                vk::ImageType::TYPE_2D,
                vk::ImageViewType::TYPE_2D,
                1,
                vk::ImageCreateFlags::empty(),
                1,
            ),
            wgsl::Dim::Cube => (
                vk::ImageType::TYPE_2D,
                vk::ImageViewType::CUBE,
                6,
                vk::ImageCreateFlags::CUBE_COMPATIBLE,
                1,
            ),
            wgsl::Dim::D3 => (
                vk::ImageType::TYPE_3D,
                vk::ImageViewType::TYPE_3D,
                1,
                vk::ImageCreateFlags::empty(),
                1,
            ),
        };
        let info = vk::ImageCreateInfo::default()
            .flags(flags)
            .image_type(ty)
            .format(vk::Format::R8G8B8A8_UNORM)
            .extent(vk::Extent3D {
                width: 1,
                height: 1,
                depth,
            })
            .mip_levels(1)
            .array_layers(layers)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::SAMPLED)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let image = vk_try!(unsafe { self.device.create_image(&info, None) });
        let req = unsafe { self.device.get_image_memory_requirements(image) };
        let (mem, off, _) = self.alloc(req, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
        vk_try!(unsafe { self.device.bind_image_memory(image, mem, off) });
        let view = vk_try!(unsafe {
            self.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(view_ty)
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: layers,
                    }),
                None,
            )
        });
        self.pending.push((
            image,
            vk::ImageAspectFlags::COLOR,
            1,
            layers,
            vk::ImageLayout::GENERAL,
        ));
        Ok((view, image))
    }

    /// Device memory for `req` with `want` properties (host visible ones mapped), carved from
    /// big blocks and never given back: a run makes its resources once.
    fn alloc(
        &mut self,
        req: vk::MemoryRequirements,
        want: vk::MemoryPropertyFlags,
    ) -> Result<(vk::DeviceMemory, u64, *mut u8), String> {
        let mut type_index = None;
        for i in 0..self.mem_props.memory_type_count {
            let t = self.mem_props.memory_types[i as usize];
            if req.memory_type_bits & (1 << i) != 0 && t.property_flags.contains(want) {
                type_index = Some(i);
                break;
            }
        }
        let Some(type_index) = type_index else {
            return Err(format!("no memory type with {want:?}"));
        };
        let mapped = want.contains(vk::MemoryPropertyFlags::HOST_VISIBLE);
        for b in &mut self.blocks {
            if b.type_index == type_index {
                let at = b.used.div_ceil(req.alignment) * req.alignment;
                if at + req.size <= BLOCK {
                    b.used = at + req.size;
                    let ptr = if mapped {
                        // SAFETY: inside the mapped block.
                        unsafe { b.ptr.add(at as usize) }
                    } else {
                        std::ptr::null_mut()
                    };
                    return Ok((b.memory, at, ptr));
                }
            }
        }
        let size = BLOCK.max(req.size);
        let memory = vk_try!(unsafe {
            self.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(size)
                    .memory_type_index(type_index),
                None,
            )
        });
        let ptr = if mapped {
            vk_try!(unsafe {
                self.device
                    .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
            }) as *mut u8
        } else {
            std::ptr::null_mut()
        };
        self.blocks.push(Block {
            memory,
            used: req.size,
            ptr,
            type_index,
        });
        Ok((memory, 0, ptr))
    }

    /// A buffer in memory the host writes and the card reads: device-local and visible when the
    /// machine has it (resizable BAR), else visible.
    fn buffer_memory(
        &mut self,
        size: u64,
        usage: vk::BufferUsageFlags,
    ) -> Result<(vk::Buffer, *mut u8, u64), String> {
        let buffer = vk_try!(unsafe {
            self.device.create_buffer(
                &vk::BufferCreateInfo::default().size(size).usage(usage),
                None,
            )
        });
        let req = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let both = vk::MemoryPropertyFlags::DEVICE_LOCAL
            | vk::MemoryPropertyFlags::HOST_VISIBLE
            | vk::MemoryPropertyFlags::HOST_COHERENT;
        let host = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        let (mem, off, ptr) = match self.alloc(req, both) {
            Ok(x) => x,
            Err(_) => self.alloc(req, host)?,
        };
        vk_try!(unsafe { self.device.bind_buffer_memory(buffer, mem, off) });
        Ok((buffer, ptr, size))
    }

    fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    #[allow(clippy::too_many_arguments)]
    fn make_image(
        &mut self,
        width: u32,
        height: u32,
        levels: u32,
        cube: bool,
        f: Format,
        target: bool,
    ) -> Result<usize, String> {
        let fmt = vk_format(f.wgpu);
        let layers = if cube { 6 } else { 1 };
        let mut usage = vk::ImageUsageFlags::SAMPLED
            | vk::ImageUsageFlags::TRANSFER_DST
            | vk::ImageUsageFlags::TRANSFER_SRC;
        if target {
            usage |= if f.depth {
                vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT
            } else {
                vk::ImageUsageFlags::COLOR_ATTACHMENT
            };
        }
        let levels = levels.clamp(1, 16);
        let info = vk::ImageCreateInfo::default()
            .flags(if cube {
                vk::ImageCreateFlags::CUBE_COMPATIBLE
            } else {
                vk::ImageCreateFlags::empty()
            })
            .image_type(vk::ImageType::TYPE_2D)
            .format(fmt)
            .extent(vk::Extent3D {
                width: width.max(1),
                height: height.max(1),
                depth: 1,
            })
            .mip_levels(levels)
            .array_layers(layers)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(usage)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let image = vk_try!(unsafe { self.device.create_image(&info, None) });
        let req = unsafe { self.device.get_image_memory_requirements(image) };
        let (mem, off, _) = self.alloc(req, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
        vk_try!(unsafe { self.device.bind_image_memory(image, mem, off) });
        let aspect = if f.depth {
            if has_stencil(fmt) {
                vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL
            } else {
                vk::ImageAspectFlags::DEPTH
            }
        } else {
            vk::ImageAspectFlags::COLOR
        };
        // A shader samples depth through its depth aspect.
        let view_aspect = if f.depth {
            vk::ImageAspectFlags::DEPTH
        } else {
            vk::ImageAspectFlags::COLOR
        };
        let view = vk_try!(unsafe {
            self.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(if cube {
                        vk::ImageViewType::CUBE
                    } else {
                        vk::ImageViewType::TYPE_2D
                    })
                    .format(fmt)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: view_aspect,
                        base_mip_level: 0,
                        level_count: levels,
                        base_array_layer: 0,
                        layer_count: layers,
                    }),
                None,
            )
        });
        self.pending.push((
            image,
            aspect,
            levels,
            layers,
            if target || f.depth {
                vk::ImageLayout::GENERAL
            } else {
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
            },
        ));
        self.images.push(Image {
            image,
            view,
            dim: if cube { wgsl::Dim::Cube } else { wgsl::Dim::D2 },
            format: fmt,
            width,
            height,
            aspect,
            fix: f.fix,
            target,
            depth: f.depth,
            opaque: f.opaque,
            levels,
        });
        Ok(self.images.len() - 1)
    }

    /// The view a pass attaches for level `level` of image `i`.
    fn attach_view(&mut self, i: usize, level: u32) -> vk::ImageView {
        if let Some(v) = self.attach_views.get(&(i, level)) {
            return *v;
        }
        let img = &self.images[i];
        let view = unsafe {
            self.device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(img.image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(img.format)
                        .subresource_range(vk::ImageSubresourceRange {
                            aspect_mask: img.aspect,
                            base_mip_level: level,
                            level_count: 1,
                            base_array_layer: 0,
                            layer_count: 1,
                        }),
                    None,
                )
                .expect("an attachment view")
        };
        self.attach_views.insert((i, level), view);
        view
    }

    /// The image and level a surface handle names.
    fn surface(&self, h: Handle) -> Option<(usize, u32)> {
        match self.objects.get(&h)? {
            Obj::Image(i) => Some((*i, 0)),
            Obj::Level(i, l) => Some((*i, *l)),
            _ => None,
        }
    }

    /// Transitions the images made since the last call and waits for them.
    fn flush_init(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let d = &self.device;
        let barriers: Vec<vk::ImageMemoryBarrier2> = self
            .pending
            .drain(..)
            .map(|(image, aspect, levels, layers, layout)| {
                vk::ImageMemoryBarrier2::default()
                    .src_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
                    .dst_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
                    .dst_access_mask(vk::AccessFlags2::MEMORY_READ | vk::AccessFlags2::MEMORY_WRITE)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(layout)
                    .image(image)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: aspect,
                        base_mip_level: 0,
                        level_count: levels,
                        base_array_layer: 0,
                        layer_count: layers,
                    })
            })
            .collect();
        unsafe {
            let cb = d
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(self.init_pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1),
                )
                .expect("a command buffer")[0];
            d.begin_command_buffer(
                cb,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
            .expect("begin");
            d.cmd_pipeline_barrier2(
                cb,
                &vk::DependencyInfo::default().image_memory_barriers(&barriers),
            );
            d.end_command_buffer(cb).expect("end");
            d.queue_submit(
                self.queue,
                &[vk::SubmitInfo::default().command_buffers(&[cb])],
                vk::Fence::null(),
            )
            .expect("submit");
            d.queue_wait_idle(self.queue).expect("idle");
            d.free_command_buffers(self.init_pool, &[cb]);
        }
    }

    fn sampler(&mut self, key: state::SamplerKey) -> vk::Sampler {
        if let Some(s) = self.samplers.get(&key) {
            return *s;
        }
        let d = key.descriptor(true);
        let filter = |f: wgpu::FilterMode| match f {
            wgpu::FilterMode::Nearest => vk::Filter::NEAREST,
            wgpu::FilterMode::Linear => vk::Filter::LINEAR,
        };
        let addr = |a: wgpu::AddressMode| match a {
            wgpu::AddressMode::Repeat => vk::SamplerAddressMode::REPEAT,
            wgpu::AddressMode::MirrorRepeat => vk::SamplerAddressMode::MIRRORED_REPEAT,
            wgpu::AddressMode::ClampToBorder => vk::SamplerAddressMode::CLAMP_TO_BORDER,
            _ => vk::SamplerAddressMode::CLAMP_TO_EDGE,
        };
        let info = vk::SamplerCreateInfo::default()
            .mag_filter(filter(d.mag_filter))
            .min_filter(filter(d.min_filter))
            .mipmap_mode(match d.mipmap_filter {
                wgpu::MipmapFilterMode::Nearest => vk::SamplerMipmapMode::NEAREST,
                wgpu::MipmapFilterMode::Linear => vk::SamplerMipmapMode::LINEAR,
            })
            .address_mode_u(addr(d.address_mode_u))
            .address_mode_v(addr(d.address_mode_v))
            .address_mode_w(addr(d.address_mode_w))
            .anisotropy_enable(d.anisotropy_clamp > 1)
            .max_anisotropy(f32::from(d.anisotropy_clamp))
            .min_lod(d.lod_min_clamp)
            .max_lod(d.lod_max_clamp)
            .border_color(vk::BorderColor::FLOAT_TRANSPARENT_BLACK);
        let s = unsafe { self.device.create_sampler(&info, None) }.expect("a sampler");
        self.samplers.insert(key, s);
        s
    }

    // ---- commands ----------------------------------------------------------------------------

    fn create_device(&mut self, present: &Data<'_>, back: &[Handle], depth: Handle) -> bool {
        let p = words(present);
        if p.len() < 14 {
            return false;
        }
        let (w, h) = (p[0], p[1]);
        let Some(f) = format::format(p[2]) else {
            return false;
        };
        for &b in back {
            match self.make_image(w, h, 1, false, f, true) {
                Ok(i) => {
                    self.objects.insert(b, Obj::Image(i));
                }
                Err(e) => {
                    eprintln!("vulkan: back buffer: {e}");
                    return false;
                }
            }
        }
        self.st.rt[0] = back.first().copied().unwrap_or(0);
        if depth != 0 {
            if let Some(df) = format::format(p[10]) {
                if let Ok(i) = self.make_image(w, h, 1, false, df, true) {
                    self.objects.insert(depth, Obj::Image(i));
                    self.st.ds = depth;
                }
            }
        }
        self.st.viewport = [0, 0, w, h, 0, 1.0f32.to_bits()];
        self.st.scissor = [0, 0, w as i32, h as i32];
        true
    }

    fn create_texture(
        &mut self,
        texture: Handle,
        levels: &[Handle],
        w: u32,
        h: u32,
        usage: u32,
        fmt: u32,
        cube_edge: Option<u32>,
        surfaces: usize,
    ) {
        let Some(f) = format::format(fmt) else {
            return;
        };
        let target = usage & 0x3 != 0;
        let (width, height, n, cube) = match cube_edge {
            Some(e) => (e, e, (surfaces / 6).max(1) as u32, true),
            None => (w, h, levels.len() as u32, false),
        };
        match self.make_image(width, height, n, cube, f, target) {
            Ok(i) => {
                self.objects.insert(texture, Obj::Image(i));
                if !cube {
                    for (l, &h) in levels.iter().enumerate() {
                        self.objects.insert(h, Obj::Level(i, l as u32));
                    }
                }
            }
            Err(e) => eprintln!("vulkan: texture: {e}"),
        }
    }

    fn create_surface(&mut self, surface: Handle, depth: bool, w: u32, h: u32, fmt: u32) {
        let Some(f) = format::format(fmt) else {
            return;
        };
        let _ = depth;
        match self.make_image(w, h, 1, false, f, true) {
            Ok(i) => {
                self.objects.insert(surface, Obj::Image(i));
            }
            Err(e) => eprintln!("vulkan: surface: {e}"),
        }
    }

    fn create_buffer(&mut self, buffer: Handle, index: bool, length: u32, fmt: u32) {
        let usage = if index {
            vk::BufferUsageFlags::INDEX_BUFFER
        } else {
            vk::BufferUsageFlags::VERTEX_BUFFER
        } | vk::BufferUsageFlags::TRANSFER_DST;
        match self.buffer_memory(u64::from(length.max(4)), usage) {
            Ok((b, ptr, _)) => {
                self.buffers.push(Buf {
                    buffer: b,
                    ptr,
                    size: length,
                    index32: index && fmt == 102,
                });
                self.objects
                    .insert(buffer, Obj::Buffer(self.buffers.len() - 1));
            }
            Err(e) => eprintln!("vulkan: buffer: {e}"),
        }
    }

    fn create_shader(&mut self, shader: Handle, tokens: &[u32]) {
        match sm::parse(tokens) {
            Ok(program) => {
                let sig = wgsl::signature(&program);
                let varyings: Vec<wgsl::Semantic> = sig.inputs.iter().map(|(_, s)| *s).collect();
                let fixed = varyings.iter().all(|s| wgsl::fixed_slot(*s).is_some());
                let id = self.id();
                self.shaders.push(Shader {
                    fregs: wgsl::float_constants_read(&program),
                    program,
                    sig,
                    id,
                    varyings,
                    fixed,
                });
                self.objects
                    .insert(shader, Obj::Shader(self.shaders.len() - 1));
            }
            Err(_) => self.counts.bad_shaders += 1,
        }
    }

    fn create_decl(&mut self, decl: Handle, elements: &[u32]) {
        let mut v = Vec::new();
        for e in elements.chunks_exact(2) {
            let stream = (e[0] & 0xFFFF) as u16;
            if stream == 0xFF {
                break;
            }
            v.push(Element {
                stream,
                offset: (e[0] >> 16) as u16,
                ty: (e[1] & 0xFF) as u8,
                usage: ((e[1] >> 16) & 0xFF) as u8,
                index: ((e[1] >> 24) & 0xFF) as u8,
            });
        }
        let id = self.id();
        self.decls.push(Decl { elements: v, id });
        self.objects.insert(decl, Obj::Decl(self.decls.len() - 1));
    }

    fn vertex_link(&mut self, vs: usize, decl: usize) -> std::sync::Arc<VertexLink> {
        let key = (self.shaders[vs].id, self.decls[decl].id);
        if let Some(l) = self.links.get(&key) {
            return l.clone();
        }
        let mut inputs = vec![wgsl::Input::Missing; 16];
        let mut attributes: [Vec<(vk::Format, u32, u32)>; 8] = Default::default();
        for (r, sem) in &self.shaders[vs].sig.inputs {
            if r.n >= 16 {
                continue;
            }
            let Some(e) = self.decls[decl]
                .elements
                .iter()
                .find(|e| e.usage == sem.usage && e.index == sem.index)
            else {
                continue;
            };
            let Some((format, input)) = input_of(e.ty) else {
                continue;
            };
            if e.stream >= 8 {
                continue;
            }
            inputs[r.n as usize] = input;
            attributes[e.stream as usize].push((format, u32::from(e.offset), r.n));
        }
        let l = std::sync::Arc::new(VertexLink { inputs, attributes });
        self.links.insert(key, l.clone());
        l
    }

    fn module(
        &mut self,
        wgsl_source: Result<String, sm::Error>,
        stage: naga::ShaderStage,
    ) -> Option<vk::ShaderModule> {
        let source = wgsl_source.ok()?;
        let module = naga::front::wgsl::parse_str(&source).ok()?;
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .ok()?;
        let options = naga::back::spv::Options {
            lang_version: (1, 3),
            flags: naga::back::spv::WriterFlags::empty(),
            ..Default::default()
        };
        let spv = naga::back::spv::write_vec(
            &module,
            &info,
            &options,
            Some(&naga::back::spv::PipelineOptions {
                shader_stage: stage,
                entry_point: "main".into(),
            }),
        )
        .ok()?;
        self.counts.modules += 1;
        unsafe {
            self.device
                .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&spv), None)
                .ok()
        }
    }

    /// The pipeline of `key` (made the first time).
    fn pipeline(&mut self, key: &PipeKey, vs: usize, ps: usize, decl: usize) -> bool {
        if let Some(p) = self.pipelines.get(key) {
            return p.is_some();
        }
        let made = self.make_pipeline(key, vs, ps, decl);
        if made.is_none() {
            self.counts.bad_shaders += 1;
        } else {
            self.counts.pipelines += 1;
        }
        let ok = made.is_some();
        self.pipelines.insert(key.clone(), made);
        ok
    }

    fn make_pipeline(
        &mut self,
        key: &PipeKey,
        vs: usize,
        ps: usize,
        decl: usize,
    ) -> Option<Pipeline> {
        let link = self.vertex_link(vs, decl);
        let (vsh, psh) = (&self.shaders[vs], &self.shaders[ps]);
        let fixed = psh.fixed;
        let ps_varyings = psh.varyings.clone();
        let vs_varyings = if fixed {
            pair_varyings(vsh, psh)
        } else {
            ps_varyings.clone()
        };
        let vkey = (vsh.id, link.inputs.clone(), vs_varyings.clone());
        let pkey = (psh.id, ps_varyings.clone());
        if !self.vs_modules.contains_key(&vkey) {
            let l = wgsl::Link {
                inputs: link.inputs.clone(),
                centroid: wgsl::centroid_everywhere(vs_varyings.len()),
                varyings: vs_varyings.clone(),
                clip: wgsl::Clip::Off,
                count: false,
                fixed,
                storage: false,
            };
            let src = wgsl::convert(&self.shaders[vs].program, &l);
            let m = self.module(src, naga::ShaderStage::Vertex);
            self.vs_modules.insert(vkey.clone(), m);
        }
        if !self.ps_modules.contains_key(&pkey) {
            let l = wgsl::Link {
                inputs: Vec::new(),
                centroid: wgsl::centroid_everywhere(ps_varyings.len()),
                varyings: ps_varyings.clone(),
                clip: wgsl::Clip::Off,
                count: false,
                fixed,
                storage: false,
            };
            let src = wgsl::convert(&self.shaders[ps].program, &l);
            let m = self.module(src, naga::ShaderStage::Fragment);
            self.ps_modules.insert(pkey.clone(), m);
        }
        let vm = (*self.vs_modules.get(&vkey)?)?;
        let pm = (*self.ps_modules.get(&pkey)?)?;
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vm)
                .name(c"main"),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(pm)
                .name(c"main"),
        ];
        // Vertex input.
        let mut bindings = Vec::new();
        let mut attrs = Vec::new();
        for (k, a) in link.attributes.iter().enumerate() {
            if a.is_empty() {
                continue;
            }
            bindings.push(
                vk::VertexInputBindingDescription::default()
                    .binding(k as u32)
                    .stride(key.strides[k])
                    .input_rate(vk::VertexInputRate::VERTEX),
            );
            for &(format, offset, location) in a {
                attrs.push(
                    vk::VertexInputAttributeDescription::default()
                        .location(location)
                        .binding(k as u32)
                        .format(format)
                        .offset(offset),
                );
            }
        }
        let vi = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&bindings)
            .vertex_attribute_descriptions(&attrs);
        let ia = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(topology(u32::from(key.topology))?);
        let vp = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        // The states, as the wgpu backend converts them.
        let mut s = state::default_render_states(false);
        for (k, &st) in PIPE_STATES.iter().enumerate() {
            s[st] = key.states[k];
        }
        let cull = match state::cull(s[rs::CULLMODE]) {
            Some(wgpu::Face::Front) => vk::CullModeFlags::FRONT,
            Some(wgpu::Face::Back) => vk::CullModeFlags::BACK,
            None => vk::CullModeFlags::NONE,
        };
        let dformat = (key.depth >= 0).then(|| vk::Format::from_raw(key.depth));
        let ds_state = dformat.map(|f| {
            let wf = if has_stencil(f) {
                wgpu::TextureFormat::Depth24PlusStencil8
            } else if f == vk::Format::D16_UNORM {
                wgpu::TextureFormat::Depth16Unorm
            } else {
                wgpu::TextureFormat::Depth24Plus
            };
            state::depth_stencil(&s, wf)
        });
        let bias = ds_state.as_ref().map(|d| d.bias);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(cull)
            .front_face(vk::FrontFace::CLOCKWISE)
            .depth_bias_enable(bias.is_some_and(|b| b.constant != 0 || b.slope_scale != 0.0))
            .depth_bias_constant_factor(bias.map_or(0.0, |b| b.constant as f32))
            .depth_bias_slope_factor(bias.map_or(0.0, |b| b.slope_scale))
            .line_width(1.0);
        let ms = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let cmp = |c: wgpu::CompareFunction| match c {
            wgpu::CompareFunction::Never => vk::CompareOp::NEVER,
            wgpu::CompareFunction::Less => vk::CompareOp::LESS,
            wgpu::CompareFunction::Equal => vk::CompareOp::EQUAL,
            wgpu::CompareFunction::LessEqual => vk::CompareOp::LESS_OR_EQUAL,
            wgpu::CompareFunction::Greater => vk::CompareOp::GREATER,
            wgpu::CompareFunction::NotEqual => vk::CompareOp::NOT_EQUAL,
            wgpu::CompareFunction::GreaterEqual => vk::CompareOp::GREATER_OR_EQUAL,
            wgpu::CompareFunction::Always => vk::CompareOp::ALWAYS,
        };
        let sop = |o: wgpu::StencilOperation| match o {
            wgpu::StencilOperation::Keep => vk::StencilOp::KEEP,
            wgpu::StencilOperation::Zero => vk::StencilOp::ZERO,
            wgpu::StencilOperation::Replace => vk::StencilOp::REPLACE,
            wgpu::StencilOperation::Invert => vk::StencilOp::INVERT,
            wgpu::StencilOperation::IncrementClamp => vk::StencilOp::INCREMENT_AND_CLAMP,
            wgpu::StencilOperation::DecrementClamp => vk::StencilOp::DECREMENT_AND_CLAMP,
            wgpu::StencilOperation::IncrementWrap => vk::StencilOp::INCREMENT_AND_WRAP,
            wgpu::StencilOperation::DecrementWrap => vk::StencilOp::DECREMENT_AND_WRAP,
        };
        let face = |f: wgpu::StencilFaceState, read: u32, write: u32| {
            vk::StencilOpState::default()
                .fail_op(sop(f.fail_op))
                .pass_op(sop(f.pass_op))
                .depth_fail_op(sop(f.depth_fail_op))
                .compare_op(cmp(f.compare))
                .compare_mask(read)
                .write_mask(write)
        };
        let mut dss = vk::PipelineDepthStencilStateCreateInfo::default();
        if let Some(d) = &ds_state {
            let compare = d.depth_compare.unwrap_or(wgpu::CompareFunction::Always);
            let write = d.depth_write_enabled.unwrap_or(false);
            dss = dss
                .depth_test_enable(compare != wgpu::CompareFunction::Always || write)
                .depth_write_enable(write)
                .depth_compare_op(cmp(compare));
            if d.stencil != wgpu::StencilState::default() {
                dss = dss
                    .stencil_test_enable(true)
                    .front(face(
                        d.stencil.front,
                        d.stencil.read_mask,
                        d.stencil.write_mask,
                    ))
                    .back(face(
                        d.stencil.back,
                        d.stencil.read_mask,
                        d.stencil.write_mask,
                    ));
            }
        }
        // Blend: the same for every colour target, each with its own write mask.
        let blend = state::blend(&s);
        let factor = |f: wgpu::BlendFactor| match f {
            wgpu::BlendFactor::Zero => vk::BlendFactor::ZERO,
            wgpu::BlendFactor::One => vk::BlendFactor::ONE,
            wgpu::BlendFactor::Src => vk::BlendFactor::SRC_COLOR,
            wgpu::BlendFactor::OneMinusSrc => vk::BlendFactor::ONE_MINUS_SRC_COLOR,
            wgpu::BlendFactor::SrcAlpha => vk::BlendFactor::SRC_ALPHA,
            wgpu::BlendFactor::OneMinusSrcAlpha => vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
            wgpu::BlendFactor::Dst => vk::BlendFactor::DST_COLOR,
            wgpu::BlendFactor::OneMinusDst => vk::BlendFactor::ONE_MINUS_DST_COLOR,
            wgpu::BlendFactor::DstAlpha => vk::BlendFactor::DST_ALPHA,
            wgpu::BlendFactor::OneMinusDstAlpha => vk::BlendFactor::ONE_MINUS_DST_ALPHA,
            wgpu::BlendFactor::SrcAlphaSaturated => vk::BlendFactor::SRC_ALPHA_SATURATE,
            wgpu::BlendFactor::Constant => vk::BlendFactor::CONSTANT_COLOR,
            wgpu::BlendFactor::OneMinusConstant => vk::BlendFactor::ONE_MINUS_CONSTANT_COLOR,
            _ => vk::BlendFactor::ONE,
        };
        let op = |o: wgpu::BlendOperation| match o {
            wgpu::BlendOperation::Add => vk::BlendOp::ADD,
            wgpu::BlendOperation::Subtract => vk::BlendOp::SUBTRACT,
            wgpu::BlendOperation::ReverseSubtract => vk::BlendOp::REVERSE_SUBTRACT,
            wgpu::BlendOperation::Min => vk::BlendOp::MIN,
            wgpu::BlendOperation::Max => vk::BlendOp::MAX,
        };
        let mut formats = Vec::new();
        let mut attachments = Vec::new();
        for (k, &c) in key.colours.iter().enumerate() {
            if c < 0 {
                continue;
            }
            formats.push(vk::Format::from_raw(c));
            let bits = s[[
                rs::COLORWRITEENABLE,
                rs::COLORWRITEENABLE1,
                rs::COLORWRITEENABLE2,
                rs::COLORWRITEENABLE3,
            ][k]];
            let mask = state::write_mask(bits, false);
            let mut m = vk::ColorComponentFlags::empty();
            if mask.contains(wgpu::ColorWrites::RED) {
                m |= vk::ColorComponentFlags::R;
            }
            if mask.contains(wgpu::ColorWrites::GREEN) {
                m |= vk::ColorComponentFlags::G;
            }
            if mask.contains(wgpu::ColorWrites::BLUE) {
                m |= vk::ColorComponentFlags::B;
            }
            if mask.contains(wgpu::ColorWrites::ALPHA) {
                m |= vk::ColorComponentFlags::A;
            }
            let mut a = vk::PipelineColorBlendAttachmentState::default().color_write_mask(m);
            if let Some(b) = blend {
                a = a
                    .blend_enable(true)
                    .src_color_blend_factor(factor(b.color.src_factor))
                    .dst_color_blend_factor(factor(b.color.dst_factor))
                    .color_blend_op(op(b.color.operation))
                    .src_alpha_blend_factor(factor(b.alpha.src_factor))
                    .dst_alpha_blend_factor(factor(b.alpha.dst_factor))
                    .alpha_blend_op(op(b.alpha.operation));
            }
            attachments.push(a);
        }
        let cb = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
        let dynamic = [
            vk::DynamicState::VIEWPORT,
            vk::DynamicState::SCISSOR,
            vk::DynamicState::BLEND_CONSTANTS,
            vk::DynamicState::STENCIL_REFERENCE,
        ];
        let dy = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic);
        let mut rendering =
            vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
        if let Some(f) = dformat {
            rendering = rendering.depth_attachment_format(f);
            if has_stencil(f) {
                rendering = rendering.stencil_attachment_format(f);
            }
        }
        let info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vi)
            .input_assembly_state(&ia)
            .viewport_state(&vp)
            .rasterization_state(&raster)
            .multisample_state(&ms)
            .depth_stencil_state(&dss)
            .color_blend_state(&cb)
            .dynamic_state(&dy)
            .layout(self.layout)
            .push_next(&mut rendering);
        let p = unsafe {
            self.device
                .create_graphics_pipelines(self.pipeline_cache, &[info], None)
        }
        .ok()?[0];
        Some(Pipeline {
            pipeline: p,
            ps_samplers: self.shaders[ps].sig.samplers.clone(),
            vs_samplers: self.shaders[vs].sig.samplers.clone(),
        })
    }

    // ---- frames ------------------------------------------------------------------------------

    fn cb(&self) -> vk::CommandBuffer {
        self.slots[self.slot].cb
    }

    /// Opens the frame's command buffer (waiting for the slot's last submit first).
    fn ensure_frame(&mut self) {
        if self.open {
            return;
        }
        self.flush_init();
        let d = &self.device;
        let slot = &mut self.slots[self.slot];
        unsafe {
            if slot.submitted {
                d.wait_for_fences(&[slot.fence], true, u64::MAX)
                    .expect("a fence");
                d.reset_fences(&[slot.fence]).expect("reset");
                slot.submitted = false;
            }
            d.reset_command_buffer(slot.cb, vk::CommandBufferResetFlags::empty())
                .expect("reset");
            d.begin_command_buffer(
                slot.cb,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
            .expect("begin");
            if self.query_pool != vk::QueryPool::null() && self.frames_begun < self.queries / 2 {
                d.cmd_write_timestamp2(
                    slot.cb,
                    vk::PipelineStageFlags2::ALL_COMMANDS,
                    self.query_pool,
                    self.frames_begun * 2,
                );
            }
        }
        self.ring_base = self.slot as u64 * RING;
        self.ring_at = 0;
        self.open = true;
        self.pass = None;
        self.bound_pipeline = vk::Pipeline::null();
        self.cur_pipeline = None;
        // Everything is bound again in a new command buffer.
        let st = &mut self.st;
        st.pipe_dirty = true;
        st.header_dirty = [true; 2];
        st.consts_dirty = [true; 2];
        st.tex_dirty = true;
        st.vertex_dirty = 0xFF;
        st.index_dirty = true;
        st.viewport_dirty = true;
        st.fregs = [0; 2];
        st.last_stencil = u32::MAX;
        unsafe {
            d.cmd_bind_descriptor_sets(
                self.slots[self.slot].cb,
                vk::PipelineBindPoint::GRAPHICS,
                self.layout,
                2,
                &[self.set2],
                &[],
            );
            d.cmd_set_blend_constants(self.slots[self.slot].cb, &[1.0; 4]);
        }
    }

    fn end_pass(&mut self) {
        if self.pass.take().is_some() {
            let cb = self.cb();
            let mem = [vk::MemoryBarrier2::default()
                .src_stage_mask(
                    vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT
                        | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS,
                )
                .src_access_mask(
                    vk::AccessFlags2::COLOR_ATTACHMENT_WRITE
                        | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
                )
                .dst_stage_mask(
                    vk::PipelineStageFlags2::FRAGMENT_SHADER
                        | vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT
                        | vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
                        | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS,
                )
                .dst_access_mask(
                    vk::AccessFlags2::SHADER_SAMPLED_READ
                        | vk::AccessFlags2::COLOR_ATTACHMENT_READ
                        | vk::AccessFlags2::COLOR_ATTACHMENT_WRITE
                        | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ
                        | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
                )];
            unsafe {
                self.device.cmd_end_rendering(cb);
                self.device.cmd_pipeline_barrier2(
                    cb,
                    &vk::DependencyInfo::default().memory_barriers(&mem),
                );
            }
        }
    }

    /// The pass over the render targets now bound (begun when they changed).
    fn ensure_pass(&mut self) -> bool {
        let rt: [Option<(usize, u32)>; 4] = std::array::from_fn(|i| self.surface(self.st.rt[i]));
        let ds = self.surface(self.st.ds);
        if self.pass.as_ref().is_some_and(|p| p.0 == rt && p.1 == ds) {
            return true;
        }
        if rt.iter().all(Option::is_none) && ds.is_none() {
            return false;
        }
        self.end_pass();
        let mut colours = Vec::new();
        let (mut w, mut h) = (u32::MAX, u32::MAX);
        for r in rt.iter().flatten() {
            let v = self.attach_view(r.0, r.1);
            let img = &self.images[r.0];
            w = w.min((img.width >> r.1).max(1));
            h = h.min((img.height >> r.1).max(1));
            colours.push(
                vk::RenderingAttachmentInfo::default()
                    .image_view(v)
                    .image_layout(vk::ImageLayout::GENERAL)
                    .load_op(vk::AttachmentLoadOp::LOAD)
                    .store_op(vk::AttachmentStoreOp::STORE),
            );
        }
        let mut depth_att = None;
        if let Some(r) = ds {
            let v = self.attach_view(r.0, r.1);
            let img = &self.images[r.0];
            w = w.min(img.width.max(1));
            h = h.min(img.height.max(1));
            depth_att = Some((
                vk::RenderingAttachmentInfo::default()
                    .image_view(v)
                    .image_layout(vk::ImageLayout::GENERAL)
                    .load_op(vk::AttachmentLoadOp::LOAD)
                    .store_op(vk::AttachmentStoreOp::STORE),
                has_stencil(img.format),
            ));
        }
        let mut info = vk::RenderingInfo::default()
            .render_area(vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D {
                    width: w,
                    height: h,
                },
            })
            .layer_count(1)
            .color_attachments(&colours);
        if let Some((a, stencil)) = &depth_att {
            info = info.depth_attachment(a);
            if *stencil {
                info = info.stencil_attachment(a);
            }
        }
        unsafe { self.device.cmd_begin_rendering(self.cb(), &info) };
        self.pass = Some((rt, ds));
        self.counts.passes += 1;
        self.st.viewport_dirty = true;
        self.bound_pipeline = vk::Pipeline::null();
        true
    }

    fn present(&mut self) {
        if !self.open {
            self.ensure_frame();
        }
        self.end_pass();
        let d = &self.device;
        let slot = &mut self.slots[self.slot];
        unsafe {
            if self.query_pool != vk::QueryPool::null() && self.frames_begun < self.queries / 2 {
                d.cmd_write_timestamp2(
                    slot.cb,
                    vk::PipelineStageFlags2::ALL_COMMANDS,
                    self.query_pool,
                    self.frames_begun * 2 + 1,
                );
            }
            d.end_command_buffer(slot.cb).expect("end");
            let cbs = [slot.cb];
            d.queue_submit(
                self.queue,
                &[vk::SubmitInfo::default().command_buffers(&cbs)],
                slot.fence,
            )
            .expect("submit");
            slot.submitted = true;
        }
        self.frames_begun += 1;
        self.slot = (self.slot + 1) % FRAMES;
        self.open = false;
    }

    /// `len` bytes of this frame's ring: their offset in the ring buffer and where to write them.
    fn ring_alloc(&mut self, len: usize, align: u64) -> Option<(u64, *mut u8)> {
        let at = self.ring_at.div_ceil(align) * align;
        if at + len as u64 > RING {
            self.counts.ring_wraps += 1;
            return None;
        }
        self.ring_at = at + len as u64;
        // SAFETY: inside this frame's slice of the mapped ring.
        Some((self.ring_base + at, unsafe {
            self.ring_ptr.add((self.ring_base + at) as usize)
        }))
    }

    fn ring_write(&mut self, bytes: &[u8], align: u64) -> Option<u64> {
        let (at, dst) = self.ring_alloc(bytes.len(), align)?;
        // SAFETY: `dst` has `bytes.len()` bytes.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len()) };
        Some(at)
    }

    // ---- the draw ----------------------------------------------------------------------------

    fn draw(
        &mut self,
        kind: u32,
        indexed: bool,
        base_vertex: i32,
        start: u32,
        count: u32,
        up: Option<(&[u8], u32)>,
    ) {
        self.counts.draws += 1;
        let (Obj::Shader(vs), Obj::Shader(ps)) = (
            self.objects
                .get(&self.st.vs)
                .copied()
                .unwrap_or(Obj::Image(usize::MAX)),
            self.objects
                .get(&self.st.ps)
                .copied()
                .unwrap_or(Obj::Image(usize::MAX)),
        ) else {
            self.counts.skipped += 1;
            return;
        };
        let Some(Obj::Decl(decl)) = self.objects.get(&self.st.decl).copied() else {
            self.counts.skipped += 1;
            return;
        };
        let Some(topo) = topology(kind) else {
            self.counts.skipped += 1;
            return;
        };
        self.ensure_frame();
        if !self.ensure_pass() {
            self.counts.skipped += 1;
            return;
        }
        let cb = self.cb();
        // The pipeline: looked up when something it depends on changed.
        if self.st.pipe_dirty || up.is_some() {
            let link = self.vertex_link(vs, decl);
            let mut strides = [0u32; 8];
            for (k, a) in link.attributes.iter().enumerate() {
                if !a.is_empty() {
                    strides[k] = if k == 0 && up.is_some() {
                        up.map_or(0, |(_, s)| s)
                    } else {
                        self.st.streams[k].2
                    };
                }
            }
            let colour = |h: Handle| {
                self.surface(h)
                    .map_or(-1, |(i, _)| self.images[i].format.as_raw())
            };
            let key = PipeKey {
                vs: self.shaders[vs].id,
                ps: self.shaders[ps].id,
                decl: self.decls[decl].id,
                strides,
                topology: kind as u8,
                colours: std::array::from_fn(|i| colour(self.st.rt[i])),
                depth: colour(self.st.ds),
                states: PIPE_STATES.map(|s| self.st.rs[s]),
            };
            if self.cur_pipeline.as_ref() != Some(&key) {
                if !self.pipeline(&key, vs, ps, decl) {
                    self.counts.skipped += 1;
                    self.st.pipe_dirty = false;
                    self.cur_pipeline = Some(key);
                    return;
                }
                self.cur_pipeline = Some(key);
                self.st.tex_dirty = true;
            }
            self.st.pipe_dirty = false;
        }
        let Some(key) = self.cur_pipeline.clone() else {
            self.counts.skipped += 1;
            return;
        };
        let Some(Some(p)) = self.pipelines.get(&key) else {
            self.counts.skipped += 1;
            return;
        };
        let (pipe, ps_samplers) = (p.pipeline, p.ps_samplers.clone());
        let d = &self.device;
        if self.bound_pipeline != pipe {
            unsafe { d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, pipe) };
            self.bound_pipeline = pipe;
        }
        // Viewport and scissor.
        if self.st.viewport_dirty {
            let v = self.st.viewport;
            let (x, y, w, h) = (v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32);
            let viewport = [vk::Viewport {
                x,
                y: y + h,
                width: w,
                height: -h,
                min_depth: f32::from_bits(v[4]),
                max_depth: f32::from_bits(v[5]),
            }];
            let sc = if self.st.rs[rs::SCISSORTESTENABLE] != 0 {
                let s = self.st.scissor;
                vk::Rect2D {
                    offset: vk::Offset2D {
                        x: s[0].max(0),
                        y: s[1].max(0),
                    },
                    extent: vk::Extent2D {
                        width: (s[2] - s[0]).max(0) as u32,
                        height: (s[3] - s[1]).max(0) as u32,
                    },
                }
            } else {
                vk::Rect2D {
                    offset: vk::Offset2D {
                        x: v[0] as i32,
                        y: v[1] as i32,
                    },
                    extent: vk::Extent2D {
                        width: v[2],
                        height: v[3],
                    },
                }
            };
            unsafe {
                d.cmd_set_viewport(cb, 0, &viewport);
                d.cmd_set_scissor(cb, 0, &[sc]);
            }
            let centre = [1.0 / w.max(1.0), -1.0 / h.max(1.0)];
            if centre != self.st.centre {
                self.st.centre = centre;
                self.st.header_dirty[0] = true;
            }
            self.st.viewport_dirty = false;
        }
        let sref = self.st.rs[rs::STENCILREF] & 0xFF;
        if self.st.last_stencil != sref {
            unsafe { d.cmd_set_stencil_reference(cb, vk::StencilFaceFlags::FRONT_AND_BACK, sref) };
            self.st.last_stencil = sref;
        }
        // The registers.
        self.write_blocks(vs, ps);
        // The pixel shader's textures.
        if self.st.tex_dirty {
            self.push_textures(&ps_samplers);
        }
        // Geometry.
        if let Some((bytes, stride)) = up {
            let Some(at) = self.ring_write(bytes, 16) else {
                self.counts.skipped += 1;
                return;
            };
            unsafe {
                self.device
                    .cmd_bind_vertex_buffers(cb, 0, &[self.ring], &[at])
            };
            self.st.vertex_dirty |= 1;
            let _ = stride;
        } else if self.st.vertex_dirty != 0 {
            let link = self.vertex_link(vs, decl);
            for k in 0..8 {
                if self.st.vertex_dirty & (1 << k) != 0 && !link.attributes[k].is_empty() {
                    let (h, off, _) = self.st.streams[k];
                    if let Some(Obj::Buffer(b)) = self.objects.get(&h) {
                        unsafe {
                            self.device.cmd_bind_vertex_buffers(
                                cb,
                                k as u32,
                                &[self.buffers[*b].buffer],
                                &[u64::from(off)],
                            );
                        }
                        self.counts.binds += 1;
                    }
                }
            }
            self.st.vertex_dirty = 0;
        }
        let d = &self.device;
        if indexed {
            let Some(Obj::Buffer(b)) = self.objects.get(&self.st.indices).copied() else {
                self.counts.skipped += 1;
                return;
            };
            let wide = self.buffers[b].index32;
            if self.st.index_bound != Some((self.st.indices, wide)) || self.st.index_dirty {
                unsafe {
                    d.cmd_bind_index_buffer(
                        cb,
                        self.buffers[b].buffer,
                        0,
                        if wide {
                            vk::IndexType::UINT32
                        } else {
                            vk::IndexType::UINT16
                        },
                    );
                }
                self.st.index_bound = Some((self.st.indices, wide));
                self.st.index_dirty = false;
                self.counts.binds += 1;
            }
            unsafe { d.cmd_draw_indexed(cb, vertex_count(kind, count), 1, start, base_vertex, 0) };
        } else {
            unsafe { d.cmd_draw(cb, vertex_count(kind, count), 1, start, 0) };
        }
        let _ = topo;
    }

    /// Writes the register blocks that changed and binds the set with their offsets.
    fn write_blocks(&mut self, vs: usize, ps: usize) {
        let fregs = [self.shaders[vs].fregs, self.shaders[ps].fregs];
        let st = &mut self.st;
        let vs_due = st.header_dirty[0] || st.consts_dirty[0] || fregs[0] > st.fregs[0];
        let ps_due = st.header_dirty[1] || st.consts_dirty[1] || fregs[1] > st.fregs[1];
        if !vs_due && !ps_due {
            return;
        }
        let mut changed = false;
        let mut head = [0u8; HEADER];
        for stage in 0..2 {
            let due = [vs_due, ps_due][stage];
            if !due {
                continue;
            }
            let st = &self.st;
            if st.header_dirty[stage] {
                let (i, b, f) = if stage == 0 {
                    (&st.vsi, &st.vsb, &st.vsf)
                } else {
                    (&st.psi, &st.psb, &st.psf)
                };
                let _ = f;
                let slots = if stage == 0 { 16..20 } else { 0..16 };
                let mut fix = [0u32; 16];
                let mut bias = [0f32; 16];
                for (k, slot) in slots.enumerate() {
                    fix[k] = st.textures[slot].map_or(0, |t| t.fix);
                    bias[k] = f32::from_bits(st.samplers[slot][ss::MIPMAPLODBIAS]);
                }
                let sys = if stage == 0 {
                    [st.centre[0], st.centre[1], 0.0, 0.0]
                } else {
                    // The alpha test left out: always passes.
                    [0.0, 8.0, 0.0, 0.0]
                };
                header(i, b, sys, fix, bias, &mut head);
                let Some(at) = self.ring_write(&head, 64) else {
                    return;
                };
                self.st.offsets[stage] = at as u32;
                self.st.header_dirty[stage] = false;
                changed = true;
            }
            let n = 16 * fregs[stage] as usize;
            let Some((at, dst)) = self.ring_alloc(n, 64) else {
                return;
            };
            let f = if stage == 0 {
                &self.st.vsf
            } else {
                &self.st.psf
            };
            // SAFETY: `dst` has `n` bytes of this frame's ring; `f` has 4096.
            unsafe { std::ptr::copy_nonoverlapping(f.as_ptr(), dst, n) };
            self.st.offsets[2 + stage] = at as u32;
            self.st.consts_dirty[stage] = false;
            self.st.fregs[stage] = fregs[stage];
            changed = true;
        }
        if changed {
            let o = self.st.offsets;
            unsafe {
                self.device.cmd_bind_descriptor_sets(
                    self.cb(),
                    vk::PipelineBindPoint::GRAPHICS,
                    self.layout,
                    0,
                    &[self.set0],
                    &[o[0], o[1], o[2], o[3]],
                );
            }
            self.counts.binds += 1;
        }
    }

    fn push_textures(&mut self, samplers: &[(u32, wgsl::Dim)]) {
        self.st.tex_dirty = false;
        if samplers.is_empty() {
            return;
        }
        let mut images: Vec<vk::DescriptorImageInfo> = Vec::with_capacity(samplers.len());
        let mut smp: Vec<vk::DescriptorImageInfo> = Vec::with_capacity(samplers.len());
        for &(n, dim) in samplers {
            let slot = (n as usize).min(15);
            let bound = self.st.textures[slot].filter(|b| b.dim == dim);
            let view = bound.map_or(
                self.dummies[match dim {
                    wgsl::Dim::D2 => 0,
                    wgsl::Dim::Cube => 1,
                    wgsl::Dim::D3 => 2,
                }]
                .0,
                |b| b.view,
            );
            images.push(
                vk::DescriptorImageInfo::default()
                    .image_view(view)
                    .image_layout(vk::ImageLayout::GENERAL),
            );
            let sampler = if bound.is_some() {
                let key = state::SamplerKey::new(&self.st.samplers[slot], true);
                self.sampler(key)
            } else {
                self.dummy_sampler
            };
            smp.push(vk::DescriptorImageInfo::default().sampler(sampler));
        }
        let mut writes = Vec::with_capacity(samplers.len() * 2);
        for (k, &(n, _)) in samplers.iter().enumerate() {
            let b = 2 * n.min(15);
            writes.push(
                vk::WriteDescriptorSet::default()
                    .dst_binding(b)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(std::slice::from_ref(&images[k])),
            );
            writes.push(
                vk::WriteDescriptorSet::default()
                    .dst_binding(b + 1)
                    .descriptor_type(vk::DescriptorType::SAMPLER)
                    .image_info(std::slice::from_ref(&smp[k])),
            );
        }
        unsafe {
            self.push.cmd_push_descriptor_set(
                self.cb(),
                vk::PipelineBindPoint::GRAPHICS,
                self.layout,
                1,
                &writes,
            );
        }
        self.counts.pushes += 1;
    }

    fn clear(&mut self, flags: u32, colour: u32, z: f32, stencil: u32) {
        self.ensure_frame();
        if !self.ensure_pass() {
            return;
        }
        let Some((rt, ds)) = self.pass.clone() else {
            return;
        };
        let mut atts = Vec::new();
        if flags & 1 != 0 {
            if let Some((i, _)) = rt[0] {
                let c = |s: u32| ((colour >> s) & 0xFF) as f32 / 255.0;
                let opaque = self.images[i].opaque;
                atts.push(vk::ClearAttachment {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    color_attachment: 0,
                    clear_value: vk::ClearValue {
                        color: vk::ClearColorValue {
                            float32: [c(16), c(8), c(0), if opaque { 1.0 } else { c(24) }],
                        },
                    },
                });
            }
        }
        if let Some((i, _)) = ds {
            let mut aspect = vk::ImageAspectFlags::empty();
            if flags & 2 != 0 {
                aspect |= vk::ImageAspectFlags::DEPTH;
            }
            if flags & 4 != 0 && has_stencil(self.images[i].format) {
                aspect |= vk::ImageAspectFlags::STENCIL;
            }
            if !aspect.is_empty() {
                atts.push(vk::ClearAttachment {
                    aspect_mask: aspect,
                    color_attachment: 0,
                    clear_value: vk::ClearValue {
                        depth_stencil: vk::ClearDepthStencilValue { depth: z, stencil },
                    },
                });
            }
        }
        if atts.is_empty() {
            return;
        }
        let (i, l) = rt[0].or(ds).expect("a target");
        let img = &self.images[i];
        let rect = [vk::ClearRect {
            rect: vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D {
                    width: (img.width >> l).max(1),
                    height: (img.height >> l).max(1),
                },
            },
            base_array_layer: 0,
            layer_count: 1,
        }];
        unsafe { self.device.cmd_clear_attachments(self.cb(), &atts, &rect) };
    }

    fn set_texture(&mut self, sampler: u32, texture: Handle) {
        let slot = if sampler >= 256 {
            16 + (sampler - 256) as usize
        } else {
            sampler as usize
        };
        if slot >= SLOTS {
            return;
        }
        self.st.textures[slot] = match self.objects.get(&texture) {
            Some(Obj::Image(i)) => {
                let img = &self.images[*i];
                Some(Bound {
                    view: img.view,
                    dim: img.dim,
                    fix: img.fix,
                })
            }
            _ => None,
        };
        self.st.tex_dirty = true;
        self.st.header_dirty[usize::from(slot < 16)] = true;
    }
}

// SAFETY: the target is made and used on one thread (the replay's); its raw pointers are into
// memory it maps and owns.
unsafe impl Send for VulkanTarget {}

impl Drop for VulkanTarget {
    fn drop(&mut self) {
        // SAFETY: the device is idle first; the objects go with the device and instance.
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

impl Backend for VulkanTarget {
    fn execute(&mut self, cmd: Cmd<Data<'_>>) -> Reply {
        match cmd {
            Cmd::CreateDevice {
                present,
                back_buffers,
                depth_stencil,
                ..
            } => {
                let back = words(&back_buffers);
                let created = self.create_device(&present, &back, depth_stencil);
                return Reply::Device {
                    created,
                    native: None,
                };
            }
            Cmd::Present { .. } => self.present(),
            Cmd::Clear {
                flags,
                colour,
                z,
                stencil,
                ..
            } => self.clear(flags, colour, f32::from_bits(z), stencil),
            Cmd::SetRenderTarget { index, surface, .. } => {
                if (index as usize) < 4 && self.st.rt[index as usize] != surface {
                    self.st.rt[index as usize] = surface;
                    self.st.pipe_dirty = true;
                }
            }
            Cmd::SetDepthStencilSurface { surface, .. } => {
                if self.st.ds != surface {
                    self.st.ds = surface;
                    self.st.pipe_dirty = true;
                }
            }
            Cmd::SetViewport { viewport, .. } => {
                self.st.viewport = viewport;
                self.st.viewport_dirty = true;
            }
            Cmd::SetScissorRect { rect, .. } => {
                self.st.scissor = rect;
                self.st.viewport_dirty = true;
            }
            Cmd::SetRenderState { state, value, .. } => {
                if let Some(s) = self.st.rs.get_mut(state as usize) {
                    if *s != value {
                        *s = value;
                        if PIPE_STATES.contains(&(state as usize)) {
                            self.st.pipe_dirty = true;
                        } else if state as usize == rs::SCISSORTESTENABLE {
                            self.st.viewport_dirty = true;
                        }
                    }
                }
            }
            Cmd::SetSamplerState {
                sampler,
                kind,
                value,
                ..
            } => {
                let slot = if sampler >= 256 {
                    16 + (sampler - 256) as usize
                } else {
                    sampler as usize
                };
                if slot < SLOTS && (kind as usize) < 14 {
                    let s = &mut self.st.samplers[slot][kind as usize];
                    if *s != value {
                        *s = value;
                        self.st.tex_dirty = true;
                        if kind as usize == ss::MIPMAPLODBIAS {
                            self.st.header_dirty[usize::from(slot < 16)] = true;
                        }
                    }
                }
            }
            Cmd::SetTexture {
                sampler, texture, ..
            } => self.set_texture(sampler, texture),
            Cmd::SetVertexDeclaration { declaration, .. } => {
                if self.st.decl != declaration {
                    self.st.decl = declaration;
                    self.st.pipe_dirty = true;
                    self.st.vertex_dirty = 0xFF;
                }
            }
            Cmd::SetShader { pixel, shader, .. } => {
                let slot = if pixel {
                    &mut self.st.ps
                } else {
                    &mut self.st.vs
                };
                if *slot != shader {
                    *slot = shader;
                    self.st.pipe_dirty = true;
                    self.st.consts_dirty[usize::from(pixel)] = true;
                }
            }
            Cmd::SetConstants {
                kind,
                start,
                count,
                data,
                ..
            } => {
                let stage = usize::from(kind >= 3);
                match kind % 3 {
                    0 => {
                        let f = if stage == 0 {
                            &mut self.st.vsf
                        } else {
                            &mut self.st.psf
                        };
                        let (a, n) = (16 * start as usize, 16 * count as usize);
                        if a + n <= 4096 {
                            data.copy_to(0, &mut f[a..a + n]);
                        }
                        self.st.consts_dirty[stage] = true;
                    }
                    1 => {
                        let w = words(&data);
                        let i = if stage == 0 {
                            &mut self.st.vsi
                        } else {
                            &mut self.st.psi
                        };
                        for (k, c) in w.chunks_exact(4).enumerate() {
                            if let Some(r) = i.get_mut(start as usize + k) {
                                r.copy_from_slice(c);
                            }
                        }
                        self.st.header_dirty[stage] = true;
                    }
                    _ => {
                        let w = words(&data);
                        let b = if stage == 0 {
                            &mut self.st.vsb
                        } else {
                            &mut self.st.psb
                        };
                        for (k, v) in w.iter().enumerate() {
                            if let Some(r) = b.get_mut(start as usize + k) {
                                *r = *v;
                            }
                        }
                        self.st.header_dirty[stage] = true;
                    }
                }
            }
            Cmd::SetStreamSource {
                stream,
                buffer,
                offset,
                stride,
                ..
            } => {
                if (stream as usize) < 8 {
                    let old = self.st.streams[stream as usize];
                    self.st.streams[stream as usize] = (buffer, offset, stride);
                    self.st.vertex_dirty |= 1 << stream;
                    if old.2 != stride {
                        self.st.pipe_dirty = true;
                    }
                }
            }
            Cmd::SetIndices { buffer, .. } => {
                self.st.indices = buffer;
                self.st.index_dirty = true;
            }
            Cmd::DrawPrimitive {
                kind, start, count, ..
            } => self.draw(kind, false, 0, start, count, None),
            Cmd::DrawIndexedPrimitive {
                kind,
                base_vertex,
                start_index,
                count,
                ..
            } => self.draw(kind, true, base_vertex, start_index, count, None),
            Cmd::DrawPrimitiveUp {
                kind,
                count,
                stride,
                vertices,
                ..
            } => {
                let bytes = vertices.bytes();
                self.draw(kind, false, 0, 0, count, Some((&bytes, stride)));
            }
            Cmd::CreateTexture {
                texture,
                levels,
                width,
                height,
                usage,
                format,
                ..
            } => {
                let l = words(&levels);
                self.create_texture(texture, &l, width, height, usage, format, None, 0);
            }
            Cmd::CreateCubeTexture {
                texture,
                surfaces,
                edge,
                usage,
                format,
                ..
            } => {
                let n = words(&surfaces).len();
                self.create_texture(texture, &[], 0, 0, usage, format, Some(edge), n);
            }
            Cmd::CreateSurface {
                surface,
                depth,
                width,
                height,
                format,
                ..
            } => self.create_surface(surface, depth, width, height, format),
            Cmd::CreateOffscreenSurface { .. } => {}
            Cmd::CreateBuffer {
                buffer,
                index,
                length,
                format,
                ..
            } => self.create_buffer(buffer, index, length, format),
            Cmd::CreateShader { shader, tokens, .. } => {
                let t = words(&tokens);
                self.create_shader(shader, &t);
            }
            Cmd::CreateDeclaration {
                declaration,
                elements,
                ..
            } => {
                let e = words(&elements);
                self.create_decl(declaration, &e);
            }
            Cmd::UploadBuffer {
                buffer,
                offset,
                data,
                ..
            } => {
                if let Some(Obj::Buffer(b)) = self.objects.get(&buffer) {
                    let b = &self.buffers[*b];
                    let n = data.len() as usize;
                    if !b.ptr.is_null() && offset as usize + n <= b.size as usize {
                        // SAFETY: inside the buffer's mapped memory.
                        unsafe {
                            let dst = std::slice::from_raw_parts_mut(b.ptr.add(offset as usize), n);
                            data.copy_to(0, dst);
                        }
                    }
                }
            }
            Cmd::Destroy { .. } => {}
            _ => {}
        }
        match cmd {
            Cmd::ReadSurface { .. } => Reply::Bytes(None),
            Cmd::GetRenderTargetData { .. } => Reply::Rows(None),
            Cmd::QueryGetData { .. } => Reply::Query(None),
            _ => Reply::None,
        }
    }
}

impl Target for VulkanTarget {
    fn name(&self) -> &'static str {
        "vulkan"
    }

    fn frame_begin(&mut self) {
        self.ensure_frame();
    }

    fn drain(&mut self) {
        self.flush_init();
        unsafe {
            let _ = self.device.queue_wait_idle(self.queue);
        }
    }

    fn gpu_ns(&mut self) -> Option<Vec<f64>> {
        if self.query_pool == vk::QueryPool::null() {
            return None;
        }
        let n = self.frames_begun.min(self.queries / 2) as usize;
        let mut ticks = vec![0u64; n * 2];
        unsafe {
            self.device
                .get_query_pool_results(
                    self.query_pool,
                    0,
                    &mut ticks,
                    vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT,
                )
                .ok()?;
        }
        Some(
            ticks
                .chunks_exact(2)
                .map(|p| p[1].saturating_sub(p[0]) as f64 * f64::from(self.timestamp_period))
                .collect(),
        )
    }

    fn notes(&self) -> String {
        let c = &self.counts;
        format!(
            "{}: draws {} skipped {} pipelines {} modules {} bad shaders {} pushes {} binds {} passes {} ring overflows {}",
            self.gpu_name, c.draws, c.skipped, c.pipelines, c.modules, c.bad_shaders, c.pushes, c.binds, c.passes, c.ring_wraps
        )
    }
}
