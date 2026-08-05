//! Map model and related helpers.

use crate::types::{DisplayName, MapId, Position};

/// Basic map representation. Keep compact — details like tiles, zones, and
/// navigation meshes are outside the internal data-model scope for now.
#[derive(Clone, Debug)]
pub struct Map {
    pub id: MapId,
    pub name: DisplayName,
    /// Optional textual description of the map
    pub description: Option<String>,
    /// Bounding box of the map as min and max positions.
    pub bounds_min: Option<Position>,
    pub bounds_max: Option<Position>,
}

impl Map {
    pub fn new(id: MapId, name: impl Into<DisplayName>) -> Self {
        Self { id, name: name.into(), description: None, bounds_min: None, bounds_max: None }
    }
}
