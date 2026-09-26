//! Languages Soniox recognises: the codes `--lang` takes.

use anyhow::Result;
use serde::Deserialize;

use crate::soniox::Client;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub struct Language {
    /// What `--lang` takes: "en", "ru", "zh"...
    pub code: String,
    /// English name, as Soniox gives it: "Russian".
    #[serde(rename = "name")]
    pub name_en: String,
}

/// A Soniox model as `GET /models` lists it; only its languages matter here.
#[derive(Debug, Clone, Deserialize)]
pub struct Model {
    #[serde(default)]
    pub languages: Vec<Language>,
}

/// Supported languages, fetched from Soniox, sorted by code.
pub fn fetch(client: &Client) -> Result<Vec<Language>> {
    Ok(of_models(&client.models()?))
}

/// Soniox lists languages per model (all the same today): every language of any model, once, sorted by code.
pub fn of_models(models: &[Model]) -> Vec<Language> {
    let mut langs: Vec<Language> = models.iter().flat_map(|m| m.languages.iter().cloned()).collect();
    langs.sort();
    langs.dedup_by(|a, b| a.code == b.code);
    langs
}

/// Human report for `sonisub languages`: one "code  name" line per language.
pub fn report(langs: &[Language]) -> String {
    let width = langs.iter().map(|l| l.code.len()).max().unwrap_or(0);
    langs.iter().map(|l| format!("{:<width$}  {}\n", l.code, l.name_en)).collect()
}
