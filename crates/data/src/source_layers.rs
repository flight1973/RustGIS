//! Vector tiles' source layer names (`roads`, `water`) as RustGIS keeps them: an archive's
//! `vector_layers`, and the ones a tile layer hides. One rule for both, so a name the Layers pane
//! lists is one the archive's style can draw and the layer can hide.

use serde_json::Value;

/// The most source layers kept.
pub const MAX: usize = 256;
/// The longest source layer name kept, in characters.
pub const MAX_NAME: usize = 256;

/// Whether a source layer name is kept: not empty, not too long (spelled as given, spaces too).
pub fn is_name(name: &str) -> bool {
    !name.is_empty() && name.chars().count() <= MAX_NAME
}

/// The names kept of `names`: each once, in their order, the first [`MAX`].
pub fn distinct<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut kept: Vec<&str> = Vec::new();
    for name in names.into_iter().filter(|n| is_name(n)) {
        if kept.len() == MAX {
            break;
        }
        if !kept.contains(&name) {
            kept.push(name);
        }
    }
    kept
}

/// The source layers a tile archive's JSON metadata lists (`vector_layers[*].id`), as
/// [`distinct`] keeps them.
pub fn of_metadata(metadata: &Value) -> Vec<String> {
    let ids = metadata.get("vector_layers").and_then(Value::as_array).into_iter().flatten().filter_map(|l| l.get("id")?.as_str());
    distinct(ids).into_iter().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Names are kept as spelled, each once in their order; empty and overlong ones, and ids that
    /// aren't text, don't count toward the cap.
    #[test]
    fn names_are_kept_once_in_order_up_to_the_cap() {
        let long = "é".repeat(MAX_NAME + 1);
        let edge = "é".repeat(MAX_NAME);
        let mut layers = vec![json!({"id": "water"}), json!({"id": 3}), json!({}), json!({"id": " roads "}), json!({"id": ""})];
        layers.extend((0..MAX + 10).map(|i| json!({"id": format!("{long}{i}")})));
        layers.extend([json!({"id": "water"}), json!({"id": edge.clone()}), json!({"id": "pois"})]);
        assert_eq!(of_metadata(&json!({"vector_layers": layers})), ["water", " roads ", &edge, "pois"]);
        // At most MAX, the first ones.
        let many: Vec<String> = (0..MAX + 5).map(|i| format!("l{i}")).collect();
        let kept = distinct(many.iter().map(String::as_str));
        assert_eq!((kept.len(), kept.first().copied(), kept.last().copied()), (MAX, Some("l0"), Some("l255")));
        // Metadata without a list of layers lists none.
        for none in [json!(null), json!({"vector_layers": "roads"}), json!({})] {
            assert!(of_metadata(&none).is_empty());
        }
    }
}
