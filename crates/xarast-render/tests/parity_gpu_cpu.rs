//! Golden level B: the GPU backend against the CPU one, perceptually.
//!
//! This test **skips** when there is no adapter. The crate must build and
//! test with no GPU and no windowing system present, so a missing adapter
//! is a fact about the machine, not a failure of the code.
//!
//! Build with `--features gpu` to compile it at all; without the feature
//! the whole file is one empty test that says so.

#![cfg(feature = "gpu")]

mod common;

use common::render_case;
use xarast_render::backend::gpu::adapter_available;
use xarast_render::corpus::all_cases;
use xarast_render::golden::compare;

#[test]
fn the_two_backends_agree_within_the_parity_band() {
    if !adapter_available() {
        eprintln!(
            "skipping GPU parity: wgpu enumerated no adapter on this machine. \
             The gate is unmeasured, not passed."
        );
        return;
    }
    let (device, queue) = match create_device() {
        Some(pair) => pair,
        None => {
            eprintln!("skipping GPU parity: an adapter exists but no device could be created");
            return;
        }
    };
    let mut gpu = xarast_render::GpuBackend::new(
        device,
        queue,
        xarast_render::backend::gpu::GpuConfig::default(),
    )
    .expect("the device can hold twelve 64 KiB tables");

    let mut failures = Vec::new();
    for case in all_cases() {
        let cpu = render_case(&case);
        let mut gpu_target =
            xarast_render::Surface::new(case.view.viewport.width(), case.view.viewport.height());
        let dl = xarast_render::DisplayList::build(
            &case.scene,
            &case.view,
            &xarast_render::DirtyRect::NONE,
        );
        gpu.render(&dl, &case.resolver, &mut gpu_target)
            .expect("the corpus fits on the device");
        let c = compare(&gpu_target, &cpu);
        if !c.passes_parity() {
            failures.push(format!(
                "{}: rms {:.5}, max {}/255",
                case.name, c.rms, c.max_channel_delta
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} scenes exceed the parity band (rms < 0.5 %, max 8/255): {:?}",
        failures.len(),
        &failures[..failures.len().min(8)]
    );
}

fn create_device() -> Option<(std::sync::Arc<wgpu::Device>, std::sync::Arc<wgpu::Queue>)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    let (device, queue) =
        block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
    Some((std::sync::Arc::new(device), std::sync::Arc::new(queue)))
}

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};
    struct Noop;
    impl Wake for Noop {
        fn wake(self: Arc<Self>) {}
    }
    let waker = Waker::from(Arc::new(Noop));
    let mut cx = Context::from_waker(&waker);
    let mut fut = Box::pin(fut);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
