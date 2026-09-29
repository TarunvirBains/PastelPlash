//! GPU access through `wgpu` on DirectX 12, plus a compute self-test used by `gpu-info`.

use anyhow::{Context, Result, bail};
use wgpu::util::DeviceExt;

const SELF_TEST_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>,
        @builtin(num_workgroups) groups: vec3<u32>) {
    let i = id.y * groups.x * 256u + id.x;
    if (i < arrayLength(&data)) {
        data[i] = data[i] * 2.0;
    }
}
"#;

const SELF_TEST_COUNT: u32 = 16 * 1024 * 1024; // one 4K texture's worth of pixels

/// The DX12 shader compiler, pinned: shader output bits depend on it. FXC ships with Windows
/// (`d3dcompiler_47.dll`); wgpu's default (`Auto`) would switch to DXC whenever a
/// `dxcompiler.dll` happens to be on the PATH.
pub const DX12_COMPILER: wgpu::Dx12Compiler = wgpu::Dx12Compiler::Fxc;

pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// Opens the high-performance DX12 adapter with its full limits.
    pub async fn new() -> Result<Self> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::DX12;
        desc.backend_options.dx12.shader_compiler = DX12_COMPILER;
        let instance = wgpu::Instance::new(desc);
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            })
            .await
            .context("no DX12 adapter found")?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .context("failed to create device")?;
        Ok(Self {
            adapter,
            device,
            queue,
        })
    }
}

/// Prints the adapter and runs a compute shader end to end, checking every result.
pub fn info() -> Result<()> {
    pollster::block_on(async {
        let gpu = Gpu::new().await?;
        let info = gpu.adapter.get_info();
        println!(
            "adapter: {} ({:?}, driver {}), shader compiler {DX12_COMPILER:?}",
            info.name, info.backend, info.driver_info
        );
        self_test(&gpu)
    })
}

fn self_test(gpu: &Gpu) -> Result<()> {
    let Gpu { device, queue, .. } = gpu;
    let input: Vec<f32> = (0..SELF_TEST_COUNT).map(|i| i as f32).collect();
    let size = std::mem::size_of_val(input.as_slice()) as u64;

    let storage = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("data"),
        contents: bytemuck::cast_slice(&input),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("double"),
        source: wgpu::ShaderSource::Wgsl(SELF_TEST_SHADER.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("double"),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: storage.as_entire_binding(),
        }],
    });

    let start = std::time::Instant::now();
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        // Split across two dimensions to stay under the 65535 workgroups-per-dimension limit.
        let groups = SELF_TEST_COUNT.div_ceil(256);
        pass.dispatch_workgroups(groups.min(65535), groups.div_ceil(65535), 1);
    }
    encoder.copy_buffer_to_buffer(&storage, 0, &readback, 0, size);
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map failed"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .context("device poll failed")?;
    let elapsed = start.elapsed();

    let mapped = slice
        .get_mapped_range()
        .context("mapped range unavailable")?;
    let output: &[f32] = bytemuck::cast_slice(&mapped);
    let bad = output
        .iter()
        .enumerate()
        .filter(|&(i, &v)| v != i as f32 * 2.0)
        .count();
    println!("{SELF_TEST_COUNT} values doubled in {elapsed:.2?}, mismatches: {bad}");
    if bad != 0 {
        bail!("GPU self-test produced {bad} wrong results");
    }
    println!("GPU self-test passed");
    Ok(())
}
