//! External benchmark executable; compiled by packet_bench.py, not Cargo.
#![forbid(unsafe_code)]
use std::{error::Error, hint::black_box, time::Instant};

fn fingerprint(input: &[u8]) -> u64 {
    match opura::Packet::parse(input) {
        Err(_) => 0,
        Ok(packet) => {
            let mut hash = packet.frames().len() as u64;
            for frame in packet.frames() {
                hash = hash
                    .wrapping_mul(1_000_003)
                    .wrapping_add((frame.as_ptr() as usize - input.as_ptr() as usize) as u64);
                hash = hash
                    .wrapping_mul(1_000_003)
                    .wrapping_add(frame.len() as u64);
            }
            hash
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    let iterations: usize = args.get(2).ok_or("missing iterations")?.parse()?;
    let bytes = std::fs::read(args.get(1).ok_or("missing corpus")?)?;
    let mut remaining = bytes.as_slice();
    let mut packets = Vec::new();
    while !remaining.is_empty() {
        let (length, rest) = remaining.split_at_checked(4).ok_or("truncated length")?;
        let size = u32::from_le_bytes(length.try_into()?) as usize;
        let (packet, rest) = rest.split_at_checked(size).ok_or("truncated packet")?;
        packets.push(packet);
        remaining = rest;
    }
    if packets.is_empty() || iterations == 0 {
        return Err("empty workload".into());
    }
    for packet in &packets {
        println!("{}", fingerprint(packet));
    }
    for _ in 0..1000 {
        for packet in &packets {
            black_box(fingerprint(black_box(packet)));
        }
    }
    let start = Instant::now();
    for _ in 0..iterations {
        for packet in &packets {
            black_box(fingerprint(black_box(packet)));
        }
    }
    println!(
        "ns {}",
        start.elapsed().as_nanos() as f64 / (iterations * packets.len()) as f64
    );
    Ok(())
}
