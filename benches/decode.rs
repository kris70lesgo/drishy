use drishy::{decode, encode};
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let width = 1920;
    let height = 1080;
    let pixels = vec![128_u8; width * height * 4];
    let qoi = encode::encode_rgb_or_rgba(width as u32, height as u32, 4, &pixels)
        .expect("benchmark image should encode");
    let iterations = 20;
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(decode::decode(black_box(&qoi)).expect("benchmark image should decode"));
    }
    println!("{} decodes in {:?}", iterations, start.elapsed());
}
