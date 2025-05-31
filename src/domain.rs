use std::{collections::VecDeque, sync::Arc};

use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemScope {
    pub itemid: Option<String>,
    pub itemtype: Vec<String>,
    pub items: VecDeque<Property>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type", content = "value")]
pub enum ValueType {
    Empty,
    Array(VecDeque<ValueType>),
    Url(String),
    String(String),
    Meter(String),
    Time(String),
    ScopeRef(Arc<ItemScope>),
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Property {
    pub name: Name,
    pub value: ValueType,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type", content = "value")]
pub enum Name {
    Url(String),
    String(String),
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Config<'a> {
    pub base_url: &'a str,
}
