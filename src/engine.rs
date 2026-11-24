use anyhow::{Context, Result, bail};
use image::RgbImage;
use libloading::Library;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_int};
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use crate::config::{resolve_engine_path, resolve_model_path};
use crate::pdf::{is_pdf_bytes, is_pdf_path, render_pdf_bytes, render_pdf_file};
use crate::tiling::{OCRLine, deduplicate_lines, run_tiled_ocr};

const VISION_BIT: c_int = 4;
const OUT_BUFFER_CAPACITY: usize = 16 * 1024 * 1024;
const SUPPORTED_LANGUAGES: [&str; 7] = ["en", "de", "fr", "es", "it", "nl", "pl"];

type FnLoad = unsafe extern "C" fn(*const u8, u64) -> c_int;
type FnModels = unsafe extern "C" fn() -> c_int;
type FnLastError = unsafe extern "C" fn() -> *const c_char;
type FnSetThreads = unsafe extern "C" fn(c_int);
type FnDescribe = unsafe extern "C" fn(
    *const u8,
    c_int,
    c_int,
    *const c_char,
    *const c_char,
    *const c_char,
    *mut c_char,
    c_int,
) -> c_int;

struct NativeHandle {
    _lib: Library,
    loaded_model: Option<PathBuf>,
    load: FnLoad,
    models: FnModels,
    last_error: FnLastError,
    set_threads: FnSetThreads,
    describe: FnDescribe,
    out_buf: Vec<u8>,
}

// Safety: all access to NativeHandle is serialized behind the global Mutex.
unsafe impl Send for NativeHandle {}

static GLOBAL_NATIVE: OnceLock<Mutex<Option<NativeHandle>>> = OnceLock::new();

fn native_mutex() -> &'static Mutex<Option<NativeHandle>> {
    GLOBAL_NATIVE.get_or_init(|| Mutex::new(None))
}

pub fn preset_forms() -> BTreeMap<String, Value> {
    let mut map = BTreeMap::new();
    map.insert(
        "invoice".into(),
        json!({
            "document_type": {"enum": ["invoice", "receipt", "quote", "other"]},
            "vendor_name": {"type": "string"},
            "invoice_number": {"type": "string"},
            "date": {"type": "string"},
            "total_amount": {"type": "string"}
        }),
    );
    map.insert(
        "receipt".into(),
        json!({
            "merchant": {"type": "string"},
            "date": {"type": "string"},
            "payment_method": {"enum": ["card", "cash", "upi", "other"]},
            "total": {"type": "string"}
        }),
    );
    map.insert(
        "id_card".into(),
        json!({
            "card_type": {"enum": ["national_id", "passport", "driver_license", "student_id", "other"]},
            "full_name": {"type": "string"},
            "id_number": {"type": "string"},
            "date_of_birth": {"type": "string"}
        }),
    );
    map.insert(
        "business_card".into(),
        json!({
            "person_name": {"type": "string"},
            "company": {"type": "string"},
            "email": {"type": "string"},
            "phone": {"type": "string"}
        }),
    );
    map
}

#[derive(Debug, Clone)]
pub struct SmartOptions {
    pub mode: String,
    pub text_conf: f64,
    pub conf: f64,
    pub language: String,
    pub keywords: Vec<String>,
    pub form: Option<Value>,
    pub form_preset: Option<String>,
    pub dpi: u32,
    pub max_pages: usize,
}

impl Default for SmartOptions {
    fn default() -> Self {
        Self {
            mode: "auto".into(),
            text_conf: 0.3,
            conf: 0.3,
            language: "en".into(),
            keywords: Vec::new(),
            form: None,
            form_preset: None,
            dpi: 180,
            max_pages: 25,
        }
    }
}

#[derive(Clone)]
pub struct OCREngine {
    pub engine_path: PathBuf,
    pub model_path: PathBuf,
    pub threads: i32,
}

impl OCREngine {
    pub fn new(
        engine_override: Option<&Path>,
        model_override: Option<&Path>,
        threads: Option<i32>,
    ) -> Result<Self> {
        let engine_path = resolve_engine_path(engine_override)?;
        let model_path = resolve_model_path(model_override)?;
        let threads = threads.unwrap_or_else(|| {
            std::env::var("AKSHAR_THREADS")
                .ok()
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or(0)
        });
        let engine = Self {
            engine_path,
            model_path,
            threads,
        };
        engine.ensure_loaded()?;
        Ok(engine)
    }

    pub fn model_filename(&self) -> String {
        self.model_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("ocr_v1.cact")
            .to_string()
    }

    fn ensure_loaded(&self) -> Result<()> {
        let mut guard = native_mutex()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine mutex poisoned"))?;

        if guard.is_none() {
            let handle = unsafe {
                let lib = Library::new(&self.engine_path).with_context(|| {
                    format!("failed to load native library {}", self.engine_path.display())
                })?;
                let load: FnLoad = *lib.get(b"cactus_load\0")?;
                let models: FnModels = *lib.get(b"cactus_models\0")?;
                let last_error: FnLastError = *lib.get(b"cactus_last_error\0")?;
                let set_threads: FnSetThreads = *lib.get(b"cactus_set_threads\0")?;
                let describe: FnDescribe = *lib.get(b"cactus_describe\0")?;
                if self.threads >= 0 {
                    set_threads(self.threads as c_int);
                }
                NativeHandle {
                    _lib: lib,
                    loaded_model: None,
                    load,
                    models,
                    last_error,
                    set_threads,
                    describe,
                    out_buf: vec![0u8; OUT_BUFFER_CAPACITY],
                }
            };
            *guard = Some(handle);
        }

        let handle = guard.as_mut().unwrap();
        if handle.loaded_model.as_ref() != Some(&self.model_path) {
            let weights = fs::read(&self.model_path)
                .with_context(|| format!("failed to read model weights {}", self.model_path.display()))?;
            let rc = unsafe { (handle.load)(weights.as_ptr(), weights.len() as u64) };
            if rc < 0 {
                let err = unsafe { read_c_error(handle.last_error) };
                bail!("failed to load model weights: {}", err);
            }
            let mask = unsafe { (handle.models)() };
            if (mask & VISION_BIT) == 0 {
                bail!("{} is not a valid vision OCR archive", self.model_filename());
            }
            if self.threads >= 0 {
                unsafe { (handle.set_threads)(self.threads as c_int) };
            }
            handle.loaded_model = Some(self.model_path.clone());
        }

        Ok(())
    }

    fn raw_describe(
        &self,
        rgb: &RgbImage,
        options: &Value,
        language: &str,
        keywords: &[String],
    ) -> Result<Value> {
        if !SUPPORTED_LANGUAGES.contains(&language) {
            bail!(
                "Unsupported language '{}'. Supported: {}",
                language,
                SUPPORTED_LANGUAGES.join(", ")
            );
        }
        let opts_json = serde_json::to_string(options)?;
        let c_opts = CString::new(opts_json)?;
        let c_lang = CString::new(language)?;
        let c_kw = if keywords.is_empty() {
            None
        } else {
            Some(CString::new(keywords.join("\n"))?)
        };

        let (width, height) = rgb.dimensions();
        let raw_pixels = rgb.as_raw();

        self.ensure_loaded()?;
        let mut guard = native_mutex()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine mutex poisoned"))?;
        let handle = guard.as_mut().unwrap();

        let cap = handle.out_buf.len() as c_int;
        let rc = unsafe {
            (handle.describe)(
                raw_pixels.as_ptr(),
                width as c_int,
                height as c_int,
                c_opts.as_ptr(),
                c_lang.as_ptr(),
                c_kw.as_ref().map(|s| s.as_ptr()).unwrap_or(ptr::null()),
                handle.out_buf.as_mut_ptr() as *mut c_char,
                cap,
            )
        };
        if rc < 0 {
            let err = unsafe { read_c_error(handle.last_error) };
            bail!("OCR engine error: {}", err);
        }

        let nul_pos = handle
            .out_buf
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(handle.out_buf.len());
        let json_str = std::str::from_utf8(&handle.out_buf[..nul_pos])
            .context("invalid UTF-8 returned from native engine")?;
        let parsed: Value =
            serde_json::from_str(json_str).context("failed to parse JSON from native engine")?;
        Ok(parsed)
    }

    fn ocr_single_rgb(
        &self,
        rgb: &RgbImage,
        text_conf: f64,
        max_lines: usize,
        language: &str,
        keywords: &[String],
    ) -> Result<Vec<OCRLine>> {
        let opts = json!({
            "sections": ["text"],
            "fields": ["bbox", "score"],
            "text_conf": text_conf,
            "max_text_lines": max_lines.clamp(1, 32)
        });
        let raw = self.raw_describe(rgb, &opts, language, keywords)?;
        let (width, height) = rgb.dimensions();
        let mut lines = Vec::new();

        if let Some(arr) = raw.get("text").and_then(|v| v.as_array()) {
            for item in arr {
                let s = item
                    .get("string")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if s.is_empty() {
                    continue;
                }
                let bn = parse_bbox(item.get("bbox"));
                let px = norm_to_px(bn, width, height);
                let conf = item.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                lines.push(OCRLine {
                    text: s.to_string(),
                    confidence: (conf * 10000.0).round() / 10000.0,
                    bbox: px,
                    bbox_norm: bn,
                });
            }
        }
        Ok(deduplicate_lines(lines))
    }

    pub fn ocr_rgb(
        &self,
        rgb: &RgbImage,
        text_conf: f64,
        max_lines: usize,
        language: &str,
        keywords: &[String],
        tiled: bool,
        tile_height: Option<u32>,
    ) -> Result<Value> {
        let t0 = Instant::now();
        let (lines, tiles_count) = if tiled {
            run_tiled_ocr(
                rgb,
                |strip| self.ocr_single_rgb(strip, text_conf, 32, language, keywords),
                tile_height,
                0.22,
            )?
        } else {
            (
                self.ocr_single_rgb(rgb, text_conf, max_lines, language, keywords)?,
                1,
            )
        };

        let elapsed_ms = round2(t0.elapsed().as_secs_f64() * 1000.0);
        let text = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let (width, height) = rgb.dimensions();

        Ok(json!({
            "text": text,
            "lines": lines,
            "meta": {
                "width": width,
                "height": height,
                "line_count": lines.len(),
                "tiled": tiled,
                "tiles_processed": tiles_count,
                "elapsed_ms": elapsed_ms
            }
        }))
    }

    pub fn extract_rgb(
        &self,
        rgb: &RgbImage,
        form: &Value,
        language: &str,
        keywords: &[String],
    ) -> Result<Value> {
        if !form.is_object() || form.as_object().map(|m| m.is_empty()).unwrap_or(true) {
            bail!("form must be a non-empty JSON schema field mapping");
        }
        let t0 = Instant::now();
        let raw = self.raw_describe(rgb, &json!({ "form": form }), language, keywords)?;
        let (width, height) = rgb.dimensions();
        Ok(json!({
            "fields": raw,
            "meta": {
                "width": width,
                "height": height,
                "elapsed_ms": round2(t0.elapsed().as_secs_f64() * 1000.0)
            }
        }))
    }

    pub fn describe_rgb(
        &self,
        rgb: &RgbImage,
        conf: f64,
        text_conf: f64,
        language: &str,
        keywords: &[String],
    ) -> Result<Value> {
        let t0 = Instant::now();
        let opts = json!({
            "sections": ["scene", "objects", "text", "relations"],
            "fields": ["bbox", "score", "position", "description"],
            "conf": conf,
            "text_conf": text_conf
        });
        let raw = self.raw_describe(rgb, &opts, language, keywords)?;
        let (width, height) = rgb.dimensions();

        let mut objects = Vec::new();
        if let Some(arr) = raw.get("objects").and_then(|v| v.as_array()) {
            for obj in arr {
                let bn = parse_bbox(obj.get("bbox"));
                let px = norm_to_px(bn, width, height);
                let score = obj.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                objects.push(json!({
                    "id": obj.get("id").cloned().unwrap_or(Value::Null),
                    "name": obj.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "confidence": (score * 10000.0).round() / 10000.0,
                    "bbox": px,
                    "bbox_norm": bn,
                    "position": obj.get("position").cloned().unwrap_or(Value::Null),
                    "description": obj.get("description").cloned().unwrap_or_else(|| Value::Object(Map::new()))
                }));
            }
        }

        let mut text_lines = Vec::new();
        if let Some(arr) = raw.get("text").and_then(|v| v.as_array()) {
            for item in arr {
                let s = item.get("string").and_then(|v| v.as_str()).unwrap_or("").trim();
                if s.is_empty() {
                    continue;
                }
                let bn = parse_bbox(item.get("bbox"));
                let px = norm_to_px(bn, width, height);
                let score = item.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                text_lines.push(OCRLine {
                    text: s.to_string(),
                    confidence: (score * 10000.0).round() / 10000.0,
                    bbox: px,
                    bbox_norm: bn,
                });
            }
        }

        let text = text_lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(json!({
            "scene": raw.get("scene").cloned().unwrap_or_else(|| Value::Object(Map::new())),
            "objects": objects,
            "text": text,
            "lines": text_lines,
            "relations": raw.get("relations").cloned().unwrap_or_else(|| Value::Array(vec![])),
            "meta": {
                "width": width,
                "height": height,
                "object_count": objects.len(),
                "line_count": text_lines.len(),
                "elapsed_ms": round2(t0.elapsed().as_secs_f64() * 1000.0)
            }
        }))
    }

    pub fn smart_process_rgb(&self, rgb: &RgbImage, opts: &SmartOptions) -> Result<Value> {
        let t0 = Instant::now();
        let resolved_form = opts.form.clone().or_else(|| {
            opts.form_preset
                .as_deref()
                .and_then(|k| preset_forms().remove(k))
        });

        match opts.mode.as_str() {
            "form" => {
                let schema = resolved_form
                    .ok_or_else(|| anyhow::anyhow!("form mode requires a form schema or valid form_preset"))?;
                let mut res = self.extract_rgb(rgb, &schema, &opts.language, &opts.keywords)?;
                res["meta"]["auto_route"] = json!("structured_form");
                Ok(res)
            }
            "vision" => {
                let mut res = self.describe_rgb(rgb, opts.conf, opts.text_conf, &opts.language, &opts.keywords)?;
                res["meta"]["auto_route"] = json!("scene_vision");
                Ok(res)
            }
            "tiled" => {
                let mut res = self.ocr_rgb(rgb, opts.text_conf, 32, &opts.language, &opts.keywords, true, None)?;
                res["meta"]["auto_route"] = json!("tiled_ocr");
                Ok(res)
            }
            "ocr" => {
                let mut res = self.ocr_rgb(rgb, opts.text_conf, 32, &opts.language, &opts.keywords, false, None)?;
                res["meta"]["auto_route"] = json!("standard_ocr");
                Ok(res)
            }
            _ => {
                // Smart `auto` routing
                let (w, h) = rgb.dimensions();
                let is_tall_doc = h > 1100 && (h as f64) / (w.max(1) as f64) >= 1.35;
                if is_tall_doc {
                    let mut res = self.ocr_rgb(rgb, opts.text_conf, 32, &opts.language, &opts.keywords, true, None)?;
                    if res["meta"]["line_count"].as_u64().unwrap_or(0) > 0 {
                        res["meta"]["auto_route"] = json!("tiled_ocr");
                        return Ok(res);
                    }
                    let mut vis = self.describe_rgb(rgb, opts.conf, opts.text_conf, &opts.language, &opts.keywords)?;
                    vis["meta"]["auto_route"] = json!("scene_vision_fallback");
                    vis["meta"]["elapsed_ms"] = json!(round2(t0.elapsed().as_secs_f64() * 1000.0));
                    return Ok(vis);
                }

                let mut res = self.ocr_rgb(rgb, opts.text_conf, 32, &opts.language, &opts.keywords, false, None)?;
                if res["meta"]["line_count"].as_u64().unwrap_or(0) > 0 {
                    res["meta"]["auto_route"] = json!("standard_ocr");
                    return Ok(res);
                }
                let mut vis = self.describe_rgb(rgb, opts.conf, opts.text_conf, &opts.language, &opts.keywords)?;
                vis["meta"]["auto_route"] = json!("scene_vision_fallback");
                vis["meta"]["elapsed_ms"] = json!(round2(t0.elapsed().as_secs_f64() * 1000.0));
                Ok(vis)
            }
        }
    }

    fn process_pdf_pages(&self, pages: Vec<RgbImage>, opts: &SmartOptions) -> Result<Value> {
        let t0 = Instant::now();
        let mut page_results = Vec::with_capacity(pages.len());
        let mut combined_parts = Vec::new();
        let mut total_lines = 0usize;
        let mut total_tiles = 0usize;

        for (idx, page_img) in pages.into_iter().enumerate() {
            let page_num = idx + 1;
            let (pw, ph) = page_img.dimensions();
            let mut page_opts = opts.clone();
            if (opts.mode == "auto" || opts.mode == "tiled")
                && (ph > 1100 || (ph as f64) / (pw.max(1) as f64) > 1.35)
            {
                page_opts.mode = "tiled".into();
            }
            let res = self.smart_process_rgb(&page_img, &page_opts)?;
            let text = res["text"].as_str().unwrap_or("").to_string();
            let lines = res.get("lines").cloned().unwrap_or_else(|| json!([]));
            let line_cnt = lines.as_array().map(|a| a.len()).unwrap_or(0);
            total_lines += line_cnt;
            total_tiles += res["meta"]["tiles_processed"].as_u64().unwrap_or(1) as usize;

            if !text.is_empty() {
                combined_parts.push(format!("--- Page {} ---\n{}", page_num, text));
            }
            page_results.push(json!({
                "page": page_num,
                "width": pw,
                "height": ph,
                "text": text,
                "lines": lines,
                "fields": res.get("fields").cloned().unwrap_or(Value::Null),
                "scene": res.get("scene").cloned().unwrap_or(Value::Null),
                "objects": res.get("objects").cloned().unwrap_or(Value::Null),
                "auto_route": res["meta"]["auto_route"].as_str().unwrap_or(&opts.mode),
                "elapsed_ms": res["meta"]["elapsed_ms"].as_f64().unwrap_or(0.0)
            }));
        }

        let first_lines = page_results
            .first()
            .and_then(|p| p.get("lines"))
            .cloned()
            .unwrap_or_else(|| json!([]));

        Ok(json!({
            "text": combined_parts.join("\n\n"),
            "pages": page_results,
            "lines": first_lines,
            "meta": {
                "is_pdf": true,
                "page_count": page_results.len(),
                "line_count": total_lines,
                "tiles_processed": total_tiles,
                "auto_route": format!("pdf_{}", opts.mode),
                "elapsed_ms": round2(t0.elapsed().as_secs_f64() * 1000.0)
            }
        }))
    }

    pub fn smart_process_bytes(
        &self,
        data: &[u8],
        filename: Option<&str>,
        opts: &SmartOptions,
    ) -> Result<Value> {
        if is_pdf_bytes(data, filename) {
            let pages = render_pdf_bytes(data, opts.dpi, opts.max_pages)?;
            return self.process_pdf_pages(pages, opts);
        }
        let rgb = decode_rgb_bytes(data)?;
        self.smart_process_rgb(&rgb, opts)
    }

    pub fn smart_process_file(&self, path: &Path, opts: &SmartOptions) -> Result<Value> {
        if is_pdf_path(path) {
            let pages = render_pdf_file(path, opts.dpi, opts.max_pages)?;
            return self.process_pdf_pages(pages, opts);
        }
        let data = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        let rgb = decode_rgb_bytes(&data)?;
        self.smart_process_rgb(&rgb, opts)
    }
}

pub fn decode_rgb_bytes(data: &[u8]) -> Result<RgbImage> {
    let img = image::load_from_memory(data).context("failed to decode image bytes")?;
    Ok(img.to_rgb8())
}

unsafe fn read_c_error(last_error: FnLastError) -> String {
    let ptr = unsafe { last_error() };
    if ptr.is_null() {
        return "unknown native error".into();
    }
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

fn parse_bbox(val: Option<&Value>) -> [i32; 4] {
    let mut out = [0i32; 4];
    if let Some(arr) = val.and_then(|v| v.as_array()) {
        for (i, v) in arr.iter().take(4).enumerate() {
            out[i] = v.as_i64().unwrap_or(0) as i32;
        }
    }
    out
}

fn norm_to_px(bn: [i32; 4], width: u32, height: u32) -> [i32; 4] {
    [
        ((bn[0] as f64) / 1000.0 * (width as f64)).round() as i32,
        ((bn[1] as f64) / 1000.0 * (height as f64)).round() as i32,
        ((bn[2] as f64) / 1000.0 * (width as f64)).round() as i32,
        ((bn[3] as f64) / 1000.0 * (height as f64)).round() as i32,
    ]
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
