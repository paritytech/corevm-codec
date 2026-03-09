/**
 * This module provides functions to asynchronously load the library.
 *
 * These can be useful inside web workers.
 * @module loader
 */
import { __wbg_set_wasm } from './corevm_codec_bg.js'

/**
 * Initialize the library using WebAssembly instance exports.
 *
 * Use this function to asynchronously initialize the library.
 * This function is a simple setter that doesn't involve WebAssembly compilation.
 *
 * #### Example
 *
 * ```javascript
 * import { IMPORT_OBJECT, initWithExports } from 'corevm-codec/loader'
 * import codeURL from 'corevm-codec/code.wasm?url' // Vite.
 *
 * const result = await WebAssembly.instantiateStreaming(fetch(codeURL), IMPORT_OBJECT)
 * initWithExports(result.instance.exports)
 * ```
 */
export function initWithExports(exports) {
    __wbg_set_wasm(exports)
}
