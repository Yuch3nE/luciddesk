//! Reject duplicate keys at every depth before decoding plans or shorthand fields.
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use std::{collections::HashSet, fmt};
struct Checked;
impl<'de> Deserialize<'de> for Checked {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Check;
        impl<'de> Visitor<'de> for Check {
            type Value = Checked;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON without duplicate object keys")
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Checked, E> {
                Ok(Checked)
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Checked, E> {
                Ok(Checked)
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Checked, E> {
                Ok(Checked)
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Checked, E> {
                Ok(Checked)
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Checked, E> {
                Ok(Checked)
            }
            fn visit_unit<E: de::Error>(self) -> Result<Checked, E> {
                Ok(Checked)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Checked, A::Error> {
                while seq.next_element::<Checked>()?.is_some() {}
                Ok(Checked)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Checked, A::Error> {
                let mut keys = HashSet::new();
                while let Some(key) = map.next_key::<String>()? {
                    if !keys.insert(key.clone()) {
                        return Err(de::Error::custom(format!("duplicate JSON key: {key}")));
                    }
                    map.next_value::<Checked>()?;
                }
                Ok(Checked)
            }
        }
        deserializer.deserialize_any(Check)
    }
}
/// Validates bounded protocol JSON without accepting duplicate object keys.
/// # Errors
/// Returns a JSON error for duplicate keys, malformed input or excessive nesting.
pub fn validate_json(bytes: &[u8]) -> Result<(), serde_json::Error> {
    serde_json::from_slice::<Checked>(bytes).map(|_| ())
}
/// Parse once while rejecting duplicate keys, including escaped equivalents.
/// Typed conversion can then consume the tree without reparsing JSON text.
pub fn parse_json(bytes: &[u8]) -> Result<serde_json::Value, serde_json::Error> {
    struct StrictValue(serde_json::Value);
    impl<'de> Deserialize<'de> for StrictValue {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Check;
            impl<'de> Visitor<'de> for Check {
                type Value = StrictValue;
                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str("JSON without duplicate object keys") }
                fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> { Ok(StrictValue(v.into())) }
                fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> { Ok(StrictValue(v.into())) }
                fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> { Ok(StrictValue(v.into())) }
                fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                    serde_json::Number::from_f64(v).map(|v| StrictValue(v.into())).ok_or_else(|| E::custom("invalid JSON number"))
                }
                fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> { Ok(StrictValue(v.into())) }
                fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> { Ok(StrictValue(v.into())) }
                fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> { Ok(StrictValue(serde_json::Value::Null)) }
                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                    let mut values = Vec::new();
                    while let Some(StrictValue(value)) = seq.next_element()? { values.push(value); }
                    Ok(StrictValue(values.into()))
                }
                fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                    let mut values = serde_json::Map::new();
                    while let Some(key) = map.next_key::<String>()? {
                        if values.contains_key(&key) { return Err(de::Error::custom(format!("duplicate JSON key: {key}"))); }
                        let StrictValue(value) = map.next_value()?;
                        values.insert(key, value);
                    }
                    Ok(StrictValue(values.into()))
                }
            }
            deserializer.deserialize_any(Check)
        }
    }
    serde_json::from_slice::<StrictValue>(bytes).map(|v| v.0)
}

pub fn decode_json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    serde_json::from_value(parse_json(bytes)?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn single_parse_preserves_strictness_and_typed_decoding() {
        for bytes in [br#"{"x":{"a":1,"a":2}}"#.as_slice(), br#"{"a":1,"\u0061":2}"#, b"{} {}", b"[1,]", br#"{"x":1e999}"#] {
            assert!(parse_json(bytes).is_err());
        }
        for bytes in [br#"{"a":null,"b":[true,2,-3,1.25,"text"]}"#.as_slice(), br#"[{"a":1},{"a":2}]"#, b"18446744073709551615"] {
            assert_eq!(parse_json(bytes).unwrap(), serde_json::from_slice::<serde_json::Value>(bytes).unwrap());
        }
        let too_deep = format!("{}0{}", "[".repeat(130), "]".repeat(130));
        assert!(parse_json(too_deep.as_bytes()).is_err());
        assert!(decode_json::<crate::Request>(br#"{"protocol_version":1,"request_id":"r","command":"status","unknown":true}"#).is_err());
        let fixtures: Vec<serde_json::Value> = serde_json::from_str(include_str!("../tests/fixtures/operations.json")).unwrap();
        for fixture in fixtures {
            let bytes = serde_json::to_vec(&fixture).unwrap();
            let before: crate::Operation = serde_json::from_slice(&bytes).unwrap();
            let after: crate::Operation = decode_json(&bytes).unwrap();
            assert_eq!(serde_json::to_value(before).unwrap(), serde_json::to_value(after).unwrap());
        }
    }
    #[test]
    fn duplicate_fields_in_nested_updates_are_rejected() {
        assert!(validate_json(br#"{"values":{"enabled":true,"enabled":false}}"#).is_err());
        assert!(validate_json(br#"{"a":1,"\u0061":2}"#).is_err());
        assert!(validate_json(br#"[{"a":1},{"a":2}]"#).is_ok());
        assert!(validate_json(br#"{"a":null,"b":[true,2,"text"]}"#).is_ok());
    }
}
