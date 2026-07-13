//! `BfldPipelineHandle` — worker-thread wrapper around [`BfldPipeline`] and a
//! [`Publish`]er. ADR-118 §2.1 single-call operator surface.
//!
//! `spawn()` returns a handle owning the inbound channel sender. The worker
//! thread loops on `recv()`, drives one `pipeline.process()` per input, and
//! forwards any emitted `BfldEvent` through `publish_event()`. `shutdown()`
//! closes the channel and joins the thread.

#![cfg(feature = "std")]

use std::sync::mpsc::{channel, RecvError, SendError, Sender};
use std::thread::{self, JoinHandle};

use crate::coherence_gate::SoulMatchOracle;
use crate::mqtt_topics::{publish_event, Publish};
use crate::pipeline::BfldPipeline;
use crate::{IdentityEmbedding, SensingInputs};

/// Frame-level input to the spawned worker. The pipeline state — gate,
/// embedding ring, hasher — lives behind the worker thread; callers only
/// send the per-frame sensing data.
pub struct PipelineInput {
    /// Sensing fields fed to `pipeline.process`.
    pub inputs: SensingInputs,
    /// Optional embedding for the iter-15 hasher input + iter-8 ring.
    pub embedding: Option<IdentityEmbedding>,
}

/// Handle to the spawned worker. Drop or `shutdown()` to stop. `send()`
/// returns an error after shutdown.
pub struct BfldPipelineHandle {
    sender: Sender<PipelineInput>,
    worker: Option<JoinHandle<()>>,
}

impl BfldPipelineHandle {
    /// Spawn a worker that owns `pipeline` and `publisher`. Returns a handle
    /// whose `send()` enqueues sensing inputs into the worker thread.
    ///
    /// Publish errors are logged to stderr and the worker continues — single
    /// frame failures should not kill the long-running pipeline.
    #[must_use]
    pub fn spawn<P>(mut pipeline: BfldPipeline, mut publisher: P) -> Self
    where
        P: Publish + Send + 'static,
        P::Error: core::fmt::Debug,
    {
        let (sender, receiver) = channel::<PipelineInput>();
        let worker = thread::spawn(move || loop {
            match receiver.recv() {
                Ok(PipelineInput { inputs, embedding }) => {
                    if let Some(event) = pipeline.process(inputs, embedding) {
                        if let Err(e) = publish_event(&mut publisher, &event) {
                            eprintln!("BFLD publish error: {e:?}");
                        }
                    }
                }
                Err(RecvError) => break, // channel closed by shutdown / drop
            }
        });
        Self {
            sender,
            worker: Some(worker),
        }
    }

    /// Variant of [`Self::spawn`] that installs a long-lived
    /// [`SoulMatchOracle`] used on every per-frame `process` call. The oracle
    /// must be `Send + Sync + 'static` because the worker thread consults it
    /// on every recv. Pairs with ADR-121 §2.6: when the oracle reports a
    /// `Match`, a would-be Recalibrate gate transition is downgraded to
    /// `PredictOnly` (high score is the *intended* outcome of a known-enrolled
    /// person match, not an attacker-grade sniffer arrival).
    #[must_use]
    pub fn spawn_with_oracle<P, O>(mut pipeline: BfldPipeline, mut publisher: P, oracle: O) -> Self
    where
        P: Publish + Send + 'static,
        P::Error: core::fmt::Debug,
        O: SoulMatchOracle + Send + Sync + 'static,
    {
        let (sender, receiver) = channel::<PipelineInput>();
        let worker = thread::spawn(move || loop {
            match receiver.recv() {
                Ok(PipelineInput { inputs, embedding }) => {
                    if let Some(event) = pipeline.process_with_oracle(inputs, embedding, &oracle) {
                        if let Err(e) = publish_event(&mut publisher, &event) {
                            eprintln!("BFLD publish error: {e:?}");
                        }
                    }
                }
                Err(RecvError) => break,
            }
        });
        Self {
            sender,
            worker: Some(worker),
        }
    }

    /// Enqueue an input. Returns `SendError<PipelineInput>` (carrying the
    /// rejected input) if the worker has already shut down.
    pub fn send(&self, input: PipelineInput) -> Result<(), SendError<PipelineInput>> {
        self.sender.send(input)
    }

    /// Close the input channel and join the worker. Panics from the worker
    /// thread propagate here; otherwise returns cleanly.
    pub fn shutdown(mut self) {
        if let Some(worker) = self.worker.take() {
            drop(std::mem::replace(&mut self.sender, channel().0));
            worker
                .join()
                .expect("BFLD pipeline worker panicked during shutdown");
        }
    }
}

impl Drop for BfldPipelineHandle {
    /// Best-effort cleanup if `shutdown()` was not called explicitly.
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            // Replace the sender with a fresh disconnected one so the worker
            // recv() returns Err(RecvError) and the loop exits.
            drop(std::mem::replace(&mut self.sender, channel().0));
            let _ = worker.join();
        }
    }
}
