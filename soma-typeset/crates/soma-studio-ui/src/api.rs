//! Thin typed wrapper over the Axum API. Same origin; no CORS, no keys.

use gloo_net::http::Request;
use serde::de::DeserializeOwned;
use soma_studio_core::{BuildRequest, NativeSummary, ProjectPayload, SaveRequest, TreeResponse};

fn encoded(path: &str) -> String {
    js_sys::encode_uri_component(path)
        .as_string()
        .unwrap_or_default()
}

async fn get_json<T: DeserializeOwned>(url: &str) -> Result<T, String> {
    let response = Request::get(url).send().await.map_err(|e| e.to_string())?;
    if !response.ok() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("GET {url}: HTTP {status} {text}"));
    }
    response.json::<T>().await.map_err(|e| e.to_string())
}

async fn post_json<B: serde::Serialize, T: DeserializeOwned>(
    url: &str,
    body: &B,
) -> Result<T, String> {
    let request = Request::post(url).json(body).map_err(|e| e.to_string())?;
    let response = request.send().await.map_err(|e| e.to_string())?;
    if !response.ok() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("POST {url}: HTTP {status} {text}"));
    }
    response.json::<T>().await.map_err(|e| e.to_string())
}

pub async fn project() -> Result<ProjectPayload, String> {
    get_json("/api/project").await
}

pub async fn tree() -> Result<TreeResponse, String> {
    get_json("/api/tree").await
}

pub async fn file(path: &str) -> Result<String, String> {
    let response = Request::get(&format!("/api/file?path={}", encoded(path)))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.ok() {
        return Err(format!("读取 {path} 失败（HTTP {}）", response.status()));
    }
    response.text().await.map_err(|e| e.to_string())
}

pub async fn save(path: &str, text: &str) -> Result<(), String> {
    let request = Request::post(&format!("/api/file?path={}", encoded(path)))
        .json(&SaveRequest {
            text: text.to_string(),
        })
        .map_err(|e| e.to_string())?;
    let response = request.send().await.map_err(|e| e.to_string())?;
    if !response.ok() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("保存失败（HTTP {status}）{text}"));
    }
    Ok(())
}

pub async fn asset(path: &str) -> Result<Vec<u8>, String> {
    let response = Request::get(&format!("/api/asset?path={}", encoded(path)))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.ok() {
        return Err(format!("图片 {path} 不可用（HTTP {}）", response.status()));
    }
    response.binary().await.map_err(|e| e.to_string())
}

pub async fn metrics() -> Result<Vec<u8>, String> {
    let response = Request::get("/api/metrics")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.ok() {
        return Err(format!("读取度量表失败（HTTP {}）", response.status()));
    }
    response.binary().await.map_err(|e| e.to_string())
}

pub async fn source_bytes() -> Result<Vec<u8>, String> {
    let response = Request::get("/api/source")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.ok() {
        return Err(format!("读取源文件失败（HTTP {}）", response.status()));
    }
    response.binary().await.map_err(|e| e.to_string())
}

pub async fn verify(request: &BuildRequest) -> Result<NativeSummary, String> {
    post_json("/api/verify", request).await
}

pub async fn rebuild() -> Result<ProjectPayload, String> {
    let request = Request::post("/api/rebuild")
        .json(&())
        .map_err(|e| e.to_string())?;
    let response = request.send().await.map_err(|e| e.to_string())?;
    if !response.ok() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("重新读取失败（HTTP {status}）{text}"));
    }
    response
        .json::<ProjectPayload>()
        .await
        .map_err(|e| e.to_string())
}

pub async fn pdf(request: &BuildRequest) -> Result<Vec<u8>, String> {
    let post = Request::post("/api/pdf")
        .json(request)
        .map_err(|e| e.to_string())?;
    let response = post.send().await.map_err(|e| e.to_string())?;
    if !response.ok() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("PDF 失败（HTTP {status}）{text}"));
    }
    response.binary().await.map_err(|e| e.to_string())
}

/// Save a byte blob in the browser. Used by the PDF button.
pub fn download(name: &str, bytes: &[u8]) -> Result<(), String> {
    use wasm_bindgen::JsCast;

    let array = js_sys::Array::new();
    array.push(&js_sys::Uint8Array::from(bytes));
    let blob = web_sys::Blob::new_with_buffer_source_sequence(&array)
        .map_err(|_| String::from("无法创建 Blob"))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|_| String::from("无法创建下载 URL"))?;
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| String::from("没有 document"))?;
    let anchor = document
        .create_element("a")
        .map_err(|_| String::from("无法创建下载链接"))?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| String::from("下载链接类型不对"))?;
    anchor.set_href(&url);
    anchor.set_download(name);
    anchor.click();
    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(())
}
