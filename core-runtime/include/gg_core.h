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

/// PBKDF2 iteration count (600,000 iterations per OWASP 2023 recommendations)
constexpr static const uint32_t ModelEncryption_PBKDF2_ITERATIONS = 600000;

/// Minimum salt size for security (16 bytes = 128 bits)
constexpr static const uintptr_t MIN_SALT_SIZE = 16;

/// Current file format version
constexpr static const uint8_t FORMAT_VERSION = 3;

/// Maximum allowed length for string fields.
constexpr static const uintptr_t MAX_FIELD_LENGTH = 256;

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
  BufferTooSmall = -16,
  SecurityRejected = -17,
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

/// Protocol version for negotiating encoding.
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

/// Authenticate with token, returns session handle.
/// # Safety
/// All pointers must be valid. `token` must be a NUL-terminated C string.
CoreErrorCode core_authenticate(CoreRuntime *runtime, const char *token, CoreSession **out_session);

/// Validate existing session.
/// # Safety
/// `runtime` and `session` must be valid non-null pointers to objects from
/// `core_runtime_create`/`core_authenticate`, live for the duration of the call.
/// The returned `CoreErrorCode` indicates success or the validation failure reason.
CoreErrorCode core_session_validate(CoreRuntime *runtime, CoreSession *session);

/// Release session handle.
/// # Safety
/// `session` must be null or a pointer previously returned by `core_authenticate`
/// and not yet released. After this call the pointer is dangling and must not be
/// used again (double-free is undefined behavior).
void core_session_release(CoreSession *session);

/// Get session ID string (valid until session released).
/// # Safety
/// `session` must be null or a valid pointer from `core_authenticate`. The returned
/// C string pointer borrows from the session and is valid only until the session is
/// released; returns null if `session` is null.
const char *core_session_id(const CoreSession *session);

/// Get the last error message (C API)
const char *core_get_last_error();

/// Clear the last error message (C API)
void core_clear_last_error();

/// Health check (no authentication required).
/// # Safety
/// `runtime` and `out_report` must be valid non-null pointers for the duration of
/// the call; `out_report` must be writable. The `CoreErrorCode` return indicates
/// success or failure.
CoreErrorCode core_health_check(CoreRuntime *runtime, CoreHealthReport *out_report);

/// Liveness check.
/// # Safety
/// `runtime` must be null or a valid pointer from `core_runtime_create`, live for
/// the duration of the call. Returns false if `runtime` is null.
bool core_is_alive(CoreRuntime *runtime);

/// Readiness check.
/// # Safety
/// `runtime` must be null or a valid pointer from `core_runtime_create`, live for
/// the duration of the call. Returns false if `runtime` is null.
bool core_is_ready(CoreRuntime *runtime);

/// Get metrics JSON (free with `core_free_string`).
/// # Safety
/// `runtime` and `out_json` must be valid non-null pointers for the duration of the
/// call; `out_json` must be writable. On success `*out_json` receives an owned C
/// string that the caller must free with `core_free_string`.
CoreErrorCode core_get_metrics_json(CoreRuntime *runtime, char **out_json);

/// Submit inference request (blocking, text-based).
/// # Safety
/// All non-null pointers must be valid. `params` may be null for defaults.
CoreErrorCode core_infer(CoreRuntime *runtime,
                         CoreSession *session,
                         const char *model_id,
                         const char *prompt,
                         const CoreInferenceParams *params,
                         CoreInferenceResult *out_result);

/// Submit inference request with timeout (blocking).
/// # Safety
/// Same as `core_infer`.
CoreErrorCode core_infer_with_timeout(CoreRuntime *runtime,
                                      CoreSession *session,
                                      const char *model_id,
                                      const char *prompt,
                                      const CoreInferenceParams *params,
                                      uint64_t timeout_ms,
                                      CoreInferenceResult *out_result);

/// Free inference result text.
/// # Safety
/// `result` must be null or a valid pointer previously populated by `core_infer`/
/// `core_infer_with_timeout` and not yet freed. After this call the owned `output_text`
/// is dangling and must not be reused (double-free is undefined behavior).
void core_free_result(CoreInferenceResult *result);

/// Inference with caller-provided buffer.
/// # Safety
/// `runtime`, `session`, `model_id`, `prompt`, `out_buf`, and `out_len` must be valid
/// non-null pointers for the duration of the call; `params` may be null for defaults.
/// `model_id` and `prompt` must be valid NUL-terminated C strings; `out_buf` must be
/// writable for `buf_len` bytes and `out_len` writable. The `CoreErrorCode` return
/// indicates success or failure.
CoreErrorCode core_infer_bounded(CoreRuntime *runtime,
                                 CoreSession *session,
                                 const char *model_id,
                                 const char *prompt,
                                 const CoreInferenceParams *params,
                                 uint8_t *out_buf,
                                 uintptr_t buf_len,
                                 uintptr_t *out_len);

/// Load a model via ModelLifecycle.
/// # Safety
/// `runtime`, `model_path`, and `out_handle_id` must be valid non-null pointers for
/// the duration of the call; `model_path` must be a valid NUL-terminated C string and
/// `out_handle_id` must be writable. The `CoreErrorCode` return indicates success or failure.
CoreErrorCode core_model_load(CoreRuntime *runtime,
                              const char *model_path,
                              uint64_t *out_handle_id);

/// Unload a model via ModelLifecycle.
/// # Safety
/// `runtime` must be a valid non-null pointer from `core_runtime_create`, live for the
/// duration of the call. The `CoreErrorCode` return indicates success or failure.
CoreErrorCode core_model_unload(CoreRuntime *runtime, uint64_t handle_id);

/// Get model info.
/// # Safety
/// `runtime` and `out_metadata` must be valid non-null pointers for the duration of the
/// call; `out_metadata` must be writable. On success it is populated with owned fields
/// the caller must free via `core_free_model_metadata`. The `CoreErrorCode` return
/// indicates success or failure.
CoreErrorCode core_model_info(CoreRuntime *runtime,
                              uint64_t handle_id,
                              CoreModelMetadata *out_metadata);

/// Free model metadata.
/// # Safety
/// `metadata` must be null or a valid pointer previously populated by `core_model_info`
/// and not yet freed. After this call the owned fields are dangling and must not be reused.
void core_free_model_metadata(CoreModelMetadata *metadata);

/// List loaded models.
/// # Safety
/// `runtime`, `out_handles`, and `out_count` must be valid non-null pointers for the
/// duration of the call; `out_handles` must point to writable storage for at least
/// `max_count` `u64` values and `out_count` must be writable. The `CoreErrorCode`
/// return indicates success or failure.
CoreErrorCode core_model_list(CoreRuntime *runtime,
                              uint64_t *out_handles,
                              uint32_t max_count,
                              uint32_t *out_count);

/// Get count of loaded models.
/// # Safety
/// `runtime` and `out_count` must be valid non-null pointers for the duration of the
/// call; `out_count` must be writable. The `CoreErrorCode` return indicates success or failure.
CoreErrorCode core_model_count(CoreRuntime *runtime, uint32_t *out_count);

/// Get default configuration values.
/// # Safety
/// `config` must be null or a valid, writable pointer to a `CoreConfig` for the
/// duration of the call. When non-null it is overwritten with default values.
void core_config_default(CoreConfig *config);

/// Create runtime with configuration.
/// # Safety
/// `config` and `out_runtime` must be valid non-null pointers.
CoreErrorCode core_runtime_create(const CoreConfig *config, CoreRuntime **out_runtime);

/// Destroy runtime (blocks until graceful shutdown).
/// # Safety
/// `runtime` must be null or from `core_runtime_create`. Must not be called concurrently.
void core_runtime_destroy(CoreRuntime *runtime);

/// Submit streaming inference (blocks until done/cancelled).
/// # Safety
/// All pointers valid. `callback` must be safe to invoke from any thread.
CoreErrorCode core_infer_streaming(CoreRuntime *runtime,
                                   CoreSession *session,
                                   const char *model_id,
                                   const char *prompt,
                                   const CoreInferenceParams *params,
                                   CoreStreamCallback callback,
                                   void *user_data);

/// Free string allocated by core functions.
/// # Safety
/// `s` must be null or a C string previously returned by a core API (e.g.
/// `core_get_metrics_json`) and not yet freed. After this call the pointer is dangling
/// and must not be used again (double-free is undefined behavior).
void core_free_string(char *s);

} // extern "C"
