// Test data generator: mergesort-gen <output> <count> <numeric|string|csv> [seed]
// Deterministic xorshift64* PRNG so tests are reproducible.
// CSV mode emits 4 comma-separated columns: id(u64), i(usize), city, name.
use std::io::{BufWriter, Write};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: gen <output> <count> <numeric|string|csv> [seed]");
        std::process::exit(2);
    }
    let out = &args[0];
    let count: usize = args[1].parse().unwrap();
    let mode = args[2].clone();
    let seed: u64 = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(0x9E3779B97F4A7C15);

    let f = std::fs::File::create(out).unwrap();
    let mut w = BufWriter::with_capacity(1024 * 1024, f);
    let mut rng = Rng(if seed == 0 { 1 } else { seed });

    match mode.as_str() {
        "numeric" => {
            for _ in 0..count {
                w.write_all(&rng.next().to_le_bytes()).unwrap();
            }
        }
        "string" => {
            for _ in 0..count {
                // Random ASCII words: length 1..32, chars a-z.
                let len = (rng.next() % 31 + 1) as usize;
                let mut line = String::with_capacity(len);
                for _ in 0..len {
                    line.push((b'a' + (rng.next() % 26) as u8) as char);
                }
                w.write_all(line.as_bytes()).unwrap();
                w.write_all(b"\n").unwrap();
            }
        }
        "csv" => {
            // Columns: id (random u64), i (row index), city, name.
            // Sortable by numeric key (col 0) or string keys (col 2/3).
            let cities = ["Jakarta", "Bandung", "Surabaya", "Medan", "Depok", "Bogor", "Tangerang", "Bekasi"];
            for i in 0..count {
                let id = rng.next();
                let city = cities[(rng.next() % cities.len() as u64) as usize];
                let name_len = (rng.next() % 11 + 2) as usize;
                let mut name = String::with_capacity(name_len);
                for _ in 0..name_len {
                    name.push((b'a' + (rng.next() % 26) as u8) as char);
                }
                w.write_all(format!("{},{},{},{}\n", id, i, city, name).as_bytes()).unwrap();
            }
        }
        other => {
            eprintln!("unknown mode: {}", other);
            std::process::exit(2);
        }
    }
    w.flush().unwrap();
}
