use std::sync::{OnceLock, RwLock};
use crate::bindings::TransformFfi;

pub trait SceneBridge: Send + Sync + 'static {
    fn get_transform(&self, entity_id: u64) -> Option<TransformFfi>;
    fn set_transform(&self, entity_id: u64, transform: TransformFfi) -> bool;
}

struct NullBridge;
impl SceneBridge for NullBridge {
    fn get_transform(&self, _: u64) -> Option<TransformFfi> {
        None
    }
    fn set_transform(&self, _: u64, _: TransformFfi) -> bool {
        false
    }
}

static SCENE_BRIDGE: OnceLock<RwLock<Box<dyn SceneBridge>>> = OnceLock::new();

fn get_bridge_cell() -> &'static RwLock<Box<dyn SceneBridge>> {
    SCENE_BRIDGE.get_or_init(|| RwLock::new(Box::new(NullBridge)))
}

/// Registers the engine scene bridge connecting C# runtime calls to the actual scene entities.
pub fn register_scene_bridge(bridge: Box<dyn SceneBridge>) {
    if let Ok(mut cell) = get_bridge_cell().write() {
        *cell = bridge;
    }
}

pub(crate) fn read_entity_transform(entity_id: u64) -> Option<TransformFfi> {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.get_transform(entity_id)
    } else {
        None
    }
}

pub(crate) fn write_entity_transform(entity_id: u64, transform: TransformFfi) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.set_transform(entity_id, transform)
    } else {
        false
    }
}
