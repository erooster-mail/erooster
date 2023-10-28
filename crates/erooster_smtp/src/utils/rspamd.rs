// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use erooster_deps::{
    serde::{self, Deserialize, Serialize},
    serde_json,
};

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "self::serde")]
pub struct Response {
    pub is_skipped: bool,
    pub score: f64,
    pub required_score: f64,
    pub action: Action,
    pub symbols: BTreeMap<String, Symbol>,
    pub subject: Option<String>,
    pub urls: Option<Vec<String>>,
    pub emails: Option<Vec<String>>,
    #[serde(rename = "message-id")]
    pub message_id: Option<String>,
    pub messages: serde_json::Value,
    pub time_real: f64,
    pub dkim_signatures: Option<String>,
    pub milter: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "self::serde")]
pub struct Symbol {
    pub name: String,
    pub score: f64,
    pub metric_score: f64,
    pub description: Option<String>,
    pub options: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "self::serde")]
#[allow(clippy::enum_variant_names)]
pub enum Action {
    #[serde(rename = "no action")]
    NoAction,
    #[serde(rename = "greylist")]
    Greylist,
    #[serde(rename = "add header")]
    AddHeader,
    #[serde(rename = "rewrite subject")]
    RewriteSubject,
    #[serde(rename = "soft reject")]
    SoftReject,
    #[serde(rename = "reject")]
    Reject,
}
