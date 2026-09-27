use dfd_canvas::model::Document;
use wasm_bindgen::JsCast;

// Preserve existing browser autosaves across the DFD Canvas rename.
pub const STORAGE_KEY: &str = "dfd-studio.document.v1";

pub fn restore() -> Result<Option<Document>, String> {
    let storage = web_sys::window()
        .unwrap()
        .local_storage()
        .map_err(|_| "ブラウザ内の自動保存を利用できません".to_string())?
        .ok_or("ブラウザ内の自動保存を利用できません")?;
    let text = storage
        .get_item(STORAGE_KEY)
        .map_err(|_| "復旧データを読めません".to_string())?;
    text.map(|s| Document::from_json(&s)).transpose()
}

pub fn autosave(doc: &Document) -> Result<(), String> {
    let text = doc.to_json()?;
    let storage = web_sys::window()
        .unwrap()
        .local_storage()
        .map_err(|_| "自動保存に失敗しました。JSONファイルで保存してください".to_string())?
        .ok_or("自動保存に失敗しました。JSONファイルで保存してください")?;
    storage.set_item(STORAGE_KEY, &text).map_err(|_| {
        "自動保存に失敗しました（容量・ブラウザ設定）。JSONファイルで保存してください".into()
    })
}

pub fn download(text: &str, filename: &str) -> Result<(), String> {
    let parts = js_sys::Array::new();
    parts.push(&wasm_bindgen::JsValue::from_str(text));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/json;charset=utf-8");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &options)
        .map_err(|_| "保存ファイルを作成できません")?;
    let url =
        web_sys::Url::create_object_url_with_blob(&blob).map_err(|_| "保存URLを作成できません")?;
    let document = web_sys::window().unwrap().document().unwrap();
    let link = document
        .create_element("a")
        .unwrap()
        .unchecked_into::<web_sys::HtmlAnchorElement>();
    link.set_href(&url);
    link.set_download(filename);
    link.set_hidden(true);
    document.body().unwrap().append_child(&link).unwrap();
    link.click();
    link.remove();
    gloo_timers::callback::Timeout::new(1000, move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    })
    .forget();
    Ok(())
}
