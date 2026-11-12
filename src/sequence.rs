//! Deterministic preparation of counter values before any printer job is sent.
//!
//! This module does not print. The caller must render and validate every
//! prepared label, then submit each as a single-copy job. On submission error,
//! stop immediately and report how many jobs were accepted and that the last
//! attempt may have reached the spooler; never retry automatically.
use serde_json::{Value, json};

#[derive(Debug, PartialEq)]
pub struct SequencePlan {
    pub labels: Vec<Vec<Value>>,
}

/// Materialise the counters for one to 100 labels, checking the entire sequence
/// before returning. Non-counter objects are preserved unchanged.
pub fn plan(objects: &[Value], count: u16) -> Result<SequencePlan, String> {
    if !(1..=100).contains(&count) { return Err("Choose between 1 and 100 labels.".into()); }
    if objects.len() > 50 { return Err("A label can contain at most 50 additional objects.".into()); }
    let mut counters = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        let kind = object.get("kind").and_then(Value::as_str).ok_or("An object type is missing.")?;
        if !kind.eq_ignore_ascii_case("counter") { continue; }
        let start = integer_or_default(object, "startAt", 1)?;
        let increment = integer_or_default(object, "increment", 1)?;
        if !(0..=999_999_999).contains(&start) || !(1..=1_000_000).contains(&increment) {
            return Err("Counter start or increment is outside the supported range.".into());
        }
        let last = increment.checked_mul(i64::from(count - 1)).and_then(|offset| start.checked_add(offset))
            .ok_or("The counter sequence is too large.")?;
        if last > 999_999_999 { return Err("The counter sequence exceeds 999999999.".into()); }
        counters.push((index, start, increment));
    }
    if counters.is_empty() { return Err("There is no counter to sequence.".into()); }
    let mut labels = Vec::with_capacity(count as usize);
    for number in 0..count {
        let mut label = objects.to_vec();
        for &(index, start, increment) in &counters {
            label[index]["startAt"] = json!(start + increment * i64::from(number));
            // Each submission is one label; do not request SDK-side advancing.
            label[index]["increment"] = json!(1);
        }
        labels.push(label);
    }
    Ok(SequencePlan { labels })
}

fn integer_or_default(object: &Value, key: &str, default: i64) -> Result<i64, String> {
    match object.get(key) {
        None => Ok(default),
        Some(value) => value.as_i64().ok_or_else(|| format!("Counter {key} must be a whole number.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plans_multiple_counters_and_preserves_other_objects() {
        let objects = vec![json!({"kind":"counter","name":"A","startAt":9,"increment":2,"textWidth":3,"leadingZeros":true}), json!({"kind":"qr","name":"B","value":"example"}), json!({"kind":"counter","name":"C","startAt":100,"increment":5})];
        let result = plan(&objects, 3).unwrap();
        assert_eq!(result.labels.iter().map(|v| v[0]["startAt"].as_i64().unwrap()).collect::<Vec<_>>(), [9, 11, 13]);
        assert_eq!(result.labels.iter().map(|v| v[2]["startAt"].as_i64().unwrap()).collect::<Vec<_>>(), [100, 105, 110]);
        assert_eq!(result.labels[2][1], objects[1]);
        assert_eq!(result.labels[2][0]["textWidth"], 3);
        assert_eq!(result.labels[2][0]["increment"], 1);
        assert_eq!(objects[0]["startAt"], 9);
    }
    #[test]
    fn refuses_invalid_or_overflowing_sequence_before_creating_jobs() {
        assert!(plan(&[json!({"kind":"counter","startAt":999_999_999,"increment":1})],2).is_err());
        assert!(plan(&[json!({"kind":"counter","startAt":-1})],1).is_err());
        assert!(plan(&[json!({"kind":"counter","startAt":"1"})],1).is_err());
        assert!(plan(&[json!({"kind":"counter","increment":0})],1).is_err());
        assert!(plan(&[json!({"kind":"counter"})],0).is_err());
        assert!(plan(&[json!({"kind":"counter"})],101).is_err());
        assert!(plan(&[json!({"kind":"qr","value":"example"})],3).is_err());
        let maximum = plan(&[json!({"kind":"counter","startAt":1,"increment":1})],100).unwrap();
        assert_eq!(maximum.labels.len(), 100);
        assert_eq!(maximum.labels[99][0]["startAt"], 100);
    }
}
