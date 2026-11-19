use serde::{Deserialize, Serialize};
pub const PERSONAL: &str = "00000000-0000-4000-8000-000000000001";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Appearance {
    pub wallpaper: String,
    pub blur: f64,
    pub transparency: f64,
    pub veil_light: f64,
    pub veil_dark: f64,
}
impl Default for Appearance {
    fn default() -> Self {
        Self { wallpaper: "none".into(), blur: 0.0, transparency: 50.0, veil_light: 0.18, veil_dark: 0.32 }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Symbol {
    pub kind: String,
    pub value: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tile {
    pub color: String,
    pub style: String,
    pub symbol: Symbol,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerPresentation {
    pub name: String,
    pub initials: String,
    pub photo: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Space {
    pub id: String,
    #[serde(default)]
    pub principal_bot_id: Option<String>,
    pub kind: String,
    pub name: String,
    pub position: i64,
    #[serde(default)]
    pub appearance_configured: bool,
    pub revision: i64,
    pub tile: Option<Tile>,
    pub appearance: Appearance,
    pub owner: Option<OwnerPresentation>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub version: u32,
    pub personal_space_id: String,
    pub revision: i64,
    pub spaces: Vec<Space>,
}
pub fn personal_id() -> String {
    PERSONAL.into()
}
