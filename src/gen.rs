// Test data generator: gen <output> <count> <numeric|string|csv> [seed] [header]
// Deterministic xorshift64* PRNG so tests are reproducible.
// CSV mode emits 4 comma-separated columns: id(u64), i(usize), city, name.
// Seed 0 dipetakan ke 1: xorshift dengan state 0 macet selamanya di 0,
// sehingga seed eksplisit 0 == seed 1 (tetap deterministik, tidak hang).
// Argumen ke-5 opsional "header": tulis baris header CSV "id,idx,city,name"
// (hanya valid untuk mode csv). Semua error keluar dengan kode 2 + pesan Indonesia.
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

/// Keluar dengan kode 2 (usage error) + pesan Indonesia. Tanpa .unwrap().
fn die(msg: &str) -> ! {
    eprintln!("gen: {}", msg);
    std::process::exit(2);
}

fn parse_count(s: &str) -> usize {
    s.parse::<usize>().unwrap_or_else(|_| {
        die(&format!("count tidak valid (harus bilangan bulat >= 0): '{}'", s));
    })
}

fn parse_seed(s: &str) -> u64 {
    s.parse::<u64>().unwrap_or_else(|_| {
        die(&format!("seed tidak valid (harus bilangan bulat 0..2^64-1): '{}'", s));
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("cara pakai: gen <output> <count> <numeric|string|csv> [seed] [header]");
        eprintln!("  seed 0 dipetakan ke 1 (xorshift state 0 macet di 0 selamanya).");
        std::process::exit(2);
    }
    let out = &args[0];
    let count: usize = parse_count(&args[1]);
    let mode = args[2].clone();
    let seed: u64 = args.get(3).map(|s| parse_seed(s)).unwrap_or(0x9E3779B97F4A7C15);
    let want_header = args.get(4).map(|s| s.as_str()) == Some("header");
    if want_header && mode != "csv" {
        die("argumen 'header' hanya berlaku untuk mode csv");
    }
    if args.len() > 5 {
        die(&format!("argumen berlebih: '{}' (maksimal 5 argumen)", args[5..].join(" ")));
    }

    let f = std::fs::File::create(out)
        .unwrap_or_else(|e| die(&format!("tidak bisa membuat file '{}': {}", out, e)));
    let mut w = BufWriter::with_capacity(1024 * 1024, f);
    // Seed 0 -> 1 agar PRNG tidak macet (terdokumentasi, deterministik).
    let mut rng = Rng(if seed == 0 { 1 } else { seed });

    match mode.as_str() {
        "numeric" => {
            for _ in 0..count {
                w.write_all(&rng.next().to_le_bytes())
                    .unwrap_or_else(|e| die(&format!("gagal menulis '{}': {}", out, e)));
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
                w.write_all(line.as_bytes())
                    .unwrap_or_else(|e| die(&format!("gagal menulis '{}': {}", out, e)));
                w.write_all(b"\n")
                    .unwrap_or_else(|e| die(&format!("gagal menulis '{}': {}", out, e)));
            }
        }
        "csv" => {
            // Columns: id (random u64), i (row index), city, name.
            // Sortable by numeric key (col 0) or string keys (col 2/3).
            if want_header {
                w.write_all(b"id,idx,city,name\n")
                    .unwrap_or_else(|e| die(&format!("gagal menulis header ke '{}': {}", out, e)));
            }
            let cities = ["Jakarta", "Bandung", "Surabaya", "Medan", "Depok", "Bogor", "Tangerang", "Bekasi"];
            for i in 0..count {
                let id = rng.next();
                let city = cities[(rng.next() % cities.len() as u64) as usize];
                let name_len = (rng.next() % 11 + 2) as usize;
                let mut name = String::with_capacity(name_len);
                for _ in 0..name_len {
                    name.push((b'a' + (rng.next() % 26) as u8) as char);
                }
                w.write_all(format!("{},{},{},{}\n", id, i, city, name).as_bytes())
                    .unwrap_or_else(|e| die(&format!("gagal menulis '{}': {}", out, e)));
            }
        }
        other => {
            die(&format!("mode tidak dikenal: '{}' (pilih numeric|string|csv)", other));
        }
    }
    w.flush()
        .unwrap_or_else(|e| die(&format!("gagal flush file '{}': {}", out, e)));
}
