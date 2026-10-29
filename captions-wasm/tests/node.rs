//! `wasm-pack test --node`: `render` gives byte for byte what `sonisub render` writes for every
//! case of `tests/cases` (checked against the command line by sonisub's `tests/wasm_render.rs`),
//! and `parse`, `formats` and `layout` give what a preview needs; `mergeSpeakers` makes speakers one.

mod cases;

use captions::capt::Doc;
use captions_wasm::{formats, layout, merge_speakers, parse, render};
use js_sys::{JSON, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen(module = "fs")]
extern "C" {
    #[wasm_bindgen(js_name = readFileSync)]
    fn read_file(path: &str, encoding: &str) -> String;
}

fn repo(path: &str) -> String {
    format!("{}/../{path}", env!("CARGO_MANIFEST_DIR"))
}

fn fixture(name: &str) -> String {
    read_file(&repo(&format!("tests/fixtures/{name}")), "utf8")
}

fn get(v: &JsValue, key: &str) -> JsValue {
    Reflect::get(v, &key.into()).unwrap()
}

fn json(v: &JsValue) -> serde_json::Value {
    serde_json::from_str(&String::from(JSON::stringify(v).unwrap())).unwrap()
}

#[wasm_bindgen_test]
fn render_gives_what_the_command_line_writes() {
    for c in cases::CASES {
        let capt = fixture(&format!("{}.capt", c.capt));
        let options = JSON::parse(c.options).unwrap();
        let out = render(&capt, c.format, Some(options)).unwrap_or_else(|e| panic!("{} {}: {e:?}", c.name, c.format));
        let text = get(&out, "text").as_string().unwrap();
        let expected = read_file(&repo(&format!("captions-wasm/tests/expected/{}.{}", c.name, c.ext)), "utf8");
        assert!(text == expected, "{} {}: render differs from the command line", c.name, c.format);
        let name = get(&out, "fileName").as_string().unwrap();
        assert!(name.ends_with(&format!(".{}", c.ext)), "{name}");
    }
}

#[wasm_bindgen_test]
fn the_dialog_srt_with_no_options_is_the_fixture() {
    let out = render(&fixture("dialog.capt"), "srt", Some(JSON::parse("{}").unwrap())).unwrap();
    assert_eq!(get(&out, "text").as_string().unwrap(), fixture("dialog.srt"));
    assert_eq!(get(&out, "fileName").as_string().unwrap(), "captions.srt");
    assert_eq!(get(&out, "count").as_f64(), Some(10.0));
    // `undefined` options are the defaults too.
    let out = render(&fixture("dialog.capt"), "srt", None).unwrap();
    assert_eq!(get(&out, "text").as_string().unwrap(), fixture("dialog.srt"));
}

#[wasm_bindgen_test]
fn parse_sums_up_a_file_and_names_every_problem_with_its_line() {
    let s = json(&parse(&fixture("marks.capt")).unwrap());
    assert_eq!(s["id"], "marks-fixture");
    assert_eq!(s["speakers"][1], serde_json::json!({"id": "2", "name": "Boris"}));
    assert_eq!(s["languages"], serde_json::json!(["en", "ru"]));
    assert_eq!(s["duration"], 17000);
    assert_eq!(s["sounds"], 2);

    let e = parse("CAPT 1\n\n00:00:01.000 --> 00:00:02.000\n{0}a{x}\n{0..9} lang:en\n").unwrap_err();
    let errors = json(&get(&e, "errors"));
    assert_eq!(errors[0]["line"], 4);
    assert!(errors[0]["message"].as_str().unwrap().contains("hundredths"), "{errors}");
    assert!(e.is_instance_of::<js_sys::Error>());
}

#[wasm_bindgen_test]
fn formats_list_the_options_captions_takes_with_their_defaults() {
    let list = json(&formats());
    let names: Vec<&str> = list.as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["srt", "vtt", "ass", "premiere", "capt", "sbv", "lrc", "transcript", "ttml", "adr", "ytt"]);
    let of = |f: &str| list.as_array().unwrap().iter().find(|x| x["name"] == f).unwrap().clone();
    let option = |f: &str, o: &str| of(f)["options"].as_array().unwrap().iter().find(|x| x["name"] == o).cloned();
    assert_eq!(option("srt", "maxLine").unwrap()["default"], 50);
    assert_eq!(option("srt", "maxDuration").unwrap()["default"], 8.0);
    assert_eq!(option("adr", "maxDuration").unwrap()["default"], 8.0);
    assert_eq!(option("adr", "fps").unwrap()["default"], "25");
    assert_eq!(option("adr", "timelineStart").unwrap()["default"], "01:00:00:00");
    assert!(option("srt", "karaoke").is_none());
    assert!(option("ass", "karaoke").is_some());
    assert_eq!(of("capt")["options"].as_array().unwrap().len(), 1);
    assert_eq!(of("adr")["extension"], "adr.csv");
}

#[wasm_bindgen_test]
fn an_option_a_format_does_not_take_or_a_wrong_value_is_an_error() {
    let capt = fixture("dialog.capt");
    let err = |format: &str, options: &str| {
        let e = render(&capt, format, Some(JSON::parse(options).unwrap())).unwrap_err();
        get(&e, "message").as_string().unwrap()
    };
    assert!(err("srt", r#"{"karaoke": true}"#).contains("takes no option"));
    assert!(err("srt", r#"{"maxLine": -1}"#).contains("maxLine"));
    assert!(err("adr", r#"{"fps": "29"}"#).contains("frame rate"));
    assert!(err("adr", r#"{"timelineStart": "00:00:00:25"}"#).contains("frames run"));
    assert!(err("doc", "{}").contains("no format"));
}

#[wasm_bindgen_test]
fn layout_gives_cues_with_every_word_and_piece_timed_and_placed() {
    let l = json(&layout(&fixture("marks.capt"), "ass", Some(JSON::parse(r#"{"karaoke": true}"#).unwrap())).unwrap());
    assert_eq!(l["kind"], "cues");
    let cues = l["cues"].as_array().unwrap();
    let first = &cues[0];
    assert_eq!(first["speaker"], "1");
    assert_eq!(first["style"]["name"], "Anna");
    assert_eq!(first["words"][3]["text"], "War");
    assert_eq!(first["words"][3]["italic"], true);
    // A word of two pieces: "deploy" + "ment", each with its time.
    let deployment =
        cues.iter().flat_map(|c| c["words"].as_array().unwrap()).find(|w| w["text"] == "deployment").unwrap();
    assert_eq!(
        deployment["pieces"],
        serde_json::json!([{"text": "deploy", "start": 6260, "end": 6480}, {"text": "ment", "start": 6480, "end": 6700}])
    );
    assert_eq!((deployment["language"].as_str(), deployment["lang"].as_str()), (Some("en"), None));
    let top = cues.iter().find(|c| c["words"][0]["text"] == "Listen").unwrap();
    assert_eq!(top["position"], "top");
    let boxed = cues.iter().find(|c| c["words"][0]["text"] == "This").unwrap();
    assert_eq!(boxed["position"]["rect"]["w"], 0.4);
    assert_eq!(boxed["shown"][0], serde_json::json!({"line": 0, "space": false, "text": "This", "word": 0}));
}

#[wasm_bindgen_test]
fn layout_gives_adr_rows_premiere_segments_and_text() {
    let capt = fixture("dialog.capt");
    let adr = json(&layout(&capt, "adr", None).unwrap());
    assert_eq!(adr["kind"], "adr");
    assert_eq!(adr["rows"][0]["in"], "01:00:00:21");
    assert!(adr["rows"][0]["character"].as_str().unwrap().starts_with("Speaker"));
    let premiere = json(&layout(&capt, "premiere", None).unwrap());
    assert!(premiere["transcript"]["segments"].as_array().unwrap().len() > 1);
    let text = json(&layout(&capt, "txt", None).unwrap());
    assert_eq!(text["kind"], "text");
}

/// Anna is heard as speakers 1 and 3, her sentence split between them around Boris (2).
const SPLIT: &str = "CAPT 1
lang:en
speaker:1
speaker:2 name:Boris
speaker:3 name:Anna
speaker:4

00:00:01.000 --> 00:00:02.000 id:aaaaaa speaker:1
{0}So{30} {40}we{60} {70}went{100}

00:00:02.200 --> 00:00:03.000 id:bbbbbb speaker:3
{0}home{40} {50}early.{80}

00:00:04.000 --> 00:00:05.000 id:cccccc speaker:2
{0}Really?{100}

00:00:06.000 --> 00:00:07.000 id:dddddd speaker:4
{0}Yes.{100}
";

fn merged(map: &str) -> Result<String, JsValue> {
    merge_speakers(SPLIT, JSON::parse(map).unwrap())
}

#[wasm_bindgen_test]
fn merge_speakers_gives_capt_text_the_formats_see_as_one_speaker() {
    let capt = merged(r#"{"3": 1}"#).unwrap();
    assert!(
        capt.contains(
            "speaker:1 name:Anna
speaker:2 name:Boris
speaker:4

"
        ),
        "{capt}"
    );
    assert!(!capt.contains("speaker:3"), "{capt}");
    let s = json(&parse(&capt).unwrap());
    assert_eq!(s["speakers"][0], serde_json::json!({"id": "1", "name": "Anna"}));

    // SRT: her two lines join into one cue.
    let text = |capt: &str, format: &str| get(&render(capt, format, None).unwrap(), "text").as_string().unwrap();
    assert!(text(SPLIT, "srt").contains(
        "So we went

2
"
    ));
    assert!(text(&capt, "srt").starts_with(
        "1
00:00:01,000 --> 00:00:03,000
So we went home early.

2
"
    ));
    // ASS: one style for her (Default, Anna, Boris, Speaker 4); the layout gives her cue its colour.
    let ass = text(&capt, "ass");
    assert_eq!(ass.lines().filter(|l| l.starts_with("Style: ")).count(), 4, "{ass}");
    assert_eq!(text(SPLIT, "ass").lines().filter(|l| l.starts_with("Style: ")).count(), 5);
    let l = json(&layout(&capt, "ass", None).unwrap());
    assert_eq!(l["cues"][0]["style"]["name"], "Anna");
    // ADR: one character.
    let rows = json(&layout(&capt, "adr", None).unwrap())["rows"].clone();
    let characters: Vec<&str> = rows.as_array().unwrap().iter().map(|r| r["character"].as_str().unwrap()).collect();
    assert_eq!(characters, ["Anna", "Boris", "Speaker 4"]);
    assert_eq!(rows[0]["dialogue"], "So we went home early.");
}

#[wasm_bindgen_test]
fn merge_speakers_resolves_chains_and_keeps_the_target_s_name() {
    let s = json(&parse(&merged(r#"{"3": 2, "2": 1}"#).unwrap()).unwrap());
    assert_eq!(s["speakers"], serde_json::json!([{"id": "1", "name": "Boris"}, {"id": "4", "name": null}]));
    let s = json(&parse(&merged(r#"{"3": 2}"#).unwrap()).unwrap());
    assert_eq!(s["speakers"][1], serde_json::json!({"id": "2", "name": "Boris"}));
    assert_eq!(s["speakers"].as_array().unwrap().len(), 3);
    assert_eq!(merged("{}").unwrap(), Doc::parse(SPLIT).unwrap().write());
}

#[wasm_bindgen_test]
fn merge_speakers_throws_for_itself_an_unknown_speaker_or_a_broken_file() {
    let message = |e: JsValue| get(&e, "message").as_string().unwrap();
    assert_eq!(message(merged(r#"{"1": 1}"#).unwrap_err()), "speaker 1 is mapped onto itself");
    assert_eq!(message(merged(r#"{"1": 2, "2": 1}"#).unwrap_err()), "speaker 1 is mapped onto itself");
    assert_eq!(message(merged(r#"{"3": 9}"#).unwrap_err()), "the file has no speaker 9");
    assert!(message(merged(r#"{"x": 1}"#).unwrap_err()).contains("numbers"));
    assert!(message(merged(r#"{"3": "1"}"#).unwrap_err()).contains("numbers"));
    assert!(message(merge_speakers(SPLIT, JsValue::NULL).unwrap_err()).contains("object"));
    let e = merge_speakers(
        "CAPT 1

00:00:01.000 --> 00:00:02.000
{0}a{x}
",
        JSON::parse("{}").unwrap(),
    )
    .unwrap_err();
    assert!(get(&e, "errors").is_object());
}

#[wasm_bindgen_test]
fn speaker_labels_every_labels_every_cue_of_a_turn() {
    let list = json(&formats());
    let srt = list.as_array().unwrap().iter().find(|f| f["name"] == "srt").unwrap();
    let option = srt["options"].as_array().unwrap().iter().find(|o| o["name"] == "speakerLabelsEvery").unwrap();
    assert_eq!((option["type"].as_str(), option["default"].as_bool()), (Some("boolean"), Some(false)));

    let capt = fixture("dialog.capt");
    let prefixes = |options: &str| {
        let l = json(&layout(&capt, "vtt", Some(JSON::parse(options).unwrap())).unwrap());
        l["cues"].as_array().unwrap().iter().map(|c| c["prefix"].as_str().unwrap().to_string()).collect::<Vec<_>>()
    };
    let every = prefixes(r#"{"speakerLabels": true, "speakerLabelsEvery": true}"#);
    assert!(every.iter().all(|p| p.starts_with("Speaker ")), "{every:?}");
    // Speaker 2's long turn: labelled on every cue, not only at the change.
    assert_eq!(every[2..5], ["Speaker 2: ", "Speaker 2: ", "Speaker 2: "]);
    assert_eq!(prefixes(r#"{"speakerLabels": true}"#)[2..5], ["", "", ""]);
    // Without speakerLabels it changes nothing.
    assert!(prefixes(r#"{"speakerLabelsEvery": true}"#).iter().all(String::is_empty));
}
