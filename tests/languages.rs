//! Supported languages from Soniox `GET /models`: merged over models, sorted, the report.

mod common;

use common::MockSoniox;
use serde_json::{Value, json};
use sonisub::languages::{self, Language};
use sonisub::soniox::Client;

fn models() -> Value {
    json!({"models": [
        {"id": "stt-async-v5", "aliased_model_id": null, "name": "Speech-to-Text Async v5", "context_version": 2,
         "transcription_mode": "async", "languages": [{"code": "ru", "name": "Russian"}, {"code": "en", "name": "English"}],
         "translation_targets": [], "supports_language_hints_strict": true},
        {"id": "stt-async-v3", "aliased_model_id": "stt-async-v5", "transcription_mode": "async",
         "languages": [{"code": "en", "name": "English"}]},
        {"id": "stt-rt-v5", "aliased_model_id": null, "transcription_mode": "real_time",
         "languages": [{"code": "es", "name": "Spanish"}]}
    ]})
}

fn lang(code: &str, name: &str) -> Language {
    Language { code: code.into(), name_en: name.into() }
}

#[test]
fn languages_of_all_models_once_sorted() {
    let mock = MockSoniox::start(|r, _| {
        assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", "/v1/models"));
        (200, models())
    });
    let client = Client::new(&mock.url, "key").unwrap();
    let langs = languages::fetch(&client).unwrap();
    assert_eq!(langs, [lang("en", "English"), lang("es", "Spanish"), lang("ru", "Russian")]);
}

#[test]
fn report_is_one_line_per_language() {
    let r = languages::report(&[lang("en", "English"), lang("zh", "Chinese")]);
    assert_eq!(r, "en  English\nzh  Chinese\n");
}

#[test]
fn api_errors_come_through() {
    let mock = MockSoniox::start(|_, _| common::soniox_error(401, "unauthenticated", "bad key"));
    let client = Client::new(&mock.url, "key").unwrap();
    let e = languages::fetch(&client).unwrap_err();
    assert!(sonisub::soniox::api_error(&e).unwrap().is_fatal());
}
