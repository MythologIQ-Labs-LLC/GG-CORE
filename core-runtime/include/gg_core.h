#include <cstdarg>
#include <cstdint>
#include <cstdlib>
#include <ostream>
#include <new>

/// Maximum text input size in bytes (64KB).
constexpr static const uintptr_t MAX_TEXT_BYTES = 65536;

/// Maximum batch size for batch operations.
constexpr static const uintptr_t MAX_BATCH_SIZE = 32;

/// Maximum token count per input.
constexpr static const uintptr_t MAX_INPUT_TOKENS = 4096;

/// Block size for quantized formats.
constexpr static const uintptr_t QUANT_BLOCK_SIZE = 32;

/// Tokens stored per page (vLLM standard).
constexpr static const uintptr_t PAGE_TOKENS = 16;

/// Maximum history entries per model (default).
constexpr static const uintptr_t DEFAULT_MAX_HISTORY = 10;

constexpr static const uint16_t LD = 0;

constexpr static const uint16_t LDX = 1;

constexpr static const uint16_t ST = 2;

constexpr static const uint16_t STX = 3;

constexpr static const uint16_t ALU = 4;

constexpr static const uint16_t JMP = 5;

constexpr static const uint16_t RET = 6;

constexpr static const uint16_t MISC = 7;

constexpr static const uint16_t W = 0;

constexpr static const uint16_t H = 8;

constexpr static const uint16_t B = 16;

constexpr static const uint16_t DW = 24;

constexpr static const uint16_t IMM = 0;

constexpr static const uint16_t ABS = 32;

constexpr static const uint16_t IND = 64;

constexpr static const uint16_t MEM = 96;

constexpr static const uint16_t LEN = 128;

constexpr static const uint16_t MSH = 160;

constexpr static const uint16_t K = 0;

constexpr static const uint16_t X = 8;

constexpr static const uint16_t JA = 0;

constexpr static const uint16_t JEQ = 16;

constexpr static const uint16_t JGT = 32;

constexpr static const uint16_t JGE = 48;

constexpr static const uint16_t JSET = 64;

/// Encryption key size (256 bits)
constexpr static const uintptr_t KEY_SIZE = 32;

/// Nonce size (96 bits for GCM)
constexpr static const uintptr_t NONCE_SIZE = 12;

/// Tag size (128 bits)
constexpr static const uintptr_t TAG_SIZE = 16;

/// Block size
constexpr static const uintptr_t BLOCK_SIZE = 16;

/// Minimum salt size for security (16 bytes = 128 bits)
constexpr static const uintptr_t MIN_SALT_SIZE = 16;

/// Current file format version
constexpr static const uint8_t FORMAT_VERSION = 3;

/// Exit codes for health probes.
constexpr static const int32_t EXIT_HEALTHY = 0;

constexpr static const int32_t EXIT_UNHEALTHY = 1;

/// Error codes for FFI functions
enum class CoreErrorCode : int32_t {
  Ok = 0,
  NullPointer = -1,
  InvalidConfig = -2,
  AuthFailed = -3,
  SessionExpired = -4,
  SessionNotFound = -5,
  RateLimited = -6,
  ModelNotFound = -7,
  ModelLoadFailed = -8,
  InferenceFailed = -9,
  ContextExceeded = -10,
  InvalidParams = -11,
  QueueFull = -12,
  ShuttingDown = -13,
  Timeout = -14,
  Cancelled = -15,
  Internal = -99,
};

/// Health state enumeration
enum class CoreHealthState {
  Healthy = 0,
  Degraded = 1,
  Unhealthy = 2,
};

/// Opaque handle wrapping Rust runtime
struct CoreRuntime;

/// Session handle with reference counting
struct CoreSession;

/// Protocol version for negotiating encoding strategies.
struct ProtocolVersion;

/// Health check report
struct CoreHealthReport {
  /// Overall health state
  CoreHealthState state;
  /// Ready to accept requests
  bool ready;
  /// Currently accepting requests
  bool accepting_requests;
  /// Number of models loaded
  uint32_t models_loaded;
  /// Memory used in bytes
  uint64_t memory_used_bytes;
  /// Current queue depth
  uint32_t queue_depth;
  /// Uptime in seconds
  uint64_t uptime_secs;
};

/// Inference parameters (matches InferenceParams)
struct CoreInferenceParams {
  /// Maximum tokens to generate (default: 256)
  uint32_t max_tokens;
  /// Temperature for sampling (default: 0.7)
  float temperature;
  /// Top-p (nucleus) sampling (default: 0.9)
  float top_p;
  /// Top-k sampling (default: 40)
  uint32_t top_k;
  /// Enable streaming output (default: false)
  bool stream;
  /// Timeout in milliseconds (0 = no timeout)
  uint64_t timeout_ms;
};

/// Inference result (for non-streaming)
struct CoreInferenceResult {
  /// Generated text (caller must free with core_free_string)
  char *output_text;
  /// Number of tokens generated
  uint32_t tokens_generated;
  /// Whether generation finished normally
  bool finished;
};

/// Model metadata
struct CoreModelMetadata {
  /// Model name (borrowed, valid until model unloaded)
  const char *name;
  /// Model size in bytes
  uint64_t size_bytes;
  /// Model handle ID
  uint64_t handle_id;
};

/// Runtime configuration (C-compatible struct)
struct CoreConfig {
  /// Base path for models directory (NULL = current directory)
  const char *base_path;
  /// Authentication token (required, non-NULL)
  const char *auth_token;
  /// Session timeout in seconds (default: 3600)
  uint64_t session_timeout_secs;
  /// Maximum context length (default: 4096)
  uint32_t max_context_length;
  /// Maximum queue depth (default: 1000)
  uint32_t max_queue_depth;
  /// Shutdown timeout in seconds (default: 30)
  uint64_t shutdown_timeout_secs;
};

/// Streaming callback signature
/// Return false to cancel streaming
using CoreStreamCallback = bool(*)(void *user_data,
                                   const char *text,
                                   bool is_final,
                                   const char *error);





extern "C" {

/// Authenticate with token, returns session handle
CoreErrorCode core_authenticate(CoreRuntime *runtime, const char *token, CoreSession **out_session);

/// Validate existing session
CoreErrorCode core_session_validate(CoreRuntime *runtime, CoreSession *session);

/// Release session handle
void core_session_release(CoreSession *session);

/// Get session ID string (borrowed pointer, valid until session released)
const char *core_session_id(const CoreSession *session);

/// Get the last error message (C API)
const char *core_get_last_error();

/// Clear the last error message (C API)
void core_clear_last_error();

/// Health check (no authentication required)
CoreErrorCode core_health_check(CoreRuntime *runtime, CoreHealthReport *out_report);

/// Liveness check (simple boolean)
bool core_is_alive(CoreRuntime *runtime);

/// Readiness check (simple boolean)
bool core_is_ready(CoreRuntime *runtime);

/// Get metrics as JSON string (caller must free with core_free_string)
CoreErrorCode core_get_metrics_json(CoreRuntime *runtime, char **out_json);

/// Submit inference request (blocking, text-based)
CoreErrorCode core_infer(CoreRuntime *runtime,
                         CoreSession *session,
                         const char *model_id,
                         const char *prompt,
                         const CoreInferenceParams *params,
                         CoreInferenceResult *out_result);

/// Submit inference request with timeout (blocking)
CoreErrorCode core_infer_with_timeout(CoreRuntime *runtime,
                                      CoreSession *session,
                                      const char *model_id,
                                      const char *prompt,
                                      const CoreInferenceParams *params,
                                      uint64_t timeout_ms,
                                      CoreInferenceResult *out_result);

/// Free inference result text (caller must call after consuming)
void core_free_result(CoreInferenceResult *result);

/// Load a model via ModelLifecycle (atomic registry + engine)
CoreErrorCode core_model_load(CoreRuntime *runtime,
                              const char *model_path,
                              uint64_t *out_handle_id);

/// Unload a model via ModelLifecycle (atomic)
CoreErrorCode core_model_unload(CoreRuntime *runtime, uint64_t handle_id);

/// Get model info
CoreErrorCode core_model_info(CoreRuntime *runtime,
                              uint64_t handle_id,
                              CoreModelMetadata *out_metadata);

/// Free model metadata
void core_free_model_metadata(CoreModelMetadata *metadata);

/// List all loaded models (fills out_handles buffer)
CoreErrorCode core_model_list(CoreRuntime *runtime,
                              uint64_t *out_handles,
                              uint32_t max_count,
                              uint32_t *out_count);

/// Get count of loaded models
CoreErrorCode core_model_count(CoreRuntime *runtime, uint32_t *out_count);

/// Get default configuration values
void core_config_default(CoreConfig *config);

/// Create runtime with configuration
CoreErrorCode core_runtime_create(const CoreConfig *config, CoreRuntime **out_runtime);

/// Destroy runtime (blocks until graceful shutdown)
void core_runtime_destroy(CoreRuntime *runtime);

/// Submit streaming inference request (blocks until complete/cancelled)
CoreErrorCode core_infer_streaming(CoreRuntime *runtime,
                                   CoreSession *session,
                                   const char *model_id,
                                   const char *prompt,
                                   const CoreInferenceParams *params,
                                   CoreStreamCallback callback,
                                   void *user_data);

/// Free string allocated by core functions
void core_free_string(char *s);

} // extern "C"
