use std::collections::HashMap;

use super::*;

#[test]
fn load_basic_text() {
    let json_text = include_str!("../inkfiles/basictext/oneline.ink.json");

    let parsed: InkJson = serde_json::from_str(json_text).unwrap();

    let actual = InkJson {
        ink_version: 21,
        root: Container {
            flags: None,
            name: None,
            named_content: HashMap::new(),
            contents: vec![
                Element::Container(Container {
                    flags: None,
                    name: None,
                    named_content: HashMap::new(),
                    contents: vec![
                        Element::Value(InkValue::String("Line.".to_string())),
                        Element::Value(InkValue::String("\n".to_string())),
                        Element::Container(Container {
                            flags: None,
                            name: Some("g-0".to_string()),
                            named_content: HashMap::new(),
                            contents: vec![Element::ControlCommand(ControlCommand::Done)],
                        }),
                    ],
                }),
                Element::ControlCommand(ControlCommand::Done),
            ],
        },
    };

    assert_eq!(parsed, actual);
}

#[test]
fn round_trip_basic_text() {
    let json_text = include_str!("../inkfiles/basictext/oneline.ink.json");
    let parsed: InkJson = serde_json::from_str(json_text).unwrap();
    let serialized = serde_json::to_string(&parsed).unwrap();
    let reparsed: InkJson = serde_json::from_str(&serialized).unwrap();
    assert_eq!(parsed, reparsed);
}
