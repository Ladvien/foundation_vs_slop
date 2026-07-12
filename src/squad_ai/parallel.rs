//! **Multi-process parallelism for the offline search** (feature `test-harness`).
//!
//! The search spends ~all its wall-clock in [`super::evaluate::rollout`], and a rollout cannot be
//! parallelised *within* a process: `sim_harness` holds a process-wide lock (`HARNESS_LOCK`) and pins the
//! Bevy compute pool + rayon to a single thread for determinism (see the `evaluate` module doc). So the
//! only axis of parallelism is **processes** — this module runs a pool of `train worker` subprocesses, each
//! a fresh OS process with its own lock and its own single-threaded pool, and fans the independent triples
//! of a candidate's scoring across them.
//!
//! **Why this is exact, not approximate.** The unit of work is one [`super::coevolve::TripleJob`], and a
//! rollout is a pure function of `(brains, world, seed, ticks)` — it draws none of the search's `ChaCha8Rng`
//! stream (it reseeds its own sim RNG from the passed dungeon seed). The driver draws every seed *before*
//! dispatching (identical RNG order to the inline path) and [`WorkerPool::eval`] returns results in input
//! order, so the reduction, the archive inserts, and therefore the final archives are **byte-identical** to
//! `jobs = 1`. `tests/search_parallel.rs` pins that equality.
//!
//! The ceiling is `OPPONENTS` (3): a batch is one candidate's opponent set, and children are sequential
//! (each samples the archive the previous one just mutated — load-bearing coevolutionary structure, not an
//! accident). Raising `jobs` past `OPPONENTS` fills no more slots.
//!
//! Protocol: length-prefixed (`u32` LE) **bincode** frames over the worker's stdin (driver → worker) and
//! stdout (worker → driver). The first frame the driver sends is the frozen [`ModePrior`] (handshake), so
//! every worker scores against the exact reference the driver holds in memory. Workers are spawned with
//! `RUST_LOG=off` so the sim's tracing output never contaminates the stdout data channel; their stderr is
//! inherited so a genuine crash is still visible.
//!
//! **Why bincode, not RON text (issue #44).** The frames carry `f32` in both directions — genome params
//! (`Genome::params`, `WorldGenome`) driver→worker, and the scored [`TripleScore`] worker→driver. RON
//! serializes floats as shortest-decimal text, whose round-trip is not a guaranteed bit-identical `f32`
//! across platforms. That 1-ULP perturbation feeds the `mean` fitness and the descriptor `cell()` binning,
//! flipping the `>=` elitism so a *different* elite wins a niche — the parallel archive diverged from the
//! inline one on x86 Linux while matching on ARM. bincode writes each `f32` as its 4 raw IEEE-754 bytes
//! (fixed endianness), so every value crosses the boundary byte-for-byte and the two paths stay identical
//! on every architecture. The frames are ephemeral IPC (never persisted, never human-read), so losing
//! RON's readability costs nothing; `test_parallel_wire_roundtrip_is_bit_exact` pins the property.

use std::io::{BufReader, BufWriter, ErrorKind, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::coevolve::{score_triple_compact, Templates, TripleJob, TripleScore};
use super::surprise::ModePrior;

/// A pool of worker processes. Held by [`super::coevolve::Evaluator::Pool`] for the run's lifetime; on drop
/// every worker is killed and reaped.
pub(crate) struct WorkerPool {
    /// One `Mutex` per worker: `eval` gives each feeder thread exclusive use of one worker, and the `Mutex`
    /// only bridges the shared `&self` to the `&mut Worker` an exchange needs — it is never contended.
    workers: Vec<Mutex<Worker>>,
}

struct Worker {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl WorkerPool {
    /// Spawn `jobs` worker processes and hand each the frozen prior. Errors (spawn failure, a worker that
    /// dies before the handshake) are fatal — there is no degraded single-process fallback, by design.
    pub(crate) fn spawn(jobs: usize, prior: &ModePrior) -> Result<WorkerPool, String> {
        let n = jobs.max(1);
        // The worker is this same binary re-invoked with `worker`. Tests override the path
        // (`TRAIN_WORKER_EXE`) because under `cargo test` `current_exe()` is the test harness, not `train`.
        let exe = match std::env::var_os("TRAIN_WORKER_EXE") {
            Some(path) => std::path::PathBuf::from(path),
            None => std::env::current_exe().map_err(|e| format!("locate current exe for workers: {e}"))?,
        };
        let mut workers = Vec::with_capacity(n);
        for i in 0..n {
            let mut child = Command::new(&exe)
                .arg("worker")
                // Silence the sim's tracing so nothing but framed results reaches the worker's stdout.
                .env("RUST_LOG", "off")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .map_err(|e| format!("spawn worker {i}: {e}"))?;
            let stdin = BufWriter::new(
                child.stdin.take().ok_or_else(|| format!("worker {i} has no stdin"))?,
            );
            let stdout = BufReader::new(
                child.stdout.take().ok_or_else(|| format!("worker {i} has no stdout"))?,
            );
            let mut worker = Worker { child, stdin, stdout };
            worker.send_prior(prior).map_err(|e| format!("worker {i} handshake: {e}"))?;
            workers.push(Mutex::new(worker));
        }
        Ok(WorkerPool { workers })
    }

    /// Evaluate `jobs` across the pool, preserving input order. At most `min(workers, jobs.len())` run
    /// concurrently. Any worker error is fatal and propagates (a lost job would silently under-count a
    /// candidate's opponents).
    pub(crate) fn eval(&self, jobs: &[TripleJob]) -> Result<Vec<Option<TripleScore>>, String> {
        if jobs.is_empty() {
            return Ok(Vec::new());
        }
        let cursor = AtomicUsize::new(0);
        // One slot per job. `None` = not yet computed; `Some(inner)` where `inner` is the criterion result
        // (itself an `Option`). The two layers keep "unevaluated" distinct from "evaluated to a reject".
        let slots: Vec<Mutex<Option<Option<TripleScore>>>> =
            (0..jobs.len()).map(|_| Mutex::new(None)).collect();
        let n = self.workers.len().min(jobs.len());

        std::thread::scope(|scope| -> Result<(), String> {
            let handles: Vec<_> = (0..n)
                .map(|k| {
                    let cursor = &cursor;
                    let slots = &slots;
                    let worker = &self.workers[k];
                    scope.spawn(move || -> Result<(), String> {
                        let mut w = worker.lock().map_err(|_| "worker mutex poisoned".to_string())?;
                        loop {
                            let idx = cursor.fetch_add(1, Ordering::Relaxed);
                            if idx >= jobs.len() {
                                break;
                            }
                            let res = w.exchange(&jobs[idx])?;
                            *slots[idx].lock().map_err(|_| "slot mutex poisoned".to_string())? = Some(res);
                        }
                        Ok(())
                    })
                })
                .collect();
            for h in handles {
                h.join().map_err(|_| "worker feeder thread panicked".to_string())??;
            }
            Ok(())
        })?;

        // Every slot must be filled; an empty one is an internal invariant break, not a criterion reject.
        slots
            .into_iter()
            .enumerate()
            .map(|(i, slot)| {
                slot.into_inner()
                    .map_err(|_| "slot mutex poisoned".to_string())?
                    .ok_or_else(|| format!("internal: triple {i} was never evaluated"))
            })
            .collect()
    }
}

impl Worker {
    /// Send the frozen prior as the handshake frame.
    fn send_prior(&mut self, prior: &ModePrior) -> Result<(), String> {
        let payload = bincode::serialize(prior).map_err(|e| format!("encode prior: {e}"))?;
        write_frame(&mut self.stdin, &payload)?;
        self.stdin.flush().map_err(|e| format!("flush prior to worker: {e}"))
    }

    /// One request/response: send a job, block for its result.
    fn exchange(&mut self, job: &TripleJob) -> Result<Option<TripleScore>, String> {
        let payload = bincode::serialize(job).map_err(|e| format!("encode job: {e}"))?;
        write_frame(&mut self.stdin, &payload)?;
        self.stdin.flush().map_err(|e| format!("flush job to worker: {e}"))?;
        let resp = read_frame(&mut self.stdout)?
            .ok_or_else(|| "worker closed its output before answering — it likely crashed".to_string())?;
        bincode::deserialize(&resp).map_err(|e| format!("decode worker reply: {e}"))
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Workers are idle (blocked reading stdin) once the search is done; kill+reap is immediate and
        // leaves no zombies. They own no shared state or files, so an abrupt stop is safe.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The `train worker` entry point: handshake on the prior, then score every job frame until stdin closes.
///
/// Rebuilds `Templates::authored()` locally (the same code-literal reference the driver uses) rather than
/// receiving it — cheap and guarantees no drift. Runs in the working directory it inherited from the
/// driver, so it reads the identical `assets/config/config.ron`.
pub fn worker_main() -> Result<(), String> {
    let t = Templates::authored();

    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut writer = BufWriter::new(stdout.lock());

    let prior_frame =
        read_frame(&mut reader)?.ok_or_else(|| "worker received no prior handshake".to_string())?;
    let prior: ModePrior =
        bincode::deserialize(&prior_frame).map_err(|e| format!("decode prior: {e}"))?;

    while let Some(frame) = read_frame(&mut reader)? {
        let job: TripleJob = bincode::deserialize(&frame).map_err(|e| format!("decode job: {e}"))?;
        let result = score_triple_compact(
            &t, &job.squad, &job.swarm, &job.world, &prior, job.seed_a, job.seed_b, job.ticks,
        )?;
        let payload = bincode::serialize(&result).map_err(|e| format!("encode result: {e}"))?;
        write_frame(&mut writer, &payload)?;
        writer.flush().map_err(|e| format!("flush result: {e}"))?;
    }
    Ok(())
}

/// Write one length-prefixed frame: `u32` LE length, then the bytes.
fn write_frame(w: &mut impl Write, bytes: &[u8]) -> Result<(), String> {
    let len = u32::try_from(bytes.len()).map_err(|_| "frame exceeds 4 GiB".to_string())?;
    w.write_all(&len.to_le_bytes()).map_err(|e| format!("write frame length: {e}"))?;
    w.write_all(bytes).map_err(|e| format!("write frame body: {e}"))
}

/// Read one length-prefixed frame. `Ok(None)` on a clean EOF at a frame boundary (the peer closed the
/// pipe); any other short read is a fatal protocol error.
fn read_frame(r: &mut impl Read) -> Result<Option<Vec<u8>>, String> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(format!("read frame length: {e}")),
    }
    let n = u32::from_le_bytes(len) as usize;
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf).map_err(|e| format!("read frame body ({n} bytes): {e}"))?;
    Ok(Some(buf))
}

#[cfg(test)]
mod tests {
    //! Pins the determinism contract of the worker wire (issue #44): every `f32` the search fans across
    //! processes must survive the encode/decode **bit-for-bit**, or the parallel archive diverges from the
    //! inline one (a 1-ULP fitness shift flips `>=` elitism; a shifted descriptor lands in a different
    //! `cell()`). Pure + fast — no rollout, no GPU. If the frame format is ever swapped back to a
    //! shortest-decimal text codec (RON) this reds, catching the regression that produced #44.
    use super::*;
    use crate::squad_ai::coevolve::{SquadGenome, SwarmGenome};
    use crate::squad_ai::genome::Genome;
    use crate::squad_ai::qd::BehaviorDescriptor;
    use crate::squad_ai::world_genome::WorldGenome;

    /// `f32` bit patterns chosen to stress a decimal round-trip: full-precision decimals, values whose
    /// shortest text is ambiguous, tiny/huge magnitudes, and the smallest subnormal.
    fn awkward_f32s() -> Vec<f32> {
        vec![
            0.1,
            0.2,
            0.3,
            1.0 / 3.0,
            0.123_456_79_f32,
            0.999_999_f32,
            f32::from_bits(0x3DCC_CCCD),
            f32::from_bits(0x3E99_999A),
            f32::MIN_POSITIVE,
            f32::from_bits(1), // smallest positive subnormal
            123_456.78_f32,
            1e-20_f32,
            1e20_f32,
        ]
    }

    fn all_bits(job: &TripleJob, score: &TripleScore) -> Vec<u32> {
        let mut bits = Vec::new();
        let mut push_genome = |g: &Genome| bits.extend(g.params.iter().map(|x| x.to_bits()));
        for g in &job.squad.0 {
            push_genome(g);
        }
        push_genome(&job.swarm.crab);
        push_genome(&job.swarm.scout);
        push_genome(&job.swarm.smiley);
        bits.extend(job.world.0.iter().map(|x| x.to_bits()));
        for d in [&score.squad, &score.swarm, &score.world] {
            bits.push(d.aggression.to_bits());
            bits.push(d.exploration.to_bits());
        }
        bits.push(score.score.to_bits());
        bits
    }

    #[test]
    fn parallel_wire_roundtrip_is_bit_exact() {
        let v = awkward_f32s();
        let genome = || Genome { params: v.clone(), ranks: vec![0u8; v.len()] };
        // The two wire payloads: a job (driver → worker) and a scored result (worker → driver).
        let job = TripleJob {
            squad: SquadGenome(vec![genome(), genome()]),
            swarm: SwarmGenome { crab: genome(), scout: genome(), smiley: genome() },
            world: WorldGenome(v.clone()),
            seed_a: 0x5C0_9191,
            seed_b: 0xA11CE,
            ticks: 7200,
        };
        let score = TripleScore {
            score: v[3],
            squad: BehaviorDescriptor::new(v[0], v[6]),
            swarm: BehaviorDescriptor::new(v[7], v[9]),
            world: BehaviorDescriptor::new(v[11], v[12]),
        };

        // Round-trip through the exact codec the IPC uses, in both wrappers the protocol sends.
        let job_bytes = bincode::serialize(&job).expect("encode job");
        let job_back: TripleJob = bincode::deserialize(&job_bytes).expect("decode job");
        let score_bytes = bincode::serialize(&Some(score)).expect("encode score");
        let score_back: Option<TripleScore> = bincode::deserialize(&score_bytes).expect("decode score");
        let score_back = score_back.expect("Some(score) round-trips as Some");

        assert_eq!(
            all_bits(&job, &score),
            all_bits(&job_back, &score_back),
            "an f32 changed bits crossing the worker IPC — the parallel search would diverge from inline"
        );
    }
}
