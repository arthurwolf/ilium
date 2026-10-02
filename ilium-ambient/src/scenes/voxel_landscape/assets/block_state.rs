//! Semantic block identities are not texture names. Every supplied property is
//! part of equality, serialization, selection and deterministic variant choice.
use super::{
    error::{AssetError, Result},
    identity::{Digest256, ResourceId},
    metadata::{self, allowed, array, boolean, object, required, text, uint},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "StateWire", into = "StateWire")]
pub struct BlockState {
    id: ResourceId,
    properties: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateWire {
    id: ResourceId,
    #[serde(default, deserialize_with = "deserialize_properties")]
    properties: BTreeMap<String, String>,
}
fn deserialize_properties<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, String>, D::Error> {
    struct PropertyVisitor;
    impl<'de> serde::de::Visitor<'de> for PropertyVisitor {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("at most 32 unique block properties")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut values = BTreeMap::new();
            while let Some(key) = map.next_key::<String>()? {
                if values.len() >= 32 || !property_atom(&key) || values.contains_key(&key) {
                    return Err(serde::de::Error::custom(
                        "invalid or duplicate block property",
                    ));
                }
                let value = map.next_value::<String>()?;
                if !property_atom(&value) {
                    return Err(serde::de::Error::custom("invalid block property value"));
                }
                values.insert(key, value);
            }
            Ok(values)
        }
    }
    deserializer.deserialize_map(PropertyVisitor)
}
fn property_atom(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'.' | b':')
        })
}
impl BlockState {
    pub fn new(
        id: ResourceId,
        properties: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self> {
        let mut sorted = BTreeMap::new();
        for (key, value) in properties {
            if !property_atom(&key) || !property_atom(&value) {
                return Err(metadata::invalid("invalid block property atom"));
            }
            if sorted.len() >= 32 {
                return Err(metadata::invalid("more than 32 block properties"));
            }
            if sorted.insert(key.clone(), value).is_some() {
                return Err(AssetError::Duplicate(format!("block property {key}")));
            }
        }
        Ok(Self {
            id,
            properties: sorted,
        })
    }
    pub fn id(&self) -> &ResourceId {
        &self.id
    }
    pub fn properties(&self) -> &BTreeMap<String, String> {
        &self.properties
    }
    pub fn property(&self, name: &str) -> Option<&str> {
        self.properties.get(name).map(String::as_str)
    }
    pub fn canonical(&self) -> String {
        let values: Vec<_> = self
            .properties
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        format!("{}[{}]", self.id, values.join(","))
    }
    pub fn fingerprint(&self) -> Digest256 {
        Digest256::of(self.canonical().as_bytes())
    }
    /// Quarter turns about world vertical. The state convention is Java X/Z
    /// ground, not the old world module's second ground-axis parameter naming.
    pub fn rotated(&self, turns: u8) -> Result<Self> {
        let turns = turns % 4;
        let mut properties = self.properties.clone();
        if let Some(facing) = self.property("facing") {
            let directions = ["north", "east", "south", "west"];
            if let Some(index) = directions.iter().position(|value| *value == facing) {
                properties.insert(
                    "facing".into(),
                    directions[(index + usize::from(turns)) % 4].into(),
                );
            }
        }
        if turns % 2 == 1 {
            match self.property("axis") {
                Some("x") => {
                    properties.insert("axis".into(), "z".into());
                }
                Some("z") => {
                    properties.insert("axis".into(), "x".into());
                }
                _ => {}
            }
        }
        // Cardinal connection properties (fences, vines) rotate as keys too.
        let directions = ["north", "east", "south", "west"];
        for direction in directions {
            properties.remove(direction);
        }
        for (index, direction) in directions.iter().enumerate() {
            if let Some(value) = self.property(direction) {
                properties.insert(
                    directions[(index + usize::from(turns)) % 4].into(),
                    value.into(),
                );
            }
        }
        Self::new(self.id.clone(), properties)
    }
}
impl TryFrom<StateWire> for BlockState {
    type Error = AssetError;
    fn try_from(value: StateWire) -> Result<Self> {
        Self::new(value.id, value.properties)
    }
}
impl From<BlockState> for StateWire {
    fn from(value: BlockState) -> Self {
        Self {
            id: value.id,
            properties: value.properties,
        }
    }
}
#[derive(Clone, Debug)]
pub enum Condition {
    Always,
    Property {
        key: String,
        values: Vec<String>,
        inverted: bool,
    },
    All(Vec<Condition>),
    Any(Vec<Condition>),
}
impl Condition {
    pub fn matches(&self, state: &BlockState) -> bool {
        match self {
            Self::Always => true,
            Self::Property {
                key,
                values,
                inverted,
            } => state
                .property(key)
                .is_some_and(|value| values.iter().any(|v| v == value) != *inverted),
            Self::All(values) => values.iter().all(|value| value.matches(state)),
            Self::Any(values) => values.iter().any(|value| value.matches(state)),
        }
    }
    fn parse_property(key: &str, value: &str) -> Result<Self> {
        if !property_atom(key) {
            return Err(metadata::invalid("bad selector property"));
        }
        let (inverted, value) = match value.strip_prefix('!') {
            Some(value) => (true, value),
            None => (false, value),
        };
        let values: Vec<_> = value.split('|').map(str::to_owned).collect();
        if values.len() > 32 || values.iter().any(|value| !property_atom(value)) {
            return Err(metadata::invalid("bad selector value"));
        }
        Ok(Self::Property {
            key: key.into(),
            values,
            inverted,
        })
    }
    fn variant(value: &str) -> Result<Self> {
        if value.is_empty() {
            return Ok(Self::Always);
        }
        let mut conditions = Vec::new();
        let mut seen = BTreeMap::new();
        for pair in value.split(',') {
            if conditions.len() >= 32 {
                return Err(metadata::invalid("variant predicate limit"));
            }
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| metadata::invalid("variant requires key=value"))?;
            if seen.insert(key, ()).is_some() {
                return Err(metadata::invalid("duplicate variant property"));
            }
            conditions.push(Self::parse_property(key, value)?);
        }
        Ok(Self::All(conditions))
    }
    fn multipart(value: &Value, depth: usize) -> Result<Self> {
        if depth > 16 {
            return Err(metadata::invalid("multipart condition depth limit"));
        }
        let fields = object(value)?;
        if fields.len() > 32 {
            return Err(metadata::invalid("multipart predicate count"));
        }
        let mut conditions = Vec::new();
        for (key, value) in fields {
            if key == "OR" || key == "AND" {
                let items = array(value)?;
                if items.is_empty() || items.len() > 64 {
                    return Err(metadata::invalid("empty or oversized multipart logic"));
                }
                let values = items
                    .iter()
                    .map(|v| Self::multipart(v, depth + 1))
                    .collect::<Result<Vec<_>>>()?;
                conditions.push(if key == "OR" {
                    Self::Any(values)
                } else {
                    Self::All(values)
                });
                continue;
            }
            conditions.push(Self::parse_property(key, text(value)?)?);
        }
        Ok(Self::All(conditions))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelApplication {
    pub model: ResourceId,
    pub x_turns: u8,
    pub y_turns: u8,
    pub uvlock: bool,
    pub weight: u32,
}
impl ModelApplication {
    fn parse(value: &Value) -> Result<Self> {
        let fields = object(value)?;
        allowed(fields, &["model", "x", "y", "uvlock", "weight"])?;
        let mut turns = [0_u8; 2];
        for (axis, key) in ["x", "y"].iter().enumerate() {
            let degrees = fields.get(*key).map(uint).transpose()?.unwrap_or(0);
            if degrees > 270 || degrees % 90 != 0 {
                return Err(metadata::invalid("model rotation must be 0/90/180/270"));
            }
            turns[axis] = (degrees / 90) as u8;
        }
        let weight = fields.get("weight").map(uint).transpose()?.unwrap_or(1);
        if weight == 0 || weight > 1_000_000 {
            return Err(metadata::invalid("model weight outside 1..1000000"));
        }
        Ok(Self {
            model: ResourceId::parse(text(required(fields, "model")?)?)?,
            x_turns: turns[0],
            y_turns: turns[1],
            uvlock: fields
                .get("uvlock")
                .map(boolean)
                .transpose()?
                .unwrap_or(false),
            weight,
        })
    }
}
#[derive(Clone, Debug)]
struct Choice {
    condition: Condition,
    models: Vec<ModelApplication>,
}
#[derive(Clone, Debug)]
pub struct StateDefinition {
    variants: Vec<Choice>,
    multipart: Vec<Choice>,
}
fn applications(value: &Value) -> Result<Vec<ModelApplication>> {
    let values = match value {
        Value::Array(values) => values.as_slice(),
        _ => std::slice::from_ref(value),
    };
    if values.is_empty() || values.len() > 256 {
        return Err(metadata::invalid("empty or oversized model alternatives"));
    }
    values.iter().map(ModelApplication::parse).collect()
}
impl StateDefinition {
    pub fn parse(value: &Value) -> Result<Self> {
        let fields = object(value)?;
        allowed(fields, &["variants", "multipart"])?;
        if fields.contains_key("variants") == fields.contains_key("multipart") {
            return Err(metadata::invalid(
                "blockstate requires exactly one of variants or multipart",
            ));
        }
        let mut variants = Vec::new();
        let mut multipart = Vec::new();
        if let Some(value) = fields.get("variants") {
            let entries = object(value)?;
            if entries.is_empty() || entries.len() > 4096 {
                return Err(metadata::invalid("variant count"));
            }
            for (predicate, models) in entries {
                variants.push(Choice {
                    condition: Condition::variant(predicate)?,
                    models: applications(models)?,
                });
            }
        }
        if let Some(value) = fields.get("multipart") {
            let entries = array(value)?;
            if entries.is_empty() || entries.len() > 256 {
                return Err(metadata::invalid("multipart count"));
            }
            for entry in entries {
                let fields = object(entry)?;
                allowed(fields, &["when", "apply"])?;
                let condition = fields
                    .get("when")
                    .map(|v| Condition::multipart(v, 0))
                    .transpose()?
                    .unwrap_or(Condition::Always);
                multipart.push(Choice {
                    condition,
                    models: applications(required(fields, "apply")?)?,
                });
            }
        }
        Ok(Self {
            variants,
            multipart,
        })
    }
    pub fn select(
        &self,
        state: &BlockState,
        anchor: [i32; 3],
        seed: u64,
    ) -> Result<Vec<ModelApplication>> {
        let matching: Vec<_> = self
            .variants
            .iter()
            .enumerate()
            .filter(|(_, choice)| choice.condition.matches(state))
            .collect();
        if !self.variants.is_empty() && matching.len() != 1 {
            return Err(metadata::invalid("missing or ambiguous blockstate variant"));
        }
        let mut output = Vec::new();
        for (index, entry) in matching.into_iter().chain(
            self.multipart
                .iter()
                .enumerate()
                .filter(|(_, choice)| choice.condition.matches(state)),
        ) {
            let total: u64 = entry
                .models
                .iter()
                .map(|model| u64::from(model.weight))
                .sum();
            let mut key = state.canonical().into_bytes();
            for coordinate in anchor {
                key.extend(coordinate.to_le_bytes());
            }
            key.extend(seed.to_le_bytes());
            key.extend((index as u64).to_le_bytes());
            let hash = Digest256::of(&key).bytes();
            let mut pick = u64::from_le_bytes([
                hash[0], hash[1], hash[2], hash[3], hash[4], hash[5], hash[6], hash[7],
            ]) % total;
            for model in &entry.models {
                if pick < u64::from(model.weight) {
                    output.push(model.clone());
                    break;
                }
                pick -= u64::from(model.weight);
            }
        }
        // Empty multipart is meaningful (no connected side), unlike a missing definition.
        Ok(output)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn state(properties: &[(&str, &str)]) -> BlockState {
        BlockState::new(
            ResourceId::parse("oak_log").unwrap(),
            properties
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string())),
        )
        .unwrap()
    }
    #[test]
    fn exact_properties_distinguish_states_and_round_trip_in_canonical_order() {
        let a = state(&[("axis", "x"), ("waterlogged", "false")]);
        let b = state(&[("waterlogged", "false"), ("axis", "x")]);
        assert_eq!(a, b);
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_ne!(a, state(&[("axis", "y"), ("waterlogged", "false")]));
        assert_eq!(
            serde_json::from_str::<BlockState>(&serde_json::to_string(&a).unwrap()).unwrap(),
            a
        );
        assert!(BlockState::new(
            ResourceId::parse("oak_log").unwrap(),
            [("axis".into(), "x".into()), ("axis".into(), "y".into())]
        )
        .is_err());
    }
    #[test]
    fn quarter_turn_changes_axis_facing_and_connections_without_erasing_growth() {
        let input = state(&[
            ("axis", "x"),
            ("facing", "north"),
            ("north", "true"),
            ("age", "3"),
        ]);
        let output = input.rotated(1).unwrap();
        assert_eq!(output.property("axis"), Some("z"));
        assert_eq!(output.property("facing"), Some("east"));
        assert_eq!(output.property("east"), Some("true"));
        assert_eq!(output.property("age"), Some("3"));
        assert_eq!(output.rotated(3).unwrap(), input);
    }
    #[test]
    fn multipart_conditions_are_cumulative_and_variants_are_not_first_match() {
        let definition = StateDefinition::parse(&serde_json::json!({"multipart":[
            {"apply":{"model":"block/post"}},
            {"when":{"OR":[{"axis":"x"},{"axis":"z"}]},"apply":{"model":"block/side","y":90}},
            {"when":{"axis":"y"},"apply":{"model":"block/cap"}}
        ]}))
        .unwrap();
        let selected = definition
            .select(&state(&[("axis", "x")]), [-32, 7, 5], 17)
            .unwrap();
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[1].model.as_str(), "minecraft:block/side");
        let ambiguous = StateDefinition::parse(
            &serde_json::json!({"variants":{"":{"model":"block/a"},"axis=x":{"model":"block/b"}}}),
        )
        .unwrap();
        assert!(ambiguous
            .select(&state(&[("axis", "x")]), [0; 3], 1)
            .is_err());
    }
    #[test]
    fn weighted_choices_are_reproducible_and_not_constant() {
        let definition = StateDefinition::parse(&serde_json::json!({"variants":{"": [{"model":"block/a","weight":1},{"model":"block/b","weight":3}]}})).unwrap();
        let input = state(&[]);
        let first: Vec<_> = (-40..40)
            .map(|x| definition.select(&input, [x, -1, 7], 123).unwrap())
            .collect();
        let second: Vec<_> = (-40..40)
            .map(|x| definition.select(&input, [x, -1, 7], 123).unwrap())
            .collect();
        assert_eq!(first, second);
        assert!(first.windows(2).any(|pair| pair[0] != pair[1]));
    }
}
