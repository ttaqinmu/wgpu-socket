use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub rotation: [f32; 3],
    pub scale: f32,
    pub color: [f32; 3],
    pub revision: u64,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            rotation: [25.0, 35.0, 0.0],
            scale: 1.0,
            color: [0.24, 0.65, 0.95],
            revision: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    SetRotation { value: [f32; 3] },
    SetScale { value: f32 },
    SetColor { value: [f32; 3] },
    State { scene: Scene },
}

impl Scene {
    pub fn apply(&mut self, message: &Message) -> bool {
        let changed = match message {
            Message::SetRotation { value } if value.iter().all(|x| x.is_finite()) => {
                let value = value.map(|x| x.clamp(-360.0, 360.0));
                let changed = self.rotation != value;
                self.rotation = value;
                changed
            }
            Message::SetScale { value } if value.is_finite() => {
                let value = value.clamp(0.2, 3.0);
                let changed = self.scale != value;
                self.scale = value;
                changed
            }
            Message::SetColor { value } if value.iter().all(|x| x.is_finite()) => {
                let value = value.map(|x| x.clamp(0.0, 1.0));
                let changed = self.color != value;
                self.color = value;
                changed
            }
            _ => false,
        };
        if changed {
            self.revision += 1;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        for message in [
            Message::SetRotation {
                value: [1.0, 2.0, 3.0],
            },
            Message::SetScale { value: 1.5 },
            Message::SetColor {
                value: [0.1, 0.2, 0.3],
            },
            Message::State {
                scene: Scene::default(),
            },
        ] {
            let encoded = serde_json::to_string(&message).unwrap();
            assert_eq!(serde_json::from_str::<Message>(&encoded).unwrap(), message);
        }
    }

    #[test]
    fn updates_are_per_property_and_ordered() {
        let mut scene = Scene::default();
        assert!(scene.apply(&Message::SetColor {
            value: [1.0, 0.0, 0.0]
        }));
        assert!(scene.apply(&Message::SetScale { value: 2.0 }));
        assert_eq!(scene.color, [1.0, 0.0, 0.0]);
        assert_eq!(scene.scale, 2.0);
        assert_eq!(scene.revision, 2);
        assert!(!scene.apply(&Message::SetScale { value: f32::NAN }));
        assert_eq!(scene.revision, 2);
    }
}
