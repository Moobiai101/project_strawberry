//! # Gizmo Overlay Renderer
//!
//! Draws the transform gizmo overlay on top of the viewport.
//! Handles rendering of bounding boxes, scale handles, rotation handle,
//! anchor point, smart guides, distance indicators, and cursor management.
//!
//! ## Design
//!
//! This module is purely rendering — no state mutation. It reads gizmo state
//! and selected entity data, then draws directly onto the egui painter.
//! All drawing uses screen-space coordinates (converted from scene-space
//! via `CoordinateTransform`).

use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use strawberry_core::gizmo::coordinate_transform::CoordinateTransform;
use strawberry_core::gizmo::hit_testing::{BoundingCorners, GizmoHandle};
use strawberry_core::gizmo::snapping::{DistanceIndicator, GuideKind, GuideLine};

// ============================================================================
// CONSTANTS — Visual styling for the gizmo overlay
// ============================================================================

/// Color of the bounding box outline (CapCut-style blue)
const BOUNDING_BOX_COLOR: Color32 = Color32::from_rgb(60, 140, 255);
/// Width of the bounding box stroke
const BOUNDING_BOX_STROKE_WIDTH: f32 = 1.5;

/// Color for scale handles (white fill with dark stroke)
const HANDLE_FILL_COLOR: Color32 = Color32::WHITE;
/// Color for handle border
const HANDLE_STROKE_COLOR: Color32 = Color32::from_rgb(40, 40, 50);
/// Radius of corner handles (screen-space px)
const CORNER_HANDLE_RADIUS: f32 = 5.0;
/// Radius of edge midpoint handles (smaller per CapCut convention)
const EDGE_HANDLE_RADIUS: f32 = 4.0;

/// Color for the rotation handle connection line
const ROTATION_LINE_COLOR: Color32 = Color32::from_rgb(60, 140, 255);
/// Distance of rotation handle above top-center (screen px)
const ROTATION_HANDLE_OFFSET: f32 = 25.0;
/// Radius of rotation handle circle
const ROTATION_HANDLE_RADIUS: f32 = 6.0;

/// Color for the anchor point crosshair
const ANCHOR_COLOR: Color32 = Color32::from_rgb(255, 100, 100);
/// Radius of anchor crosshair circle
const ANCHOR_RADIUS: f32 = 5.0;
/// Length of anchor crosshair lines
const ANCHOR_CROSSHAIR_LENGTH: f32 = 8.0;

/// Smart guide line color (CapCut-style magenta)
const GUIDE_COLOR: Color32 = Color32::from_rgb(255, 0, 200);
/// Distance indicator text color
const DISTANCE_INDICATOR_TEXT: Color32 = Color32::from_rgb(255, 200, 0);

// ============================================================================
// SELECTED ELEMENT DATA — What docking.rs passes to the overlay
// ============================================================================

/// Data for a single selected element, gathered from ECS queries.
///
/// The caller (docking.rs / viewport integration) queries `Selected` +
/// `Transform2D` + optional source size and passes this struct.
#[derive(Debug, Clone)]
pub struct SelectedElementData {
    /// Bounding corners in scene coordinates (computed from Transform2D)
    pub corners: BoundingCorners,
    /// Anchor point in scene coordinates
    pub anchor_scene: glam::Vec2,
    /// Rotation angle in radians (for cursor orientation)
    pub rotation_radians: f32,
}

// ============================================================================
// GIZMO OVERLAY — Main rendering struct
// ============================================================================

/// Stateless gizmo overlay renderer.
///
/// Call `GizmoOverlay::draw()` each frame to render the gizmo on top of the
/// viewport. All data is passed in — no internal state.
pub struct GizmoOverlay;

impl GizmoOverlay {
    /// Draw the complete gizmo overlay for selected elements.
    ///
    /// ## Parameters
    ///
    /// - `painter`: The egui painter to draw onto (viewport layer)
    /// - `coord`: Coordinate transform for scene↔screen conversion
    /// - `selected`: Slice of selected element data (empty = nothing drawn)
    /// - `hovered_handle`: Currently hovered handle (for visual feedback)
    /// - `active_guides`: Smart guide lines to render (from snap engine)
    /// - `distance_indicators`: Distance labels to render
    pub fn draw(
        painter: &egui::Painter,
        coord: &CoordinateTransform,
        selected: &[SelectedElementData],
        hovered_handle: GizmoHandle,
        active_guides: &[GuideLine],
        distance_indicators: &[DistanceIndicator],
    ) {
        // Draw gizmo for each selected element
        for element in selected {
            Self::draw_bounding_box(painter, coord, element);
            Self::draw_scale_handles(painter, coord, element);
            Self::draw_rotation_handle(painter, coord, element);
            Self::draw_anchor_point(painter, coord, element);
        }

        // Highlight hovered handle (if any)
        if hovered_handle != GizmoHandle::None && !selected.is_empty() {
            Self::draw_hovered_handle_highlight(painter, coord, &selected[0], hovered_handle);
        }

        // Draw smart alignment guides
        for guide in active_guides {
            Self::draw_guide_line(painter, coord, guide);
        }

        // Draw distance indicators
        for indicator in distance_indicators {
            Self::draw_distance_indicator(painter, coord, indicator);
        }
    }

    /// Returns the egui cursor icon for the given hovered handle.
    ///
    /// Accounts for element rotation to orient resize cursors correctly.
    /// This is called by the input handler to set the cursor.
    pub fn cursor_for_handle(handle: GizmoHandle, rotation_radians: f32) -> egui::CursorIcon {
        match handle {
            GizmoHandle::None => egui::CursorIcon::Default,
            GizmoHandle::Body => egui::CursorIcon::Move,
            GizmoHandle::Rotate => egui::CursorIcon::Alias,
            GizmoHandle::Anchor => egui::CursorIcon::Crosshair,

            // Corner handles — diagonal resize, rotated
            GizmoHandle::TopLeft | GizmoHandle::BottomRight => {
                Self::diagonal_cursor_for_rotation(rotation_radians, false)
            }
            GizmoHandle::TopRight | GizmoHandle::BottomLeft => {
                Self::diagonal_cursor_for_rotation(rotation_radians, true)
            }

            // Edge handles — cardinal resize, rotated
            GizmoHandle::Top | GizmoHandle::Bottom => {
                Self::cardinal_cursor_for_rotation(rotation_radians, true)
            }
            GizmoHandle::Left | GizmoHandle::Right => {
                Self::cardinal_cursor_for_rotation(rotation_radians, false)
            }
        }
    }

    // ========================================================================
    // PRIVATE DRAWING METHODS
    // ========================================================================

    /// Draw bounding box around the element
    fn draw_bounding_box(
        painter: &egui::Painter,
        coord: &CoordinateTransform,
        element: &SelectedElementData,
    ) {
        let screen_corners: [Pos2; 4] = Self::corners_to_screen(coord, &element.corners);

        let stroke: Stroke = Stroke::new(BOUNDING_BOX_STROKE_WIDTH, BOUNDING_BOX_COLOR);
        for i in 0..4 {
            let from: Pos2 = screen_corners[i];
            let to: Pos2 = screen_corners[(i + 1) % 4];
            painter.line_segment([from, to], stroke);
        }
    }

    /// Draw 8 scale handles (4 corners + 4 edge midpoints)
    fn draw_scale_handles(
        painter: &egui::Painter,
        coord: &CoordinateTransform,
        element: &SelectedElementData,
    ) {
        let screen_corners: [Pos2; 4] = Self::corners_to_screen(coord, &element.corners);

        let stroke: Stroke = Stroke::new(1.5, HANDLE_STROKE_COLOR);

        // Corner handles (larger)
        for corner in &screen_corners {
            painter.circle_filled(*corner, CORNER_HANDLE_RADIUS, HANDLE_FILL_COLOR);
            painter.circle_stroke(*corner, CORNER_HANDLE_RADIUS, stroke);
        }

        // Edge midpoint handles (smaller)
        let midpoints: [Pos2; 4] = Self::edge_midpoints(&screen_corners);
        for midpoint in &midpoints {
            painter.circle_filled(*midpoint, EDGE_HANDLE_RADIUS, HANDLE_FILL_COLOR);
            painter.circle_stroke(*midpoint, EDGE_HANDLE_RADIUS, stroke);
        }
    }

    /// Draw rotation handle above top-center with connecting line
    fn draw_rotation_handle(
        painter: &egui::Painter,
        coord: &CoordinateTransform,
        element: &SelectedElementData,
    ) {
        let screen_corners: [Pos2; 4] = Self::corners_to_screen(coord, &element.corners);

        // Top-center midpoint
        let top_center: Pos2 = Pos2::new(
            (screen_corners[0].x + screen_corners[1].x) * 0.5,
            (screen_corners[0].y + screen_corners[1].y) * 0.5,
        );

        // Rotation handle: offset perpendicular to top edge (accounting for rotation)
        let rotation: f32 = element.rotation_radians;
        let offset_x: f32 = ROTATION_HANDLE_OFFSET * rotation.sin();
        let offset_y: f32 = -ROTATION_HANDLE_OFFSET * rotation.cos();
        let rotation_pos: Pos2 = Pos2::new(top_center.x - offset_x, top_center.y + offset_y);

        // Connecting line
        painter.line_segment(
            [top_center, rotation_pos],
            Stroke::new(1.0, ROTATION_LINE_COLOR),
        );

        // Rotation handle circle
        let stroke: Stroke = Stroke::new(1.5, HANDLE_STROKE_COLOR);
        painter.circle_filled(rotation_pos, ROTATION_HANDLE_RADIUS, BOUNDING_BOX_COLOR);
        painter.circle_stroke(rotation_pos, ROTATION_HANDLE_RADIUS, stroke);
    }

    /// Draw anchor point crosshair
    fn draw_anchor_point(
        painter: &egui::Painter,
        coord: &CoordinateTransform,
        element: &SelectedElementData,
    ) {
        let anchor_screen: Pos2 = Self::scene_to_screen_pos(coord, element.anchor_scene);

        let half_len: f32 = ANCHOR_CROSSHAIR_LENGTH;
        let stroke: Stroke = Stroke::new(1.5, ANCHOR_COLOR);

        painter.line_segment(
            [
                Pos2::new(anchor_screen.x - half_len, anchor_screen.y),
                Pos2::new(anchor_screen.x + half_len, anchor_screen.y),
            ],
            stroke,
        );
        painter.line_segment(
            [
                Pos2::new(anchor_screen.x, anchor_screen.y - half_len),
                Pos2::new(anchor_screen.x, anchor_screen.y + half_len),
            ],
            stroke,
        );

        painter.circle_stroke(anchor_screen, ANCHOR_RADIUS, stroke);
    }

    /// Highlight the currently hovered handle with a larger stroke
    fn draw_hovered_handle_highlight(
        painter: &egui::Painter,
        coord: &CoordinateTransform,
        element: &SelectedElementData,
        handle: GizmoHandle,
    ) {
        let screen_corners: [Pos2; 4] = Self::corners_to_screen(coord, &element.corners);
        let highlight_stroke: Stroke = Stroke::new(2.0, BOUNDING_BOX_COLOR);

        let position: Option<Pos2> = match handle {
            GizmoHandle::TopLeft => Some(screen_corners[0]),
            GizmoHandle::TopRight => Some(screen_corners[1]),
            GizmoHandle::BottomRight => Some(screen_corners[2]),
            GizmoHandle::BottomLeft => Some(screen_corners[3]),
            GizmoHandle::Top => {
                let midpoints: [Pos2; 4] = Self::edge_midpoints(&screen_corners);
                Some(midpoints[0])
            }
            GizmoHandle::Right => {
                let midpoints: [Pos2; 4] = Self::edge_midpoints(&screen_corners);
                Some(midpoints[1])
            }
            GizmoHandle::Bottom => {
                let midpoints: [Pos2; 4] = Self::edge_midpoints(&screen_corners);
                Some(midpoints[2])
            }
            GizmoHandle::Left => {
                let midpoints: [Pos2; 4] = Self::edge_midpoints(&screen_corners);
                Some(midpoints[3])
            }
            GizmoHandle::Anchor => Some(Self::scene_to_screen_pos(coord, element.anchor_scene)),
            GizmoHandle::Rotate => {
                let top_center: Pos2 = Pos2::new(
                    (screen_corners[0].x + screen_corners[1].x) * 0.5,
                    (screen_corners[0].y + screen_corners[1].y) * 0.5,
                );
                let rotation: f32 = element.rotation_radians;
                let offset_x: f32 = ROTATION_HANDLE_OFFSET * rotation.sin();
                let offset_y: f32 = -ROTATION_HANDLE_OFFSET * rotation.cos();
                Some(Pos2::new(top_center.x - offset_x, top_center.y + offset_y))
            }
            _ => None,
        };

        if let Some(pos) = position {
            let radius: f32 = match handle {
                GizmoHandle::TopLeft
                | GizmoHandle::TopRight
                | GizmoHandle::BottomRight
                | GizmoHandle::BottomLeft => CORNER_HANDLE_RADIUS + 2.0,
                GizmoHandle::Rotate => ROTATION_HANDLE_RADIUS + 2.0,
                GizmoHandle::Anchor => ANCHOR_RADIUS + 2.0,
                _ => EDGE_HANDLE_RADIUS + 2.0,
            };
            painter.circle_stroke(pos, radius, highlight_stroke);
        }
    }

    /// Draw a smart guide line across the viewport
    fn draw_guide_line(painter: &egui::Painter, coord: &CoordinateTransform, guide: &GuideLine) {
        let start: Pos2 = Self::scene_to_screen_pos(coord, guide.start);
        let end: Pos2 = Self::scene_to_screen_pos(coord, guide.end);

        let color: Color32 = match guide.kind {
            GuideKind::CenterAlignment => GUIDE_COLOR,
            GuideKind::EdgeAlignment => Color32::from_rgb(255, 80, 200),
            GuideKind::EqualSpacing => Color32::from_rgb(100, 255, 200),
        };

        Self::draw_dashed_line(painter, start, end, Stroke::new(1.0, color), 6.0, 4.0);
    }

    /// Draw a distance indicator label
    fn draw_distance_indicator(
        painter: &egui::Painter,
        coord: &CoordinateTransform,
        indicator: &DistanceIndicator,
    ) {
        let start: Pos2 = Self::scene_to_screen_pos(coord, indicator.from);
        let end: Pos2 = Self::scene_to_screen_pos(coord, indicator.to);
        let center: Pos2 = Pos2::new((start.x + end.x) * 0.5, (start.y + end.y) * 0.5);

        // Draw connecting line
        painter.line_segment([start, end], Stroke::new(1.0, DISTANCE_INDICATOR_TEXT));

        // Draw label background + text
        let label: String = format!("{:.0}px", indicator.distance);
        let font: egui::FontId = egui::FontId::proportional(10.0);
        let galley = painter.layout_no_wrap(label, font, DISTANCE_INDICATOR_TEXT);

        let bg_color: Color32 = Color32::from_rgba_unmultiplied(0, 0, 0, 180);
        let label_rect: Rect = Rect::from_center_size(center, galley.size() + Vec2::new(8.0, 4.0));

        painter.rect_filled(label_rect, 3.0, bg_color);
        painter.galley(
            Pos2::new(label_rect.min.x + 4.0, label_rect.min.y + 2.0),
            galley,
            DISTANCE_INDICATOR_TEXT,
        );
    }

    // ========================================================================
    // HELPER METHODS
    // ========================================================================

    /// Convert BoundingCorners to screen positions [TL, TR, BR, BL]
    fn corners_to_screen(coord: &CoordinateTransform, corners: &BoundingCorners) -> [Pos2; 4] {
        [
            Self::scene_to_screen_pos(coord, corners.top_left),
            Self::scene_to_screen_pos(coord, corners.top_right),
            Self::scene_to_screen_pos(coord, corners.bottom_right),
            Self::scene_to_screen_pos(coord, corners.bottom_left),
        ]
    }

    /// Calculate edge midpoints from 4 screen-space corners
    fn edge_midpoints(corners: &[Pos2; 4]) -> [Pos2; 4] {
        [
            Pos2::new(
                (corners[0].x + corners[1].x) * 0.5,
                (corners[0].y + corners[1].y) * 0.5,
            ),
            Pos2::new(
                (corners[1].x + corners[2].x) * 0.5,
                (corners[1].y + corners[2].y) * 0.5,
            ),
            Pos2::new(
                (corners[2].x + corners[3].x) * 0.5,
                (corners[2].y + corners[3].y) * 0.5,
            ),
            Pos2::new(
                (corners[3].x + corners[0].x) * 0.5,
                (corners[3].y + corners[0].y) * 0.5,
            ),
        ]
    }

    /// Convert a scene-space Vec2 to a screen-space Pos2
    fn scene_to_screen_pos(coord: &CoordinateTransform, scene_pos: glam::Vec2) -> Pos2 {
        let screen: glam::Vec2 = coord.scene_to_screen(scene_pos);
        Pos2::new(screen.x, screen.y)
    }

    /// Draw a dashed line segment
    fn draw_dashed_line(
        painter: &egui::Painter,
        from: Pos2,
        to: Pos2,
        stroke: Stroke,
        dash_length: f32,
        gap_length: f32,
    ) {
        let delta: Vec2 = to - from;
        let total_length: f32 = delta.length();
        if total_length < 0.001 {
            return;
        }

        let dir: Vec2 = delta / total_length;
        let segment_length: f32 = dash_length + gap_length;
        let mut distance: f32 = 0.0;

        while distance < total_length {
            let dash_start: Pos2 = from + dir * distance;
            let dash_end_distance: f32 = (distance + dash_length).min(total_length);
            let dash_end: Pos2 = from + dir * dash_end_distance;

            painter.line_segment([dash_start, dash_end], stroke);
            distance += segment_length;
        }
    }

    /// Choose diagonal resize cursor based on element rotation.
    fn diagonal_cursor_for_rotation(rotation: f32, is_anti_diagonal: bool) -> egui::CursorIcon {
        let base_angle: f32 = if is_anti_diagonal {
            std::f32::consts::FRAC_PI_4
        } else {
            0.0
        };

        let effective_angle: f32 = (rotation + base_angle).rem_euclid(std::f32::consts::PI);
        let sector: i32 = (effective_angle / (std::f32::consts::PI / 4.0) + 0.5) as i32;

        match sector % 4 {
            0 => egui::CursorIcon::ResizeNwSe,
            1 => egui::CursorIcon::ResizeVertical,
            2 => egui::CursorIcon::ResizeNeSw,
            3 => egui::CursorIcon::ResizeHorizontal,
            _ => egui::CursorIcon::ResizeNwSe,
        }
    }

    /// Choose cardinal resize cursor based on element rotation.
    fn cardinal_cursor_for_rotation(rotation: f32, is_vertical: bool) -> egui::CursorIcon {
        let base_angle: f32 = if is_vertical {
            std::f32::consts::FRAC_PI_2
        } else {
            0.0
        };

        let effective_angle: f32 = (rotation + base_angle).rem_euclid(std::f32::consts::PI);
        let sector: i32 = (effective_angle / (std::f32::consts::FRAC_PI_4) + 0.5) as i32;

        match sector % 4 {
            0 => egui::CursorIcon::ResizeHorizontal,
            1 => egui::CursorIcon::ResizeNwSe,
            2 => egui::CursorIcon::ResizeVertical,
            3 => egui::CursorIcon::ResizeNeSw,
            _ => egui::CursorIcon::ResizeHorizontal,
        }
    }
}
