use std::collections::HashSet;
use std::time::{Duration, Instant};

use iced::futures::executor::block_on;
use iced::wgpu::{
    self, BindGroupDescriptor, BindGroupEntry, Buffer, BufferDescriptor, BufferUsages,
    CommandEncoderDescriptor, ComputePassDescriptor, ComputePipeline, ComputePipelineDescriptor,
    Device, DeviceDescriptor, DeviceType, PollType, Queue, ShaderModuleDescriptor, ShaderSource,
};
use peepers_core::bench::{Gpu, GpuKind};

use crate::graph::level;

/// How long each kernel runs on an adapter.
const PHASE: Duration = Duration::from_millis(1500);
const WORKGROUP: u32 = 256;
/// Rounds each invocation of the compute kernel loops, and the operations one round performs:
/// a fused multiply-add counts as two, the multiply and add after it as two more.
const ROUNDS: u64 = 4096;
const OPERATIONS_PER_ROUND: u64 = 4;
/// Workgroups the compute kernel is dispatched over.
const COMPUTE_GROUPS: u32 = 4096;
/// Bytes in each buffer of the copy kernel, and the vectors of four floats that make them up.
const COPY_BYTES: u64 = 64 << 20;
const VECTOR_BYTES: u64 = 16;
const BATCH: usize = 8;

fn compute_source() -> String {
    format!(
        "
@group(0) @binding(0) var<storage, read_write> sink: array<f32>;

@compute @workgroup_size({WORKGROUP})
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    var a = f32(id.x) * 0.001 + 1.0;
    var b = 1.0001;
    for (var round = 0u; round < {ROUNDS}u; round = round + 1u) {{
        a = fma(a, b, 0.5);
        b = b * 0.9999 + 0.0001;
        a = a * 0.5;
    }}
    sink[id.x] = a + b;
}}
"
    )
}

fn copy_source() -> String {
    format!(
        "
@group(0) @binding(0) var<storage, read> input: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> output: array<vec4<f32>>;

@compute @workgroup_size({WORKGROUP})
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    output[id.x] = input[id.x] + vec4<f32>(1.0);
}}
"
    )
}

/// A graphics adapter the benchmark can run on, named the way the graphics driver names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    pub kind: GpuKind,
    vendor: u32,
    device: u32,
}

/// What tells one physical adapter from another, which the PCI ids do and the name may not.
pub type Id = (u32, u32);

impl Candidate {
    #[must_use]
    pub fn id(&self) -> Id {
        (self.vendor, self.device)
    }
}

/// The hardware adapters, once each: software renderers are not what the machine's GPU does, and
/// a loader that lists one device through two driver manifests would otherwise show it twice.
fn hardware(instance: &wgpu::Instance) -> Vec<wgpu::Adapter> {
    let mut seen = HashSet::new();
    instance
        .enumerate_adapters(wgpu::Backends::all())
        .into_iter()
        .filter(|adapter| {
            let info = adapter.get_info();
            info.device_type != DeviceType::Cpu
                && seen.insert((info.backend, info.vendor, info.device))
        })
        .collect()
}

fn kind(device_type: DeviceType) -> GpuKind {
    match device_type {
        DeviceType::IntegratedGpu => GpuKind::Integrated,
        DeviceType::DiscreteGpu => GpuKind::Discrete,
        _ => GpuKind::Other,
    }
}

#[must_use]
pub fn candidates() -> Vec<Candidate> {
    hardware(&wgpu::Instance::new(&wgpu::InstanceDescriptor::default()))
        .iter()
        .map(|adapter| {
            let info = adapter.get_info();
            Candidate {
                kind: kind(info.device_type),
                name: info.name,
                vendor: info.vendor,
                device: info.device,
            }
        })
        .collect()
}

/// Benchmarks the hardware adapters whose ids are in `selected`, or all of them when it is `None`.
///
/// # Errors
///
/// Fails when none of them is available or one cannot be used.
pub fn run(selected: Option<&HashSet<Id>>) -> Result<Vec<Gpu>, String> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let results = hardware(&instance)
        .into_iter()
        .filter(|adapter| {
            let info = adapter.get_info();
            selected.is_none_or(|ids| ids.contains(&(info.vendor, info.device)))
        })
        .map(|adapter| {
            let info = adapter.get_info();
            let (device, queue) = block_on(adapter.request_device(&DeviceDescriptor::default()))
                .map_err(|error| format!("{}: {error}", info.name))?;
            let gigaflops = compute(&device, &queue);
            let bandwidth = copy(&device, &queue)?;
            Ok(Gpu {
                kind: kind(info.device_type),
                adapter: info.name,
                gigaflops,
                bandwidth,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if results.is_empty() {
        Err("None of the selected graphics adapters could be used".to_owned())
    } else {
        Ok(results)
    }
}

/// Billions of single-precision operations per second.
fn compute(device: &Device, queue: &Queue) -> f32 {
    let invocations = u64::from(WORKGROUP) * u64::from(COMPUTE_GROUPS);
    let sink = buffer(device, invocations * 4);
    let pipeline = pipeline(device, &compute_source());
    let bind = device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[BindGroupEntry {
            binding: 0,
            resource: sink.as_entire_binding(),
        }],
    });
    let (elapsed, dispatches) = time(device, queue, &pipeline, &bind, COMPUTE_GROUPS);
    level(invocations * ROUNDS * OPERATIONS_PER_ROUND * dispatches) / elapsed.as_secs_f32() / 1e9
}

fn copy(device: &Device, queue: &Queue) -> Result<u64, String> {
    let (source, target) = (buffer(device, COPY_BYTES), buffer(device, COPY_BYTES));
    let pipeline = pipeline(device, &copy_source());
    let bind = device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: source.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: target.as_entire_binding(),
            },
        ],
    });
    let groups = u32::try_from(COPY_BYTES / VECTOR_BYTES / u64::from(WORKGROUP))
        .map_err(|error| error.to_string())?;
    let (elapsed, dispatches) = time(device, queue, &pipeline, &bind, groups);
    let moved = COPY_BYTES * 2 * dispatches;
    Ok(moved.saturating_mul(1000)
        / u64::try_from(elapsed.as_millis())
            .unwrap_or(u64::MAX)
            .max(1))
}

/// Dispatches in batches until [`PHASE`] has passed, returning the time and how many dispatches ran.
fn time(
    device: &Device,
    queue: &Queue,
    pipeline: &ComputePipeline,
    bind: &wgpu::BindGroup,
    groups: u32,
) -> (Duration, u64) {
    let (began, mut dispatches) = (Instant::now(), 0_u64);
    while began.elapsed() < PHASE {
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            for _ in 0..BATCH {
                pass.dispatch_workgroups(groups, 1, 1);
            }
        }
        queue.submit([encoder.finish()]);
        drop(device.poll(PollType::wait_indefinitely()));
        dispatches += BATCH as u64;
    }
    (began.elapsed(), dispatches)
}

fn buffer(device: &Device, size: u64) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: None,
        size,
        usage: BufferUsages::STORAGE,
        mapped_at_creation: false,
    })
}

fn pipeline(device: &Device, source: &str) -> ComputePipeline {
    let module = device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Wgsl(source.into()),
    });
    device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs a hardware graphics adapter"]
    fn every_selected_hardware_adapter_gets_a_score() {
        let everything: HashSet<_> = candidates().iter().map(Candidate::id).collect();
        let results = run(Some(&everything)).unwrap();
        let names: HashSet<_> = results.iter().map(|gpu| &gpu.adapter).collect();
        assert_eq!(
            names.len(),
            results.len(),
            "each adapter is benchmarked once"
        );
        assert!(
            results
                .iter()
                .all(|gpu| gpu.gigaflops > 0.0 && gpu.bandwidth > 0)
        );
    }
}
