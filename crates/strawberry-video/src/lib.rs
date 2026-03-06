//! # Strawberry Video
//!
//! Video decode/encode service using FFmpeg subprocess.
//!
//! This approach is used by production NLEs (DaVinci, Premiere) because:
//! - No Rust FFI/binding compatibility issues
//! - Full FFmpeg codec support
//! - Easy to update FFmpeg version
//! - LGPL compliant (subprocess doesn't infect our license)

pub mod cache;
pub mod decode;
pub mod encode;
pub mod ffmpeg;
pub mod jpeg_cache;
pub mod pipe_decoder;
pub mod prefetcher;
pub mod proxy;
pub mod service;
pub mod thumbnail;
pub mod worker;

pub use decode::{DecodeError, VideoDecoder, VideoFrame};
pub use encode::VideoEncoder;
pub use ffmpeg::VideoMetadata;
pub use jpeg_cache::{CachedFrame, JpegCacheKey, JpegFrameCache};
pub use pipe_decoder::PipeDecoder;
pub use prefetcher::{FramePrefetcher, PrefetchRequest};
pub use proxy::{ProxyConfig, ProxyJob, ProxyJobStatus, ProxyManager, ProxyResult};
pub use service::{CacheStats, FrameStatus, LoadedVideo, VideoService};
pub use thumbnail::{get_lod_for_zoom, ThumbnailLod, ThumbnailManager, ThumbnailManifest};
pub use worker::DecodeWorker;
