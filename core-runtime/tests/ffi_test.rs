//! FFI module integration tests for CORE Runtime.
//!
//! Tests the C FFI layer including error handling, type conversions,
//! and API function signatures. Validates lifecycle-aware model
//! management and text-based inference API.

#![cfg(feature = "ffi")]

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use gg_core::ffi::{
    core_clear_last_error, core_config_default, core_get_last_error,
    core_runtime_create, core_runtime_destroy,
    CoreConfig, CoreErrorCode, CoreHealthReport, CoreHealthState,
    CoreInferenceParams, CoreInferenceResult, CoreModelMetadata,
};

// ============================================================================
// Error Code Tests
// ============================================================================

#[test]
fn test_error_codes_have_correct_values() {
    assert_eq!(CoreErrorCode::Ok as i32, 0);
    assert_eq!(CoreErrorCode::NullPointer as i32, -1);
    assert_eq!(CoreErrorCode::InvalidConfig as i32, -2);
    assert_eq!(CoreErrorCode::AuthFailed as i32, -3);
    assert_eq!(CoreErrorCode::ModelNotFound as i32, -7);
    assert_eq!(CoreErrorCode::ModelLoadFailed as i32, -8);
    assert_eq!(CoreErrorCode::InferenceFailed as i32, -9);
    assert_eq!(CoreErrorCode::InvalidParams as i32, -11);
    assert_eq!(CoreErrorCode::Cancelled as i32, -15);
    assert_eq!(CoreErrorCode::Internal as i32, -99);
}

#[test]
fn test_error_codes_are_copy_and_eq() {
    let code1 = CoreErrorCode::Ok;
    let code2 = code1;
    assert_eq!(code1, code2);
    assert_ne!(CoreErrorCode::Ok, CoreErrorCode::Internal);
}

// ============================================================================
// Error Message Tests
// ============================================================================

#[test]
fn test_get_last_error_returns_null_when_no_error() {
    core_clear_last_error();
    let ptr = core_get_last_error();
    assert!(ptr.is_null());
}

#[test]
fn test_clear_last_error_clears_message() {
    let mut out_runtime: *mut gg_core::ffi::CoreRuntime = ptr::null_mut();
    unsafe {
        core_runtime_create(ptr::null(), &mut out_runtime);
    }
    let ptr = core_get_last_error();
    assert!(!ptr.is_null());

    core_clear_last_error();
    let ptr = core_get_last_error();
    assert!(ptr.is_null());
}

#[test]
fn test_error_message_set_on_null_pointer() {
    core_clear_last_error();
    let mut out_runtime: *mut gg_core::ffi::CoreRuntime = ptr::null_mut();
    let result = unsafe { core_runtime_create(ptr::null(), &mut out_runtime) };

    assert_eq!(result, CoreErrorCode::NullPointer);
    let ptr = core_get_last_error();
    assert!(!ptr.is_null());
    let msg = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
    assert!(msg.contains("null"));
}

// ============================================================================
// CoreConfig Tests
// ============================================================================

#[test]
fn test_config_default_sets_reasonable_values() {
    let mut config = CoreConfig {
        base_path: ptr::null(),
        auth_token: ptr::null(),
        session_timeout_secs: 0,
        max_context_length: 0,
        max_queue_depth: 0,
        shutdown_timeout_secs: 0,
    };

    core_config_default(&mut config);

    assert!(config.base_path.is_null());
    assert!(config.auth_token.is_null());
    assert_eq!(config.session_timeout_secs, 3600);
    assert_eq!(config.max_context_length, 4096);
    assert_eq!(config.max_queue_depth, 1000);
    assert_eq!(config.shutdown_timeout_secs, 30);
}

#[test]
fn test_config_default_handles_null_pointer() {
    core_config_default(ptr::null_mut());
}

// ============================================================================
// CoreInferenceParams Tests
// ============================================================================

#[test]
fn test_inference_params_default_values() {
    let params = CoreInferenceParams::default();
    assert_eq!(params.max_tokens, 256);
    assert!((params.temperature - 0.7).abs() < f32::EPSILON);
    assert!((params.top_p - 0.9).abs() < f32::EPSILON);
    assert_eq!(params.top_k, 40);
    assert!(!params.stream);
    assert_eq!(params.timeout_ms, 0);
}

// ============================================================================
// CoreInferenceResult Tests (text-based v1 API)
// ============================================================================

#[test]
fn test_inference_result_default_values() {
    let result = CoreInferenceResult::default();
    assert!(result.output_text.is_null());
    assert_eq!(result.tokens_generated, 0);
    assert!(!result.finished);
}

// ============================================================================
// Health Tests
// ============================================================================

#[test]
fn test_health_state_values() {
    assert_eq!(CoreHealthState::Healthy as i32, 0);
    assert_eq!(CoreHealthState::Degraded as i32, 1);
    assert_eq!(CoreHealthState::Unhealthy as i32, 2);
}

#[test]
fn test_health_report_default_values() {
    let report = CoreHealthReport::default();
    assert_eq!(report.state, CoreHealthState::Unhealthy);
    assert!(!report.ready);
    assert_eq!(report.models_loaded, 0);
}

// ============================================================================
// CoreModelMetadata Tests
// ============================================================================

#[test]
fn test_model_metadata_default_values() {
    let metadata = CoreModelMetadata::default();
    assert!(metadata.name.is_null());
    assert_eq!(metadata.size_bytes, 0);
    assert_eq!(metadata.handle_id, 0);
}

// ============================================================================
// Runtime Lifecycle Tests
// ============================================================================

#[test]
fn test_runtime_create_rejects_null_config() {
    core_clear_last_error();
    let mut out_runtime: *mut gg_core::ffi::CoreRuntime = ptr::null_mut();
    let result = unsafe { core_runtime_create(ptr::null(), &mut out_runtime) };
    assert_eq!(result, CoreErrorCode::NullPointer);
    assert!(out_runtime.is_null());
}

#[test]
fn test_runtime_create_rejects_null_out_pointer() {
    let config = CoreConfig::default();
    let result = unsafe { core_runtime_create(&config, ptr::null_mut()) };
    assert_eq!(result, CoreErrorCode::NullPointer);
}

#[test]
fn test_runtime_create_requires_auth_token() {
    core_clear_last_error();
    let config = CoreConfig::default();
    let mut out_runtime: *mut gg_core::ffi::CoreRuntime = ptr::null_mut();
    let result = unsafe { core_runtime_create(&config, &mut out_runtime) };

    assert_eq!(result, CoreErrorCode::InvalidConfig);
    let ptr = core_get_last_error();
    assert!(!ptr.is_null());
    let msg = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
    assert!(msg.contains("auth_token"));
}

#[test]
fn test_runtime_destroy_handles_null() {
    unsafe { core_runtime_destroy(ptr::null_mut()) };
}

#[test]
fn test_runtime_create_and_destroy_lifecycle() {
    let auth_token = CString::new("test_token_12345").unwrap();
    let mut config = CoreConfig::default();
    config.auth_token = auth_token.as_ptr();

    let mut out_runtime: *mut gg_core::ffi::CoreRuntime = ptr::null_mut();
    let result = unsafe { core_runtime_create(&config, &mut out_runtime) };

    assert_eq!(result, CoreErrorCode::Ok);
    assert!(!out_runtime.is_null());

    unsafe { core_runtime_destroy(out_runtime) };
}

// ============================================================================
// Model Load/Unload via Lifecycle (null-safety tests)
// ============================================================================

#[test]
fn test_model_load_rejects_null_runtime() {
    core_clear_last_error();
    let path = CString::new("models/test.gguf").unwrap();
    let mut handle_id: u64 = 0;

    let result = unsafe {
        gg_core::ffi::core_model_load(ptr::null_mut(), path.as_ptr(), &mut handle_id)
    };
    assert_eq!(result, CoreErrorCode::NullPointer);
}

#[test]
fn test_model_load_rejects_null_path() {
    core_clear_last_error();
    let auth_token = CString::new("test_token").unwrap();
    let mut config = CoreConfig::default();
    config.auth_token = auth_token.as_ptr();

    let mut rt: *mut gg_core::ffi::CoreRuntime = ptr::null_mut();
    unsafe { core_runtime_create(&config, &mut rt) };
    assert!(!rt.is_null());

    let mut handle_id: u64 = 0;
    let result = unsafe {
        gg_core::ffi::core_model_load(rt, ptr::null(), &mut handle_id)
    };
    assert_eq!(result, CoreErrorCode::NullPointer);

    unsafe { core_runtime_destroy(rt) };
}

#[test]
fn test_model_unload_rejects_null_runtime() {
    let result = unsafe {
        gg_core::ffi::core_model_unload(ptr::null_mut(), 1)
    };
    assert_eq!(result, CoreErrorCode::NullPointer);
}

#[test]
fn test_model_unload_nonexistent_returns_not_found() {
    let auth_token = CString::new("test_token").unwrap();
    let mut config = CoreConfig::default();
    config.auth_token = auth_token.as_ptr();

    let mut rt: *mut gg_core::ffi::CoreRuntime = ptr::null_mut();
    unsafe { core_runtime_create(&config, &mut rt) };

    let result = unsafe {
        gg_core::ffi::core_model_unload(rt, 999)
    };
    assert_eq!(result, CoreErrorCode::ModelNotFound);

    unsafe { core_runtime_destroy(rt) };
}

// ============================================================================
// Model List Tests (fills buffer correctly)
// ============================================================================

#[test]
fn test_model_list_rejects_null_pointers() {
    let result = unsafe {
        gg_core::ffi::core_model_list(ptr::null_mut(), ptr::null_mut(), 10, ptr::null_mut())
    };
    assert_eq!(result, CoreErrorCode::NullPointer);
}

#[test]
fn test_model_list_empty_returns_zero() {
    let auth_token = CString::new("test_token").unwrap();
    let mut config = CoreConfig::default();
    config.auth_token = auth_token.as_ptr();

    let mut rt: *mut gg_core::ffi::CoreRuntime = ptr::null_mut();
    unsafe { core_runtime_create(&config, &mut rt) };

    let mut handles = [0u64; 16];
    let mut count: u32 = 99;
    let result = unsafe {
        gg_core::ffi::core_model_list(rt, handles.as_mut_ptr(), 16, &mut count)
    };

    assert_eq!(result, CoreErrorCode::Ok);
    assert_eq!(count, 0);

    unsafe { core_runtime_destroy(rt) };
}

#[test]
fn test_model_count_returns_zero_initially() {
    let auth_token = CString::new("test_token").unwrap();
    let mut config = CoreConfig::default();
    config.auth_token = auth_token.as_ptr();

    let mut rt: *mut gg_core::ffi::CoreRuntime = ptr::null_mut();
    unsafe { core_runtime_create(&config, &mut rt) };

    let mut count: u32 = 99;
    let result = unsafe {
        gg_core::ffi::core_model_count(rt, &mut count)
    };

    assert_eq!(result, CoreErrorCode::Ok);
    assert_eq!(count, 0);

    unsafe { core_runtime_destroy(rt) };
}

// ============================================================================
// Inference Null-Safety Tests (text-based API)
// ============================================================================

#[test]
fn test_infer_rejects_null_runtime() {
    let model_id = CString::new("test-model").unwrap();
    let prompt = CString::new("Hello").unwrap();
    let mut result = CoreInferenceResult::default();

    let code = unsafe {
        gg_core::ffi::core_infer(
            ptr::null_mut(),
            ptr::null_mut(),
            model_id.as_ptr(),
            prompt.as_ptr(),
            ptr::null(),
            &mut result,
        )
    };
    assert_eq!(code, CoreErrorCode::NullPointer);
}

#[test]
fn test_infer_rejects_null_prompt() {
    let auth_token = CString::new("test_token").unwrap();
    let mut config = CoreConfig::default();
    config.auth_token = auth_token.as_ptr();

    let mut rt: *mut gg_core::ffi::CoreRuntime = ptr::null_mut();
    unsafe { core_runtime_create(&config, &mut rt) };

    // Need a session - but we can test null prompt rejection
    let model_id = CString::new("test-model").unwrap();
    let mut result = CoreInferenceResult::default();

    let code = unsafe {
        gg_core::ffi::core_infer(
            rt,
            ptr::null_mut(), // null session
            model_id.as_ptr(),
            ptr::null(), // null prompt
            ptr::null(),
            &mut result,
        )
    };
    assert_eq!(code, CoreErrorCode::NullPointer);

    unsafe { core_runtime_destroy(rt) };
}

// ============================================================================
// Free Functions Safety Tests
// ============================================================================

#[test]
fn test_free_result_handles_null() {
    unsafe { gg_core::ffi::core_free_result(ptr::null_mut()) };
}

#[test]
fn test_free_result_handles_null_text() {
    let mut result = CoreInferenceResult::default();
    unsafe { gg_core::ffi::core_free_result(&mut result) };
}

#[test]
fn test_free_model_metadata_handles_null() {
    unsafe { gg_core::ffi::core_free_model_metadata(ptr::null_mut()) };
}

#[test]
fn test_free_string_handles_null() {
    unsafe { gg_core::ffi::core_free_string(ptr::null_mut()) };
}

// ============================================================================
// Type Size and Alignment Tests (ABI stability)
// ============================================================================

#[test]
fn test_core_config_is_repr_c() {
    use std::mem::{align_of, size_of};
    assert!(align_of::<CoreConfig>() >= align_of::<*const c_char>());
    assert!(size_of::<CoreConfig>() >= 40);
}

#[test]
fn test_core_error_code_is_i32() {
    use std::mem::size_of;
    assert_eq!(size_of::<CoreErrorCode>(), size_of::<i32>());
}

// ============================================================================
// Authenticated Session + Model Lifecycle Integration
// ============================================================================

/// Helper: create a runtime and authenticate a session
unsafe fn create_runtime_and_session() -> (
    *mut gg_core::ffi::CoreRuntime,
    *mut gg_core::ffi::CoreSession,
    CString,
) {
    let auth_token = CString::new("test_token_lifecycle").unwrap();
    let mut config = CoreConfig::default();
    config.auth_token = auth_token.as_ptr();

    let mut rt: *mut gg_core::ffi::CoreRuntime = ptr::null_mut();
    let result = core_runtime_create(&config, &mut rt);
    assert_eq!(result, CoreErrorCode::Ok);

    let mut session: *mut gg_core::ffi::CoreSession = ptr::null_mut();
    let result = gg_core::ffi::core_authenticate(rt, auth_token.as_ptr(), &mut session);
    assert_eq!(result, CoreErrorCode::Ok);
    assert!(!session.is_null());

    (rt, session, auth_token)
}

/// Helper: destroy runtime and session
unsafe fn cleanup(rt: *mut gg_core::ffi::CoreRuntime, session: *mut gg_core::ffi::CoreSession) {
    gg_core::ffi::core_session_release(session);
    core_runtime_destroy(rt);
}

#[test]
fn test_infer_on_unloaded_model_returns_error() {
    unsafe {
        let (rt, session, _token) = create_runtime_and_session();

        let model_id = CString::new("nonexistent-model").unwrap();
        let prompt = CString::new("Hello world").unwrap();
        let mut result = CoreInferenceResult::default();

        let code = gg_core::ffi::core_infer(
            rt,
            session,
            model_id.as_ptr(),
            prompt.as_ptr(),
            ptr::null(),
            &mut result,
        );

        // Model is not loaded, should return ModelNotFound
        assert_eq!(code, CoreErrorCode::ModelNotFound);

        cleanup(rt, session);
    }
}

#[test]
fn test_session_validate_works() {
    unsafe {
        let (rt, session, _token) = create_runtime_and_session();

        let code = gg_core::ffi::core_session_validate(rt, session);
        assert_eq!(code, CoreErrorCode::Ok);

        let id_ptr = gg_core::ffi::core_session_id(session);
        assert!(!id_ptr.is_null());

        cleanup(rt, session);
    }
}

#[test]
fn test_health_check_on_fresh_runtime() {
    unsafe {
        let (rt, session, _token) = create_runtime_and_session();

        let mut report = CoreHealthReport::default();
        let code = gg_core::ffi::core_health_check(rt, &mut report);
        assert_eq!(code, CoreErrorCode::Ok);
        assert_eq!(report.models_loaded, 0);

        cleanup(rt, session);
    }
}

#[test]
fn test_model_list_and_count_consistent() {
    unsafe {
        let (rt, session, _token) = create_runtime_and_session();

        // count should be 0
        let mut count: u32 = 99;
        let code = gg_core::ffi::core_model_count(rt, &mut count);
        assert_eq!(code, CoreErrorCode::Ok);
        assert_eq!(count, 0);

        // list should also return 0
        let mut handles = [0u64; 16];
        let mut list_count: u32 = 99;
        let code = gg_core::ffi::core_model_list(
            rt,
            handles.as_mut_ptr(),
            16,
            &mut list_count,
        );
        assert_eq!(code, CoreErrorCode::Ok);
        assert_eq!(list_count, 0);

        cleanup(rt, session);
    }
}

#[test]
fn test_model_info_on_nonexistent_handle() {
    unsafe {
        let (rt, session, _token) = create_runtime_and_session();

        let mut meta = CoreModelMetadata::default();
        let code = gg_core::ffi::core_model_info(rt, 999, &mut meta);
        assert_eq!(code, CoreErrorCode::ModelNotFound);

        cleanup(rt, session);
    }
}
