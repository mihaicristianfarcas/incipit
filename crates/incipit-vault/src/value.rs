//! A document's properties as values: what YAML frontmatter can hold, with the order of keys
//! kept. Obsidian shows properties in the order they are written, and a learner's own keys must
//! come back where they were, so a map here is a list of pairs, not a hash map. A frontmatter is a
//! dozen keys or so, so finding one by walking the list costs nothing.
//!
//! The YAML library only sees these types through serde, so it can be replaced without touching
//! anything else.

use std::fmt;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// One property's value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    List(Vec<Value>),
    Map(Properties),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The value as a line of text, for fields a person may have typed as a number or a word
    /// (`id: 12`, `deck: 2024`).
    pub fn to_text(&self) -> Option<String> {
        match self {
            Value::Text(text) => Some(text.clone()),
            Value::Int(n) => Some(n.to_string()),
            Value::Float(x) => Some(x.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            Value::Null | Value::List(_) | Value::Map(_) => None,
        }
    }

    /// A list of texts: a list as it is, or a single text as a list of one (Obsidian accepts
    /// `tags: physics` as well as `tags: [physics]`).
    pub fn to_texts(&self) -> Vec<String> {
        match self {
            Value::List(items) => items.iter().filter_map(Value::to_text).collect(),
            Value::Null => Vec::new(),
            other => other
                .to_text()
                .filter(|t| !t.is_empty())
                .into_iter()
                .collect(),
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::Float(x) if x.is_finite() => Some(*x),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Value::Int(n) => u32::try_from(*n).ok(),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&Properties> {
        match self {
            Value::Map(map) => Some(map),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(items) => Some(items),
            _ => None,
        }
    }

    /// Every text inside this value, however deep (to find the links in a property).
    pub(crate) fn texts<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Value::Text(text) => out.push(text),
            Value::List(items) => items.iter().for_each(|item| item.texts(out)),
            Value::Map(map) => map.iter().for_each(|(_, value)| value.texts(out)),
            _ => {}
        }
    }

    /// The same value with every text passed through `change`.
    pub(crate) fn map_texts(&self, change: &mut dyn FnMut(&str) -> String) -> Value {
        match self {
            Value::Text(text) => Value::Text(change(text)),
            Value::List(items) => Value::List(items.iter().map(|v| v.map_texts(change)).collect()),
            Value::Map(map) => Value::Map(Properties(
                map.iter()
                    .map(|(k, v)| (k.to_owned(), v.map_texts(change)))
                    .collect(),
            )),
            other => other.clone(),
        }
    }
}

impl From<&str> for Value {
    fn from(text: &str) -> Self {
        Value::Text(text.to_owned())
    }
}

impl From<String> for Value {
    fn from(text: String) -> Self {
        Value::Text(text)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Int(n)
    }
}

impl From<f64> for Value {
    fn from(x: f64) -> Self {
        Value::Float(x)
    }
}

impl From<Vec<String>> for Value {
    fn from(items: Vec<String>) -> Self {
        Value::List(items.into_iter().map(Value::Text).collect())
    }
}

/// Properties in the order they are written.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Properties(Vec<(String, Value)>);

impl Properties {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Sets a property where it already is, or adds it at the end.
    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        let value = value.into();
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some((_, old)) => *old = value,
            None => self.0.push((key.to_owned(), value)),
        }
    }

    /// Sets a property where it already is, or adds it before the first of `before` that is
    /// present (so new known fields keep a tidy order without moving anyone's own keys).
    pub(crate) fn set_before(&mut self, key: &str, value: Value, before: &[&str]) {
        if let Some((_, old)) = self.0.iter_mut().find(|(k, _)| k == key) {
            *old = value;
            return;
        }
        let at = self
            .0
            .iter()
            .position(|(k, _)| before.contains(&k.as_str()))
            .unwrap_or(self.0.len());
        self.0.insert(at, (key.to_owned(), value));
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let at = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.remove(at).1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_str())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The text of a property, if it is one (or a number or a word a person typed).
    pub fn text(&self, key: &str) -> Option<String> {
        self.get(key)
            .and_then(Value::to_text)
            .filter(|t| !t.is_empty())
    }

    /// The texts of a list property (a single text counts as a list of one).
    pub fn texts(&self, key: &str) -> Vec<String> {
        self.get(key).map(Value::to_texts).unwrap_or_default()
    }
}

impl FromIterator<(String, Value)> for Properties {
    fn from_iter<I: IntoIterator<Item = (String, Value)>>(pairs: I) -> Self {
        Properties(pairs.into_iter().collect())
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Int(n) => serializer.serialize_i64(*n),
            Value::Float(x) => serializer.serialize_f64(*x),
            Value::Text(text) => serializer.serialize_str(text),
            Value::List(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Value::Map(map) => map.serialize(serializer),
        }
    }
}

impl Serialize for Properties {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a property value")
    }

    fn visit_bool<E>(self, b: bool) -> Result<Value, E> {
        Ok(Value::Bool(b))
    }

    fn visit_i64<E>(self, n: i64) -> Result<Value, E> {
        Ok(Value::Int(n))
    }

    fn visit_u64<E>(self, n: u64) -> Result<Value, E> {
        Ok(i64::try_from(n).map_or(Value::Float(n as f64), Value::Int))
    }

    fn visit_i128<E>(self, n: i128) -> Result<Value, E> {
        Ok(i64::try_from(n).map_or(Value::Float(n as f64), Value::Int))
    }

    fn visit_u128<E>(self, n: u128) -> Result<Value, E> {
        Ok(i64::try_from(n).map_or(Value::Float(n as f64), Value::Int))
    }

    fn visit_f64<E>(self, x: f64) -> Result<Value, E> {
        Ok(Value::Float(x))
    }

    fn visit_str<E>(self, text: &str) -> Result<Value, E> {
        Ok(Value::Text(text.to_owned()))
    }

    fn visit_string<E>(self, text: String) -> Result<Value, E> {
        Ok(Value::Text(text))
    }

    fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<Value, E> {
        String::from_utf8(bytes.to_vec())
            .map(Value::Text)
            .map_err(|_| E::custom("binary data is not a property value"))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, inner: D) -> Result<Value, D::Error> {
        Value::deserialize(inner)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Value::List(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let mut pairs: Vec<(String, Value)> = Vec::new();
        while let Some(key) = access.next_key::<Value>()? {
            let key = key
                .to_text()
                .ok_or_else(|| de::Error::custom("a property's name must be a word or a number"))?;
            let value = access.next_value()?;
            match pairs.iter_mut().find(|(k, _)| *k == key) {
                // A name written twice: the last one counts, as in Obsidian.
                Some((_, old)) => *old = value,
                None => pairs.push((key, value)),
            }
        }
        Ok(Value::Map(Properties(pairs)))
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}
