use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use image::{ImageBuffer, Rgba};
use ms::vision::PerceptionPipeline;

/// Create a realistic test frame (1366x767, typical game resolution)
fn create_test_frame() -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    const WIDTH: u32 = 1366;
    const HEIGHT: u32 = 767;

    let mut img = ImageBuffer::new(WIDTH, HEIGHT);

    // Fill with game-like background (dark tones)
    for pixel in img.pixels_mut() {
        *pixel = Rgba([30, 30, 35, 255]);
    }

    // Add HUD elements (light text regions)
    for x in 10..150 {
        for y in 10..50 {
            img.put_pixel(x, y, Rgba([200, 200, 200, 255]));
        }
    }

    // Add HP bar area
    for x in 10..200 {
        for y in 55..75 {
            img.put_pixel(x, y, Rgba([220, 50, 50, 255]));
        }
    }

    img
}

/// `PerceptionPipeline` is stateful: the motion detector keeps the previous
/// frame and the combat detector keeps a motion history, so a pipeline reused
/// across iterations measures a cold first iteration followed by warm ones.
/// Every benchmark below therefore builds a fresh pipeline in an untimed
/// setup closure. `PerIteration` batching is used because a warmed pipeline
/// retains a full frame copy, which makes larger batches memory-hungry.
fn benchmark_full_pipeline(c: &mut Criterion) {
    let frame = black_box(create_test_frame());

    c.bench_function("full_pipeline_single_frame", |b| {
        b.iter_batched(
            PerceptionPipeline::new,
            |mut pipeline| pipeline.detect(&frame),
            BatchSize::PerIteration,
        )
    });
}

fn benchmark_frame_dimensions(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_sizes");
    group.sample_size(30);

    // Test different frame dimensions
    let dimensions = vec![
        (1366, 767, "1366x767 (typical)"),
        (1920, 1080, "1920x1080 (fullhd)"),
        (2560, 1440, "2560x1440 (2k)"),
    ];

    for (width, height, label) in dimensions {
        let mut frame = ImageBuffer::new(width, height);
        for pixel in frame.pixels_mut() {
            *pixel = Rgba([30, 30, 35, 255]);
        }

        let frame = black_box(frame);

        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &(width, height),
            |b, _| {
                b.iter_batched(
                    PerceptionPipeline::new,
                    |mut pipeline| pipeline.detect(&frame),
                    BatchSize::PerIteration,
                )
            },
        );
    }

    group.finish();
}

fn benchmark_successive_frames(c: &mut Criterion) {
    let mut group = c.benchmark_group("temporal");
    group.sample_size(30);

    let frame1 = black_box(create_test_frame());
    let mut frame2 = create_test_frame();

    // Simulate motion in frame 2
    for x in 100..120 {
        for y in 100..120 {
            frame2.put_pixel(x, y, Rgba([150, 150, 200, 255]));
        }
    }
    let frame2 = black_box(frame2);

    // Cold start: a pipeline that has never seen a frame, which is what the
    // name promises. With a shared pipeline only the very first iteration was
    // actually cold.
    group.bench_function("first_frame_cold_start", |b| {
        b.iter_batched(
            PerceptionPipeline::new,
            |mut pipeline| pipeline.detect(&frame1),
            BatchSize::PerIteration,
        )
    });

    // Warm state: prime the pipeline with frame1 in the untimed setup so the
    // measured call is the second frame of a fresh pipeline every time.
    group.bench_function("second_frame_warm_state", |b| {
        b.iter_batched(
            || {
                let mut pipeline = PerceptionPipeline::new();
                pipeline.detect(&frame1);
                pipeline
            },
            |mut pipeline| pipeline.detect(&frame2),
            BatchSize::PerIteration,
        )
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_full_pipeline,
    benchmark_frame_dimensions,
    benchmark_successive_frames
);
criterion_main!(benches);
