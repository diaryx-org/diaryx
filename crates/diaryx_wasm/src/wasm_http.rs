//! WASM HTTP client using `web_sys::fetch`.
//!
//! Implements `diaryx_sync::share_session::HttpClient` for use in the
//! WASM worker environment (WorkerGlobalScope).

use diaryx_sync::share_session::{HttpClient, HttpResponse};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// HTTP client using WorkerGlobalScope.fetch().
pub struct WasmHttpClient;

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl HttpClient for WasmHttpClient {
    async fn request(
        &self,
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponse, String> {
        let opts = web_sys::RequestInit::new();
        opts.set_method(&method);

        let web_headers = web_sys::Headers::new().map_err(|e| format!("{:?}", e))?;
        for (key, value) in &headers {
            web_headers
                .set(key, value)
                .map_err(|e| format!("Header error: {:?}", e))?;
        }
        opts.set_headers(&web_headers);

        if let Some(ref data) = body {
            let u8_array = js_sys::Uint8Array::from(data.as_slice());
            opts.set_body(&u8_array.into());
        }

        let request = web_sys::Request::new_with_str_and_init(&url, &opts)
            .map_err(|e| format!("Request creation failed: {:?}", e))?;

        let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
        let resp_value = JsFuture::from(global.fetch_with_request(&request))
            .await
            .map_err(|e| format!("Fetch failed: {:?}", e))?;

        let response: web_sys::Response = resp_value.unchecked_into();
        let status = response.status();

        let array_buffer = JsFuture::from(
            response
                .array_buffer()
                .map_err(|e| format!("Body read error: {:?}", e))?,
        )
        .await
        .map_err(|e| format!("Body await error: {:?}", e))?;

        let u8_array = js_sys::Uint8Array::new(&array_buffer);
        let body_bytes = u8_array.to_vec();

        Ok(HttpResponse {
            status,
            body: body_bytes,
        })
    }
}
