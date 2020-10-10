use proptest::prelude::*;
use serde_json::value::Value;
pub fn arb_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<f64>().prop_map(|f| f.into()),
        ".*".prop_map(Value::String),
    ];
    leaf.prop_recursive(
        8,   // 8 levels deep
        256, // Shoot for maximum size of 256 nodes
        10,  // We put up to 10 items per collection
        |inner| {
            prop_oneof![
                // Take the inner strategy and make the two recursive cases.
                prop::collection::vec(inner.clone(), 0..10).prop_map(Value::Array),
                prop::collection::hash_map(".*", inner, 0..10)
                    .prop_map(|m| { Value::Object(m.into_iter().collect()) }),
            ]
        },
    )
}
