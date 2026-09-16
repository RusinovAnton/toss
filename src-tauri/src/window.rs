//! Window geometry: remembered across launches, and kept square.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "window.json";
/// Below this the radar has no room for a second orbit.
pub const MIN_SIDE: u32 = 360;
/// How often the geometry may be written while the window is being dragged.
pub const SAVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    /// The window is square, so one side is enough.
    pub side: u32,
}

impl Geometry {
    pub fn file_path(dir: &Path) -> PathBuf {
        dir.join(FILE_NAME)
    }

    pub fn load(dir: &Path) -> Option<Geometry> {
        let bytes = std::fs::read(Geometry::file_path(dir)).ok()?;
        let geometry: Geometry = serde_json::from_slice(&bytes).ok()?;
        geometry.is_sane().then_some(geometry)
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{FILE_NAME}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(&tmp, Geometry::file_path(dir))
    }

    /// Guards against a stored geometry that would open the window off screen
    /// or at an unusable size, e.g. after a monitor was unplugged.
    fn is_sane(&self) -> bool {
        self.side >= MIN_SIDE
            && self.side <= 8000
            && self.x > -20_000
            && self.x < 20_000
            && self.y > -20_000
            && self.y < 20_000
    }
}

/// The side to snap a freshly resized window to.
///
/// `None` means it is already square enough to leave alone; resizing in
/// response to every pixel would fight the user's drag.
pub fn square_side(width: u32, height: u32) -> Option<u32> {
    let side = width.max(height).max(MIN_SIDE);
    let difference = width.abs_diff(height);
    if difference <= 2 && width >= MIN_SIDE {
        return None;
    }
    Some(side)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("toss-window-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn geometry_survives_a_round_trip() {
        let dir = temp_dir();
        let geometry = Geometry {
            x: 120,
            y: 80,
            side: 520,
        };
        geometry.save(&dir).unwrap();
        assert_eq!(Geometry::load(&dir), Some(geometry));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_or_absurd_geometry_is_ignored() {
        let dir = temp_dir();
        assert_eq!(Geometry::load(&dir), None);
        Geometry {
            x: 0,
            y: 0,
            side: 10,
        }
        .save(&dir)
        .unwrap();
        assert_eq!(Geometry::load(&dir), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_off_screen_position_is_ignored() {
        let dir = temp_dir();
        Geometry {
            x: -99_000,
            y: 0,
            side: 480,
        }
        .save(&dir)
        .unwrap();
        assert_eq!(Geometry::load(&dir), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_square_window_is_left_alone() {
        assert_eq!(square_side(480, 480), None);
        assert_eq!(square_side(481, 480), None);
    }

    #[test]
    fn a_stretched_window_snaps_to_its_longer_side() {
        assert_eq!(square_side(600, 480), Some(600));
        assert_eq!(square_side(480, 700), Some(700));
    }

    #[test]
    fn the_minimum_is_respected() {
        assert_eq!(square_side(200, 300), Some(360));
        assert_eq!(square_side(100, 100), Some(360));
    }
}
