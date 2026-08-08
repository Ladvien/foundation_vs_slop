//! **One seeded generator, and why every reproducibility claim rests on it.**
//!
//! `seeded(u64)` returns a ChaCha8 stream; `DetRng` is the extension trait the generators actually
//! draw through (`raw_u64`, `below`, `unit`). Same seed, same sequence — on any platform, regardless
//! of thread or ECS system execution order. That is the whole contract, and it is why the crate has
//! exactly one RNG type rather than a bespoke PRNG next to `rand`'s.
//!
//! `below(n)` is a genuinely unbiased range draw, not `raw_u64() % n` — modulo reduction skews toward
//! low indices whenever `n` does not divide 2^64, and that skew would quietly bias every placement
//! decision downstream.
//!
//! Run: `cargo run -p emerge-core --example det_rng`

use emerge_core::rng::{seeded, DetRng};

fn main() {
    println!("Same seed, two independent streams — the sequences must match exactly:\n");
    let mut a = seeded(0xA11CE);
    let mut b = seeded(0xA11CE);
    let mut all_match = true;

    println!("     raw_u64 (stream A)   raw_u64 (stream B)   equal");
    for _ in 0..6 {
        let (x, y) = (a.raw_u64(), b.raw_u64());
        all_match &= x == y;
        println!("  {x:>20}  {y:>20}   {}", if x == y { "yes" } else { "NO" });
    }

    println!("\nA different seed diverges immediately:");
    let mut c = seeded(0xA11CF);
    println!("  seed 0xA11CE → {:>20}", seeded(0xA11CE).raw_u64());
    println!("  seed 0xA11CF → {:>20}", c.raw_u64());

    // `unit()` is the [0,1) float draw the solvers use for weighted picks.
    println!("\nunit() — uniform in [0,1):");
    let mut r = seeded(7);
    let draws: Vec<f64> = (0..8).map(|_| r.unit()).collect();
    let line: Vec<String> = draws.iter().map(|v| format!("{v:.4}")).collect();
    println!("  {}", line.join("  "));
    let mean = draws.iter().sum::<f64>() / draws.len() as f64;
    println!("  mean of 8 draws {mean:.4} (expect ≈0.5 in the limit, not at n=8)");

    // `below(n)` over a non-power-of-two n is where modulo reduction would show its bias.
    const N: usize = 7;
    const DRAWS: usize = 700_000;
    println!("\nbelow({N}) over {DRAWS} draws — a modulo reduction would over-pick the low buckets:");
    let mut counts = [0usize; N];
    let mut r = seeded(0xB1A5);
    for _ in 0..DRAWS {
        counts[r.below(N)] += 1;
    }
    let expected = DRAWS as f64 / N as f64;
    let mut worst = 0.0f64;
    for (i, &c) in counts.iter().enumerate() {
        let dev = (c as f64 - expected) / expected * 100.0;
        worst = worst.max(dev.abs());
        println!("  bucket {i}: {c:>7}  ({dev:+.3}% from uniform)");
    }
    println!("  worst deviation {worst:.3}%");

    if all_match {
        println!("\n✔ reproducible — the property every generated world in this project depends on");
    } else {
        eprintln!("\n✘ streams diverged; that would be a bug in the crate");
        std::process::exit(1);
    }
}
