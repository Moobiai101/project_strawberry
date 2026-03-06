//! Inspector panel

use egui::Ui;

/// Inspector panel for editing selected entity properties
pub struct InspectorPanel;

impl InspectorPanel {
    pub fn show(ui: &mut Ui) {
        ui.heading("Inspector");
        ui.label("Select an entity to inspect");
        // TODO: Show properties of selected entity
    }
}
