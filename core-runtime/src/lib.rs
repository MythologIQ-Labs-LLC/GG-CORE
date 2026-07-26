//! GG-CORE - Greatest Good - Contained Offline Restricted Execution
//!
//! A sandboxed, offline inference engine that performs model execution only.
//! No authority over data, tools, or system actions.
//!
//! # Philosophy
//!
//! Built on triage principles ("Greatest Good for the Greatest Number").
//! Resource-aware AI that prioritizes system stability over individual request ego.
//!
//! # Design Principles (C.O.R.E.)
//!
//! - **Contained**: Sandbox with no ambient privileges
//! - **Offline**: Zero network access (inbound/outbound blocked)
//! - **Restricted**: IPC-only communication with authenticated callers
//! - **Execution**: Pure compute, no business logic or decision authority
//!
//! # Security Boundaries
//!
//! - Process: Separate OS process, restricted user
//! - Filesystem: Read `models/`, `tokenizers/`. Write `temp/`, `cache/`.
//! - Network: Blocked (deny all)
//! - IPC: Named pipes/Unix sockets only. No HTTP/REST/WebSocket.

pub mod config;
pub mod engine;
pub mod health;
pub mod ipc;
pub mod memory;
pub mod models;
pub mod sandbox;
pub mod scheduler;
pub mod security;
pub mod shutdown;
pub mod telemetry;

// A/B testing module (v0.5.0)
pub mod ab_testing;

// Kubernetes types (v0.5.0)
pub mod k8s;

// CLI module for health probes (v0.5.0)
pub mod cli;

// Deployment automation (v0.6.0)
pub mod deployment;

// Request shim interface (v0.8.0)
// Extension point for commercial multi-tenant features (GG-CORE Nexus)
pub mod shim;

// C FFI module (v0.3.1)
#[cfg(feature = "ffi")]
pub mod ffi;

// Python bindings module (v0.3.1)
#[cfg(feature = "python")]
pub mod python;

// Secure inference façade + Runtime construction helpers (v0.8.x)
mod runtime_facade;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use engine::gpu::GpuConfig as EngineGpuConfig;
use engine::gpu_manager::GpuManager;
use engine::InferenceEngine;
use health::{HealthChecker, HealthConfig};
use ipc::{ConnectionConfig, ConnectionPool, IpcHandler, IpcServerConfig};
use memory::{
    ContextCache, ContextCacheConfig, GpuMemory, GpuMemoryConfig, MemoryPool, MemoryPoolConfig,
    ResourceLimits, ResourceLimitsConfig,
};
use models::{ModelLifecycle, ModelLoader, ModelRegistry, SmartLoader, SmartLoaderConfig};
use runtime_facade::build_loader_callback;
use scheduler::{
    BatchConfig, BatchProcessor, OutputCache, OutputCacheConfig, RequestQueue, RequestQueueConfig,
};
use security::SecurityPipeline;
use shutdown::ShutdownCoordinator;
use telemetry::MetricsStore;
use tokio::sync::Mutex;

/// Runtime configuration.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub base_path: PathBuf,
    pub auth_token: String,
    pub session_timeout: Duration,
    pub max_context_length: usize,
    pub memory_pool: MemoryPoolConfig,
    pub gpu_memory: GpuMemoryConfig,
    pub context_cache: ContextCacheConfig,
    pub request_queue: RequestQueueConfig,
    pub resource_limits: ResourceLimitsConfig,
    pub batch: BatchConfig,
    pub shutdown_timeout: Duration,
    pub output_cache: OutputCacheConfig,
    pub connections: ConnectionConfig,
    pub ipc_server: IpcServerConfig,
    pub gpu: EngineGpuConfig,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            base_path: PathBuf::from("."),
            auth_token: String::new(),
            session_timeout: Duration::from_secs(3600),
            max_context_length: 4096,
            memory_pool: MemoryPoolConfig::default(),
            gpu_memory: GpuMemoryConfig::default(),
            context_cache: ContextCacheConfig::default(),
            request_queue: RequestQueueConfig::default(),
            resource_limits: ResourceLimitsConfig::default(),
            batch: BatchConfig::default(),
            shutdown_timeout: Duration::from_secs(30),
            output_cache: OutputCacheConfig::default(),
            connections: ConnectionConfig::default(),
            ipc_server: IpcServerConfig::default(),
            gpu: EngineGpuConfig::default(),
        }
    }
}

/// The CORE Runtime instance.
pub struct Runtime {
    pub config: RuntimeConfig,
    pub memory_pool: MemoryPool,
    pub gpu_memory: GpuMemory,
    pub context_cache: ContextCache,
    pub model_loader: ModelLoader,
    pub model_registry: Arc<ModelRegistry>,
    pub inference_engine: Arc<InferenceEngine>,
    pub model_lifecycle: Arc<ModelLifecycle>,
    pub smart_loader: Arc<SmartLoader>,
    pub request_queue: Arc<RequestQueue>,
    pub batch_processor: BatchProcessor,
    pub resource_limits: ResourceLimits,
    pub ipc_handler: IpcHandler,
    pub gpu_manager: Option<GpuManager>,
    pub shutdown: Arc<ShutdownCoordinator>,
    pub health: Arc<HealthChecker>,
    pub metrics_store: Arc<MetricsStore>,
    pub output_cache: Arc<Mutex<OutputCache>>,
    pub connections: Arc<ConnectionPool>,
    /// Shared security pipeline enforcing ingress/egress on the façade and the
    /// scheduler worker (single-source config, built once in `new`).
    pub security: Arc<SecurityPipeline>,
}

impl Runtime {
    /// Create a new runtime instance with the given configuration.
    pub fn new(config: RuntimeConfig) -> Self {
        let (memory_pool, gpu_memory, context_cache) = Self::init_memory(&config);
        let model_loader = ModelLoader::new(config.base_path.clone());
        let model_registry = Arc::new(ModelRegistry::new());
        let inference_engine = Arc::new(InferenceEngine::new(config.max_context_length));
        let model_lifecycle = Arc::new(ModelLifecycle::new(
            model_registry.clone(),
            Arc::clone(&inference_engine),
        ));
        let smart_loader = Arc::new(SmartLoader::new(
            SmartLoaderConfig::default(),
            build_loader_callback(model_registry.clone()),
        ));
        let (request_queue, batch_processor, resource_limits, output_cache) =
            Self::init_scheduler(&config);
        let shutdown = Arc::new(ShutdownCoordinator::new());
        let health = Arc::new(HealthChecker::new(HealthConfig::default()));
        let metrics_store = Arc::new(MetricsStore::new());
        let connections = Arc::new(ConnectionPool::new(config.connections.clone()));
        let gpu_manager = GpuManager::new(config.gpu.clone()).ok();
        let security = Arc::new(SecurityPipeline::from_env());
        let ipc_handler = Self::init_ipc(
            &config,
            &request_queue,
            &shutdown,
            &health,
            &model_registry,
            &metrics_store,
            &inference_engine,
        );

        Self {
            config,
            memory_pool,
            gpu_memory,
            context_cache,
            model_loader,
            model_registry,
            inference_engine,
            model_lifecycle,
            smart_loader,
            request_queue,
            batch_processor,
            resource_limits,
            ipc_handler,
            gpu_manager,
            shutdown,
            health,
            metrics_store,
            output_cache,
            connections,
            security,
        }
    }
}
