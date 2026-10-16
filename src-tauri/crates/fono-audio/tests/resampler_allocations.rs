use fono_audio::audio_source::resample::StreamingResampler;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct CountingAllocator;

fn count() {
    if COUNTING.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCATIONS.try_with(|value| value.set(value.get() + 1));
    }
}

// This test binary alone installs the instrumented system allocator. Counting is
// thread-local so unrelated test/runtime threads cannot make the assertion flaky.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        System.alloc(layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        System.alloc_zeroed(layout)
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        System.realloc(pointer, layout, size)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn prepared_resampler_does_not_allocate_in_the_capture_loop() {
    let input = [1_234; 65_536];
    for (input_rate, output_rate) in [
        (44_100, 16_000),
        (48_000, 16_000),
        (96_000, 16_000),
        (192_000, 16_000),
        (16_000, 16_000),
        (8_000, 16_000),
    ] {
        let mut resampler = StreamingResampler::new(input_rate, output_rate);
        let mut output = Vec::with_capacity(resampler.output_capacity(input.len()));
        ALLOCATIONS.set(0);
        COUNTING.set(true);
        for packet in [1, 137, 512, 65_536, 3, 2, 65_535] {
            resampler.process(&input[..packet], &mut output);
        }
        resampler.finish(&mut output);
        COUNTING.set(false);
        assert_eq!(ALLOCATIONS.get(), 0, "{input_rate}/{output_rate}");
    }
    let mut resampler = StreamingResampler::new(48_000, 16_000);
    let mut output = Vec::with_capacity(resampler.output_capacity(1));
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    for sample in &input[..1_000] {
        resampler.process(std::slice::from_ref(sample), &mut output);
    }
    resampler.finish(&mut output);
    COUNTING.set(false);
    assert_eq!(ALLOCATIONS.get(), 0, "single-frame packets and tail");
}
