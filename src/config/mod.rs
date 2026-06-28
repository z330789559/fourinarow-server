use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct ModeConfig {
    pub mode: i32,
    pub name: String,
    #[serde(rename = "unlockByJourneyLevel")]
    pub unlock_by_journey_level: i32,
    #[serde(rename = "firstLevelId")]
    pub first_level_id: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LevelConfig {
    pub id: i32,
    pub mode: i32,
    #[serde(rename = "levelMin")]
    pub level_min: i32,
    #[serde(rename = "levelMax")]
    pub level_max: i32,
    pub chapter: i32,
    #[serde(rename = "ammoCount")]
    pub ammo_count: i32,
    #[serde(rename = "tripleStar")]
    pub triple_star: i32,
    #[serde(rename = "doubleStar")]
    pub double_star: i32,
    #[serde(rename = "singleStar")]
    pub single_star: i32,
    #[serde(rename = "sceneId")]
    pub scene_id: i32,
}

#[derive(Debug, Clone)]
pub struct GameConfig {
    pub modes: Vec<ModeConfig>,
    /// Levels sorted by `id` ascending within each mode (key = mode number).
    pub levels_by_mode: HashMap<i32, Vec<LevelConfig>>,
    pub level_count_by_mode: HashMap<i32, usize>,
}

impl GameConfig {
    /// Fail-fast: panics on missing/invalid config files (intentional, per design).
    pub fn load() -> Self {
        let mode_data = std::fs::read_to_string("config/MinigameModeConfigData.json")
            .expect("failed to read config/MinigameModeConfigData.json");
        let level_data = std::fs::read_to_string("config/MinigameLevelConfigData.json")
            .expect("failed to read config/MinigameLevelConfigData.json");

        let modes: Vec<ModeConfig> = serde_json::from_str(&mode_data)
            .expect("failed to parse MinigameModeConfigData.json");
        let mut all_levels: Vec<LevelConfig> = serde_json::from_str(&level_data)
            .expect("failed to parse MinigameLevelConfigData.json");

        // Sort by (mode, id) ascending — id is the canonical identity for a level within a mode.
        all_levels.sort_by_key(|l| (l.mode, l.id));

        let mut levels_by_mode: HashMap<i32, Vec<LevelConfig>> = HashMap::new();
        for level in all_levels {
            levels_by_mode.entry(level.mode).or_default().push(level);
        }

        let level_count_by_mode = levels_by_mode
            .iter()
            .map(|(mode, levels)| (*mode, levels.len()))
            .collect();

        GameConfig {
            modes,
            levels_by_mode,
            level_count_by_mode,
        }
    }

    pub fn levels_in_mode(&self, mode: i32) -> &[LevelConfig] {
        self.levels_by_mode
            .get(&mode)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn level_by_id(&self, mode: i32, id: i32) -> Option<&LevelConfig> {
        self.levels_by_mode
            .get(&mode)?
            .iter()
            .find(|l| l.id == id)
    }

    pub fn mode_config(&self, mode: i32) -> Option<&ModeConfig> {
        self.modes.iter().find(|m| m.mode == mode)
    }

    /// Returns the id of the level at `after_id + 1` position and subsequent levels (for pagination).
    pub fn levels_after(&self, mode: i32, after_id: i32, page_size: usize) -> &[LevelConfig] {
        let levels = self.levels_in_mode(mode);
        let start = levels.partition_point(|l| l.id <= after_id);
        let end = (start + page_size).min(levels.len());
        &levels[start..end]
    }
}
