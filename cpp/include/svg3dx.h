#ifndef SVG3DX_H
#define SVG3DX_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct Svg3dxResult Svg3dxResult;
typedef struct Svg3dxSession Svg3dxSession;

/** Parse + evaluate UTF-8 svg3dx document. Caller must svg3dx_free_result. */
Svg3dxResult *svg3dx_parse(const uint8_t *input, size_t input_len);

/**
 * Parse with external params JSON object (string keys) and optional model id.
 * `params_json` may be NULL or "{}"; `model` may be NULL.
 */
Svg3dxResult *svg3dx_parse_with_params(
    const uint8_t *input,
    size_t input_len,
    const char *params_json,
    const char *model);

bool svg3dx_result_ok(const Svg3dxResult *result);

/** NUL-terminated JSON bytes (EvaluationResult, UpdateResult, or {"error":"..."}). */
const char *svg3dx_result_json(const Svg3dxResult *result);

/** Byte length of JSON without trailing NUL. */
size_t svg3dx_result_len(const Svg3dxResult *result);

size_t svg3dx_result_warning_count(const Svg3dxResult *result);

void svg3dx_free_result(Svg3dxResult *result);

/**
 * Open a document session (parse once). On success returns session and writes the first
 * UpdateResult JSON into *out_update (may be NULL if caller does not need it).
 * Caller frees session with svg3dx_session_free and update with svg3dx_free_result.
 */
Svg3dxSession *svg3dx_session_open(
    const uint8_t *input,
    size_t input_len,
    Svg3dxResult **out_update);

/** Merge external params JSON object and re-evaluate. Returns UpdateResult JSON. */
Svg3dxResult *svg3dx_session_patch_params(Svg3dxSession *session, const char *params_json);

/** Replace external params JSON object and re-evaluate. Returns UpdateResult JSON. */
Svg3dxResult *svg3dx_session_set_params(Svg3dxSession *session, const char *params_json);

/** Select active s3dx:model by id (NULL clears override). Returns UpdateResult JSON. */
Svg3dxResult *svg3dx_session_set_model(Svg3dxSession *session, const char *model);

/** Re-parse document text and re-evaluate. Returns UpdateResult JSON. */
Svg3dxResult *svg3dx_session_reload(
    Svg3dxSession *session,
    const uint8_t *input,
    size_t input_len);

/** Force re-evaluate with current params/model. Returns UpdateResult JSON. */
Svg3dxResult *svg3dx_session_evaluate(Svg3dxSession *session);

uint64_t svg3dx_session_revision(const Svg3dxSession *session);

void svg3dx_session_free(Svg3dxSession *session);

#ifdef __cplusplus
}
#endif

#endif /* SVG3DX_H */
