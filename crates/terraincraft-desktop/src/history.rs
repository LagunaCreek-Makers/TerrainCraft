//! Undo and redo for project settings, as snapshots of the saved project state.
use crate::studio::Studio;
use serde_json::Value;
use std::time::{Duration, Instant};

/// Saved fields that are navigation or bookkeeping rather than edits: they are neither
/// recorded nor changed by undo. `path` stays because the loaded terrain belongs to it.
const NOT_UNDONE: [&str; 10] = [
    "path",
    "place_names",
    "project_path",
    "message",
    "view",
    "sheet",
    "map_center_lat",
    "map_center_lon",
    "map_zoom",
    "search_query",
];

/// How long settings must stay unchanged before a change becomes one undo step, so a
/// slider drag or a typed word is a single step.
const SETTLE: Duration = Duration::from_millis(400);
const LIMIT: usize = 100;

#[derive(Default)]
pub(crate) struct History {
    undo: Vec<Value>,
    redo: Vec<Value>,
    /// The state the stacks are relative to.
    committed: Option<Value>,
    /// The latest differing state and when it was first seen unchanged.
    pending: Option<(Value, Instant)>,
}

/// The undoable part of a project.
pub(crate) fn snapshot(s: &Studio) -> Value {
    let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
    if let Some(map) = v.as_object_mut() {
        for k in NOT_UNDONE {
            map.remove(k);
        }
    }
    v
}

impl History {
    /// Record `current` once it has stayed the same for a moment while nothing is being
    /// dragged.
    pub(crate) fn observe(&mut self, current: Value, now: Instant, interacting: bool) {
        let Some(committed) = &self.committed else {
            self.committed = Some(current);
            return;
        };
        if *committed == current {
            self.pending = None;
            return;
        }
        match &self.pending {
            Some((v, since)) if *v == current => {
                if !interacting && now.duration_since(*since) >= SETTLE {
                    let old = self.committed.replace(current).expect("checked above");
                    self.undo.push(old);
                    if self.undo.len() > LIMIT {
                        self.undo.remove(0);
                    }
                    self.redo.clear();
                    self.pending = None;
                }
            }
            _ => self.pending = Some((current, now)),
        }
    }
    /// Whether a change is waiting to settle (the caller should repaint to record it).
    pub(crate) fn settling(&self) -> bool {
        self.pending.is_some()
    }
    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.pending.is_some()
    }
    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    /// The state to return to, given the `current` one.
    pub(crate) fn undo(&mut self, current: Value) -> Option<Value> {
        // An unsettled change is undone back to the last recorded state.
        if self.pending.take().is_some() {
            self.redo.push(current);
            return self.committed.clone();
        }
        let prev = self.undo.pop()?;
        self.redo
            .push(self.committed.replace(prev.clone()).unwrap_or(current));
        Some(prev)
    }
    pub(crate) fn redo(&mut self) -> Option<Value> {
        let next = self.redo.pop()?;
        if let Some(c) = self.committed.replace(next.clone()) {
            self.undo.push(c);
        }
        self.pending = None;
        Some(next)
    }
}

impl Studio {
    pub(crate) fn undo(&mut self) {
        let current = snapshot(self);
        if let Some(state) = self.history.undo(current) {
            self.message = self
                .restore(state)
                .map_or_else(|e| format!("Undo failed: {e}"), |()| "Undone".into());
        }
    }
    pub(crate) fn redo(&mut self) {
        if let Some(state) = self.history.redo() {
            self.message = self
                .restore(state)
                .map_or_else(|e| format!("Redo failed: {e}"), |()| "Redone".into());
        }
    }
    /// Replace the undoable settings with `state`, keeping navigation, the loaded terrain
    /// and everything else that is not part of the project file.
    pub(crate) fn restore(&mut self, state: Value) -> Result<(), String> {
        let mut merged = serde_json::to_value(&*self).map_err(|e| e.to_string())?;
        if let (Some(m), Value::Object(s)) = (merged.as_object_mut(), state) {
            for (k, v) in s {
                m.insert(k, v);
            }
        }
        let mut restored: Studio = serde_json::from_value(merged).map_err(|e| e.to_string())?;
        restored.adopt_runtime(self);
        *self = restored;
        Ok(())
    }

    /// Move the fields that are not saved in project files (loaded data, caches, open
    /// downloads, textures, camera) from `old`.
    fn adopt_runtime(&mut self, old: &mut Studio) {
        use std::mem::take;
        self.dem = old.dem.take();
        self.source_dem = old.source_dem.take();
        self.applied_prep = old.applied_prep.take();
        self.last_3mf = old.last_3mf.take();
        self.laser_cache = take(&mut old.laser_cache);
        self.engraving_cache = take(&mut old.engraving_cache);
        self.map_select_start = old.map_select_start.take();
        self.map_aiming = old.map_aiming;
        self.map_synced = old.map_synced;
        self.map_selected = old.map_selected;
        self.map_refit = old.map_refit;
        self.search_results = take(&mut old.search_results);
        self.search_rx = old.search_rx.take();
        self.names_rx = old.names_rx.take();
        self.orbit_yaw = old.orbit_yaw;
        self.orbit_pitch = old.orbit_pitch;
        self.orbit_zoom = old.orbit_zoom;
        self.download_rx = old.download_rx.take();
        self.map_results = old.map_results.take();
        self.map_textures = take(&mut old.map_textures);
        self.map_pending = take(&mut old.map_pending);
        self.map_requested = take(&mut old.map_requested);
        self.map_tx = old.map_tx.take();
        self.history = take(&mut old.history);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64, base: Instant) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn records_settled_changes_and_undoes_and_redoes_them() {
        let t = Instant::now();
        let mut h = History::default();
        let v = |n: i64| serde_json::json!({ "width": n });
        h.observe(v(1), at(0, t), false);
        // A burst of changes (dragging a slider) becomes one step once it settles.
        h.observe(v(2), at(10, t), true);
        h.observe(v(3), at(20, t), true);
        h.observe(v(3), at(500, t), true);
        assert!(h.undo.is_empty(), "nothing recorded while dragging");
        h.observe(v(3), at(900, t), false);
        assert_eq!(h.undo, vec![v(1)]);
        h.observe(v(4), at(1000, t), false);
        h.observe(v(4), at(1500, t), false);
        assert_eq!(h.undo, vec![v(1), v(3)]);
        assert_eq!(h.undo(v(4)), Some(v(3)));
        assert_eq!(h.undo(v(3)), Some(v(1)));
        assert_eq!(h.undo(v(1)), None);
        assert_eq!(h.redo(), Some(v(3)));
        assert_eq!(h.redo(), Some(v(4)));
        assert!(!h.can_redo());
        // A new change after undo clears redo.
        h.undo(v(4));
        h.observe(v(9), at(2000, t), false);
        h.observe(v(9), at(2500, t), false);
        assert!(!h.can_redo());
    }

    #[test]
    fn unsettled_change_is_undone_immediately() {
        let t = Instant::now();
        let mut h = History::default();
        let v = |n: i64| serde_json::json!({ "width": n });
        h.observe(v(1), at(0, t), false);
        h.observe(v(2), at(10, t), false);
        assert!(h.can_undo());
        assert_eq!(h.undo(v(2)), Some(v(1)));
        assert_eq!(h.redo(), Some(v(2)));
    }

    #[test]
    fn restore_keeps_navigation_and_loaded_data() {
        let mut s = Studio::default();
        let before = snapshot(&s);
        s.width = 123.;
        s.map_center_lat = 10.;
        s.path = "terrain.tif".into();
        s.dem = Some(terraincraft_core::Dem::new(2, 2, vec![0.; 4]).unwrap());
        s.restore(before).unwrap();
        assert_eq!(s.width, Studio::default().width);
        assert_eq!((s.map_center_lat, s.path.as_str()), (10., "terrain.tif"));
        assert!(s.dem.is_some());
    }
}
