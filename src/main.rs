//! Phase 0 smoke test: confirm the Windows build reaches the GPU through DirectX 12
//! and can run a compute shader end to end.

use wgpu::util::DeviceExt;

const SHADER: &str = r#"
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

const COUNT: u32 = 16 * 1024 * 1024; // one 4K texture's worth of pixels

fn main() {
    pollster::block_on(run());
}

async fn run() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        })
        .await
        .expect("no DX12 adapter found");

    let info = adapter.get_info();
    println!("adapter: {} ({:?}, driver {})", info.name, info.backend, info.driver_info);

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        })
        .await
        .expect("failed to create device");

    let input: Vec<f32> = (0..COUNT).map(|i| i as f32).collect();
    let size = (input.len() * std::mem::size_of::<f32>()) as u64;

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
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
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
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: storage.as_entire_binding() }],
    });

    let start = std::time::Instant::now();
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        // Split across two dimensions to stay under the 65535 workgroups-per-dimension limit.
        let groups = COUNT.div_ceil(256);
        pass.dispatch_workgroups(groups.min(65535), groups.div_ceil(65535), 1);
    }
    encoder.copy_buffer_to_buffer(&storage, 0, &readback, 0, size);
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map failed"));
    device.poll(wgpu::PollType::wait_indefinitely()).expect("poll failed");
    let elapsed = start.elapsed();

    let mapped = slice.get_mapped_range().expect("mapped range unavailable");
    let output: &[f32] = bytemuck::cast_slice(&mapped);
    let bad = output.iter().enumerate().filter(|&(i, &v)| v != i as f32 * 2.0).count();
    println!("{COUNT} values doubled in {elapsed:.2?}, mismatches: {bad}");
    assert_eq!(bad, 0, "GPU produced wrong results");
    println!("GPU smoke test passed");
}
