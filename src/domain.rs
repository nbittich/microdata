use std::collections::VecDeque;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct ItemScope {
    pub id: Option<String>,
    pub items: VecDeque<Property>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValueType {
    Empty,
    Url(String),
    String(String),
    Meter(String),
    Time(String),
    Scope(ItemScope),
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Property {
    pub name: Name,
    pub value: ValueType,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Name {
    Url(String),
    String(String),
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Config<'a> {
    pub base_url: &'a str,
}
