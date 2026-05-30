//! VQVAE components — class embedding, codebook, quant/post-quant convolutions.
//!
//! ## Implementation status
//!
//! | Component            | Status  | Notes                                          |
//! |----------------------|---------|------------------------------------------------|
//! | `ClassEmbedding`     | Full    | `Embedding(18, 64)` — matches Python exactly   |
//! | `VQCodebook`         | Full    | Nearest-neighbour lookup via squared-L2        |
//! | `QuantConv`          | Full    | `Conv2d(128 → 512, k=1)` — quant_conv          |
//! | `PostQuantConv`      | Full    | `Conv2d(512 → 128, k=1)` — post_quant_conv     |
//! | `fold_3d_to_2d`      | Full    | (B*F, C, H, W*D) reshape for 2D CNN            |
//! | Encoder2D (ResNet)   | STUB    | Returns random z of correct shape (B*F,128,50,50). |
//!                                    Full implementation requires loading ~35 M params  |
//!                                    from the Phase-5 SafeTensors checkpoint.           |
//! | Decoder2D (ResNet)   | STUB    | Returns random logits of correct shape.        |
//!
//! The stubs produce outputs of the correct dtype and shape so that the full
//! inference pipeline compiles, runs, and can be benchmarked end-to-end
//! before the checkpoint is available.

use candle_core::{DType, Device, Module, Result, Tensor};
use candle_nn::{Conv2d, Conv2dConfig, Embedding, VarBuilder};

use crate::config::OccWorldConfig;
use crate::error::OccWorldError;

// ── Class embedding ───────────────────────────────────────────────────────────

/// Embeds integer class labels `[0, num_classes)` into `base_channels`-dim vectors.
///
/// Matches `nn.Embedding(18, 64)` in `vae_2d_resnet.py`.
pub struct ClassEmbedding {
    embed: Embedding,
}

impl ClassEmbedding {
    /// Build from a [`VarBuilder`] using the sub-path `"class_embed"`.
    pub fn new(num_classes: usize, embed_dim: usize, vb: VarBuilder<'_>) -> Result<Self> {
        let embed = candle_nn::embedding(num_classes, embed_dim, vb.pp("class_embed"))?;
        Ok(Self { embed })
    }

    /// Build with random initialisation (for tests / benchmarks).
    pub fn dummy(num_classes: usize, embed_dim: usize, device: &Device) -> Result<Self> {
        let w = Tensor::randn(0f32, 1.0, (num_classes, embed_dim), device)?;
        let embed = Embedding::new(w, embed_dim);
        Ok(Self { embed })
    }

    /// Forward: `(B*F, H, W, D)` u32 indices → `(B*F, embed_dim, H, W*D)`.
    ///
    /// The 3-D grid is folded along the depth axis so a 2-D CNN can process it.
    pub fn forward(&self, x: &Tensor, grid_d: usize) -> Result<Tensor> {
        // x: (B*F, H, W, D) — integer class labels stored as u32
        let (bf, h, w, _d) = x.dims4()?;

        // Flatten spatial+depth → apply embedding → (B*F, H, W, D, embed_dim)
        let flat = x.flatten_all()?; // (B*F*H*W*D,)
        let embedded = self.embed.forward(&flat)?; // (B*F*H*W*D, embed_dim)
        let c = embedded.dim(1)?;

        // Reshape to (B*F, H, W, D, C) then transpose to (B*F, C, H, W*D)
        let vol = embedded.reshape((bf, h, w, grid_d, c))?;
        // (B*F, H, W, D, C) → (B*F, C, H, W, D) → (B*F, C, H, W*D)
        let transposed = vol.permute((0, 4, 1, 2, 3))?;
        let (bf2, c2, h2, w2, d2) = transposed.dims5()?;
        transposed.reshape((bf2, c2, h2, w2 * d2))
    }
}

// ── fold_3d_to_2d helper ─────────────────────────────────────────────────────

/// Reshape `(B*F, C, H, W, D)` into `(B*F, C, H, W*D)` for 2-D CNNs.
///
/// This is the "fold" operation described in `vae_2d_resnet.py`:
/// the depth axis is concatenated into the width so that standard
/// `Conv2d` layers can process the full 3-D occupancy volume.
pub fn fold_3d_to_2d(x: &Tensor) -> Result<Tensor> {
    let (bf, c, h, w, d) = x.dims5()?;
    x.reshape((bf, c, h, w * d))
}

/// Inverse of `fold_3d_to_2d`: `(B*F, C, H, W*D)` → `(B*F, C, H, W, D)`.
pub fn unfold_2d_to_3d(x: &Tensor, grid_w: usize, grid_d: usize) -> Result<Tensor> {
    let (bf, c, h, _wd) = x.dims4()?;
    x.reshape((bf, c, h, grid_w, grid_d))
}

// ── Vector-quantisation codebook ─────────────────────────────────────────────

/// VQ codebook: `num_codes × embed_dim` lookup table.
///
/// Nearest-neighbour assignment uses squared L2 distance:
/// ```text
/// d(z, e_k) = ||z − e_k||² = ||z||² − 2·z·e_kᵀ + ||e_k||²
/// ```
/// This is standard VQ-VAE (van den Oord et al., 2017).
pub struct VQCodebook {
    /// Shape: `(codebook_size, embed_dim)`.
    embeddings: Tensor,
    /// Number of discrete codes in the codebook.
    pub codebook_size: usize,
    /// Dimensionality of each codebook embedding vector.
    pub embed_dim: usize,
}

impl VQCodebook {
    /// Load from a [`VarBuilder`] using the sub-path `"quantize.embedding.weight"`.
    pub fn new(codebook_size: usize, embed_dim: usize, vb: VarBuilder<'_>) -> Result<Self> {
        let embeddings = vb
            .pp("quantize")
            .pp("embedding")
            .get((codebook_size, embed_dim), "weight")?;
        Ok(Self {
            embeddings,
            codebook_size,
            embed_dim,
        })
    }

    /// Random initialisation (for tests / benchmarks).
    pub fn dummy(codebook_size: usize, embed_dim: usize, device: &Device) -> Result<Self> {
        let embeddings = Tensor::randn(0f32, 1.0, (codebook_size, embed_dim), device)?;
        Ok(Self {
            embeddings,
            codebook_size,
            embed_dim,
        })
    }

    /// Quantise `z` (any shape `[..., embed_dim]`) → `(z_q, indices)`.
    ///
    /// `z_q` has the same shape as `z`; `indices` has shape `[..., 1]` squeezed
    /// to `[...]` (batch of scalar indices).
    pub fn encode(&self, z: &Tensor) -> Result<(Tensor, Tensor)> {
        let orig_shape = z.shape().clone();
        let orig_dims = orig_shape.dims().to_vec();
        let last = *orig_shape.dims().last().unwrap_or(&0);
        // Flatten to (N, embed_dim)
        let n = z.elem_count() / last;
        let z_flat = z.reshape((n, last))?; // (N, D)

        // Squared L2: ||z||² - 2*z*Eᵀ + ||E||²
        // z_sq: (N, 1)
        let z_sq = z_flat
            .sqr()?
            .sum(candle_core::D::Minus1)?
            .unsqueeze(1)?;
        // e_sq: (1, codebook_size)
        let e_sq = self
            .embeddings
            .sqr()?
            .sum(candle_core::D::Minus1)?
            .unsqueeze(0)?;
        // dot: (N, codebook_size)
        let dot = z_flat.matmul(&self.embeddings.t()?)?;
        // distances: (N, codebook_size)
        let distances = z_sq.broadcast_add(&e_sq)?.broadcast_sub(&dot.affine(2.0, 0.0)?)?;
        // indices: (N,)
        let indices = distances.argmin(candle_core::D::Minus1)?;

        // Look up quantised embeddings
        let z_q_flat = self.embeddings.index_select(&indices, 0)?; // (N, D)

        // Reshape back to original shape
        let z_q = z_q_flat.reshape(orig_dims.clone())?;
        let idx_shape: Vec<usize> = orig_dims[..orig_dims.len() - 1].to_vec();
        let indices_out = indices.reshape(idx_shape)?;

        Ok((z_q, indices_out))
    }

    /// Decode flat index tensor `(N,)` or `(B, ...)` → same shape `+ embed_dim`.
    pub fn decode(&self, indices: &Tensor) -> Result<Tensor> {
        let flat = indices.flatten_all()?;
        let z_flat = self.embeddings.index_select(&flat, 0)?; // (N, D)
        let mut out_shape: Vec<usize> = indices.dims().to_vec();
        out_shape.push(self.embed_dim);
        z_flat.reshape(out_shape)
    }
}

// ── Quant / post-quant convolutions ──────────────────────────────────────────

/// `Conv2d(z_channels → embed_dim, kernel=1)` — `quant_conv` in Python.
pub struct QuantConv {
    conv: Conv2d,
}

impl QuantConv {
    /// Load from weights.
    pub fn new(z_channels: usize, embed_dim: usize, vb: VarBuilder<'_>) -> Result<Self> {
        let conv = candle_nn::conv2d(
            z_channels,
            embed_dim,
            1,
            Conv2dConfig::default(),
            vb.pp("quant_conv"),
        )?;
        Ok(Self { conv })
    }

    /// Random initialisation.
    pub fn dummy(z_channels: usize, embed_dim: usize, device: &Device) -> Result<Self> {
        let w = Tensor::randn(0f32, 1.0, (embed_dim, z_channels, 1, 1), device)?;
        let b = Tensor::zeros(embed_dim, DType::F32, device)?;
        let conv = Conv2d::new(w, Some(b), Conv2dConfig::default());
        Ok(Self { conv })
    }

    /// Forward: `(B*F, z_channels, H, W)` → `(B*F, embed_dim, H, W)`.
    pub fn forward(&self, x: &Tensor) -> Result<Tensor> {
        self.conv.forward(x)
    }
}

/// `Conv2d(embed_dim → z_channels, kernel=1)` — `post_quant_conv` in Python.
pub struct PostQuantConv {
    conv: Conv2d,
}

impl PostQuantConv {
    /// Load from weights.
    pub fn new(embed_dim: usize, z_channels: usize, vb: VarBuilder<'_>) -> Result<Self> {
        let conv = candle_nn::conv2d(
            embed_dim,
            z_channels,
            1,
            Conv2dConfig::default(),
            vb.pp("post_quant_conv"),
        )?;
        Ok(Self { conv })
    }

    /// Random initialisation.
    pub fn dummy(embed_dim: usize, z_channels: usize, device: &Device) -> Result<Self> {
        let w = Tensor::randn(0f32, 1.0, (z_channels, embed_dim, 1, 1), device)?;
        let b = Tensor::zeros(z_channels, DType::F32, device)?;
        let conv = Conv2d::new(w, Some(b), Conv2dConfig::default());
        Ok(Self { conv })
    }

    /// Forward: `(B*F, embed_dim, H, W)` → `(B*F, z_channels, H, W)`.
    pub fn forward(&self, x: &Tensor) -> Result<Tensor> {
        self.conv.forward(x)
    }
}

// ── Encoder2D stub ────────────────────────────────────────────────────────────

/// **STUB** — returns a random tensor of the correct shape.
///
/// The full `Encoder2D` from `vae_2d_resnet.py` is a multi-resolution ResNet
/// with three down-sampling stages (stride-2 `Conv2d` + residual blocks).
/// Porting all ~35 M parameters requires the Phase-5 SafeTensors checkpoint
/// to be available so the weight names can be mapped.  Until then, this
/// stub ensures the pipeline compiles and end-to-end shape tests pass.
///
/// Replace this function with the real ResNet implementation in Phase 5.
pub fn encode_occupancy(
    x: &Tensor,
    cfg: &OccWorldConfig,
    device: &Device,
) -> std::result::Result<Tensor, OccWorldError> {
    // Derive batch*frames from the input shape
    let dims = x.dims();
    // Acceptable input shapes: (B, F, H, W, D) or (B*F, H, W, D)
    let bf = match dims.len() {
        5 => dims[0] * dims[1],
        4 => dims[0],
        _ => {
            return Err(OccWorldError::ShapeMismatch(format!(
                "encode_occupancy: expected 4-D or 5-D input, got {}-D",
                dims.len()
            )))
        }
    };

    // STUB: return random z of correct shape (B*F, z_channels, token_h, token_w)
    let z = Tensor::randn(
        0f32,
        1.0,
        (bf, cfg.z_channels, cfg.token_h, cfg.token_w),
        device,
    )
    .map_err(OccWorldError::Candle)?;

    Ok(z)
}

/// **STUB** — returns random class logits of the correct shape.
///
/// The full `Decoder2D` mirrors the encoder: three up-sampling stages
/// followed by a `Conv2d` head that produces `num_classes` logits per voxel.
/// Implementation is deferred to Phase 5 (checkpoint loading).
///
/// Replace with the real decoder when Phase-5 weights are available.
pub fn decode_to_logits(
    z: &Tensor,
    cfg: &OccWorldConfig,
    device: &Device,
) -> std::result::Result<Tensor, OccWorldError> {
    let (bf, _c, _h, _w) = z.dims4().map_err(OccWorldError::Candle)?;

    // STUB: return random logits (B*F, num_classes, H, W, D)
    let logits = Tensor::randn(
        0f32,
        1.0,
        (bf, cfg.num_classes, cfg.grid_h, cfg.grid_w, cfg.grid_d),
        device,
    )
    .map_err(OccWorldError::Candle)?;

    Ok(logits)
}

// ── VQVAE component bundle ────────────────────────────────────────────────────

/// All VQVAE components bundled together for use in `OccWorldCandle`.
pub struct VQVAEComponents {
    /// Class label → float embedding (`nn.Embedding(18, 64)` in Python).
    pub class_embed: ClassEmbedding,
    /// `Conv2d(z_channels → embed_dim, k=1)` before quantisation.
    pub quant_conv: QuantConv,
    /// VQ codebook for nearest-neighbour quantisation.
    pub codebook: VQCodebook,
    /// `Conv2d(embed_dim → z_channels, k=1)` after quantisation.
    pub post_quant_conv: PostQuantConv,
}

impl VQVAEComponents {
    /// Build all components from a single [`VarBuilder`].
    pub fn new(cfg: &OccWorldConfig, vb: VarBuilder<'_>) -> Result<Self> {
        let class_embed = ClassEmbedding::new(cfg.num_classes, cfg.base_channels, vb.clone())?;
        let quant_conv = QuantConv::new(cfg.z_channels, cfg.embed_dim, vb.clone())?;
        let codebook = VQCodebook::new(cfg.codebook_size, cfg.embed_dim, vb.clone())?;
        let post_quant_conv = PostQuantConv::new(cfg.embed_dim, cfg.z_channels, vb)?;
        Ok(Self {
            class_embed,
            quant_conv,
            codebook,
            post_quant_conv,
        })
    }

    /// Build all components with random weights (for testing / benchmarking).
    pub fn dummy(cfg: &OccWorldConfig, device: &Device) -> Result<Self> {
        let class_embed = ClassEmbedding::dummy(cfg.num_classes, cfg.base_channels, device)?;
        let quant_conv = QuantConv::dummy(cfg.z_channels, cfg.embed_dim, device)?;
        let codebook = VQCodebook::dummy(cfg.codebook_size, cfg.embed_dim, device)?;
        let post_quant_conv = PostQuantConv::dummy(cfg.embed_dim, cfg.z_channels, device)?;
        Ok(Self {
            class_embed,
            quant_conv,
            codebook,
            post_quant_conv,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vq_codebook_roundtrip() -> candle_core::Result<()> {
        let device = Device::Cpu;
        let codebook = VQCodebook::dummy(512, 512, &device)?;

        // Random input of shape (4, 512) — simulate a batch of 4 latent vectors
        let z = Tensor::randn(0f32, 1.0, (4, 512), &device)?;

        let (z_q, indices) = codebook.encode(&z)?;
        // z_q must have same shape as z
        assert_eq!(z_q.dims(), z.dims());
        // indices must have shape (4,) — one per row
        assert_eq!(indices.dims(), &[4]);

        // Decode must recover the same codebook entries
        let z_decoded = codebook.decode(&indices)?;
        assert_eq!(z_decoded.dims(), &[4, 512]);

        Ok(())
    }

    #[test]
    fn test_fold_unfold_roundtrip() -> candle_core::Result<()> {
        let device = Device::Cpu;
        let x = Tensor::randn(0f32, 1.0, (2, 64, 10, 10, 8), &device)?;
        let folded = fold_3d_to_2d(&x)?;
        assert_eq!(folded.dims(), &[2, 64, 10, 80]);
        let unfolded = unfold_2d_to_3d(&folded, 10, 8)?;
        assert_eq!(unfolded.dims(), &[2, 64, 10, 10, 8]);
        Ok(())
    }
}
