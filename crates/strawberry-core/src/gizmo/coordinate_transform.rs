//! # Coordinate Transform
//!
//! Screen ↔ Scene coordinate conversion for viewport gizmo interactions.
//!
//! ## Coordinate Spaces
//!
//! - **Screen space**: Pixel coordinates within the egui viewport widget
//!   (origin at widget top-left, Y increases downward)
//! - **Scene space**: Coordinates in the project canvas (e.g., 1920×1080)
//!   (origin at canvas top-left, Y increases downward)
//!
//! ## Transform Pipeline (mirrors Olive's `GenerateGizmoTransform`)
//!
//! ```text
//! Screen → translate(-viewport_offset) → scale(1/zoom) → translate(-pan) → Scene
//! Scene  → translate(pan) → scale(zoom) → translate(viewport_offset) → Screen
//! ```
//!
//! The viewport may letterbox/pillarbox the canvas to maintain aspect ratio.
//! The `display_rect` (computed by the viewport widget) is the on-screen rectangle
//! where the canvas is actually drawn — accounting for aspect ratio fitting.

use glam::Vec2;

/// Parameters describing how the scene is displayed in the viewport.
///
/// This struct captures the full transform chain from screen pixels to
/// scene coordinates. It is computed once per frame by the viewport widget
/// and passed into the gizmo engine.
///
/// ## Example
///
/// ```ignore
/// let coord: CoordinateTransform = CoordinateTransform::new(
///     Vec2::new(0.0, 0.0),     // display_rect origin (top-left in screen px)
///     Vec2::new(800.0, 450.0), // display_rect size (screen px)
///     Vec2::new(1920.0, 1080.0), // canvas size (scene units)
///     1.0,                     // zoom level
///     Vec2::ZERO,              // pan offset (scene units)
/// );
/// let scene_pos: Vec2 = coord.screen_to_scene(Vec2::new(400.0, 225.0));
/// // → (960.0, 540.0) at 1x zoom, no pan
/// ```
#[derive(Debug, Clone)]
pub struct CoordinateTransform {
    /// Top-left corner of the display rect in screen pixels.
    /// This is where the canvas starts drawing (after letterbox padding).
    pub display_origin: Vec2,

    /// Size of the display rect in screen pixels.
    /// This is the actual rendered area of the canvas on screen.
    pub display_size: Vec2,

    /// Canvas size in scene units (e.g., 1920×1080).
    pub canvas_size: Vec2,

    /// Zoom level (1.0 = fit to viewport, >1.0 = zoomed in).
    pub zoom: f32,

    /// Pan offset in scene coordinates (positive = canvas shifted right/down).
    pub pan: Vec2,
}

impl CoordinateTransform {
    /// Create a new coordinate transform with the given parameters.
    ///
    /// # Arguments
    ///
    /// * `display_origin` - Top-left of the canvas display area in screen pixels
    /// * `display_size` - Size of the canvas display area in screen pixels
    /// * `canvas_size` - Canvas resolution in scene units (e.g., 1920×1080)
    /// * `zoom` - Zoom level (1.0 = fit to viewport)
    /// * `pan` - Pan offset in scene coordinates
    pub fn new(
        display_origin: Vec2,
        display_size: Vec2,
        canvas_size: Vec2,
        zoom: f32,
        pan: Vec2,
    ) -> Self {
        Self {
            display_origin,
            display_size,
            canvas_size,
            zoom: zoom.max(f32::EPSILON), // Prevent division by zero
            pan,
        }
    }

    /// Convert a screen-space point to scene-space coordinates.
    ///
    /// This is the core inverse transform used for hit-testing:
    /// when the user clicks at screen pixel (x, y), this returns the
    /// corresponding position on the project canvas.
    ///
    /// Mirrors Olive's `ScreenToScenePoint` at viewerdisplay.cpp:L1288-1295,
    /// but uses our own display-rect-based coordinate system instead of
    /// Olive's QTransform matrix approach.
    pub fn screen_to_scene(&self, screen_pos: Vec2) -> Vec2 {
        // Step 1: Convert screen position to normalized display position (0..1)
        let relative_to_display: Vec2 = screen_pos - self.display_origin;
        let normalized: Vec2 = relative_to_display / self.display_size;

        // Step 2: Convert normalized to canvas coordinates
        let canvas_pos: Vec2 = normalized * self.canvas_size;

        // Step 3: Apply inverse zoom and pan
        let zoomed_center: Vec2 = self.canvas_size * 0.5;
        let from_center: Vec2 = canvas_pos - zoomed_center;
        let unzoomed: Vec2 = from_center / self.zoom + zoomed_center;

        // Step 4: Apply inverse pan
        unzoomed - self.pan
    }

    /// Convert a scene-space point to screen-space coordinates.
    ///
    /// Used for rendering gizmo handles on screen at the correct position.
    pub fn scene_to_screen(&self, scene_pos: Vec2) -> Vec2 {
        // Step 1: Apply pan
        let panned: Vec2 = scene_pos + self.pan;

        // Step 2: Apply zoom around canvas center
        let zoomed_center: Vec2 = self.canvas_size * 0.5;
        let from_center: Vec2 = panned - zoomed_center;
        let zoomed: Vec2 = from_center * self.zoom + zoomed_center;

        // Step 3: Normalize to 0..1
        let normalized: Vec2 = zoomed / self.canvas_size;

        // Step 4: Convert to screen pixels
        let screen_pos: Vec2 = normalized * self.display_size + self.display_origin;
        screen_pos
    }

    /// Convert a screen-space delta (movement) to scene-space delta.
    ///
    /// Unlike `screen_to_scene`, this only converts the magnitude/direction
    /// of a movement — no origin translation. Used for drag deltas.
    pub fn screen_delta_to_scene_delta(&self, screen_delta: Vec2) -> Vec2 {
        // Delta doesn't need origin or pan adjustment, only scale
        let scale_factor: f32 = self.scene_pixels_per_screen_pixel();
        screen_delta * scale_factor / self.zoom
    }

    /// Convert a scene-space delta to screen-space delta.
    ///
    /// Used for converting scene-unit movements to screen pixel movements.
    pub fn scene_delta_to_screen_delta(&self, scene_delta: Vec2) -> Vec2 {
        let scale_factor: f32 = self.screen_pixels_per_scene_pixel();
        scene_delta * scale_factor * self.zoom
    }

    /// How many scene units correspond to one screen pixel.
    ///
    /// Used for determining handle sizes that should be constant
    /// in screen space regardless of zoom.
    pub fn scene_pixels_per_screen_pixel(&self) -> f32 {
        if self.display_size.x.abs() < f32::EPSILON {
            return 1.0;
        }
        self.canvas_size.x / self.display_size.x
    }

    /// How many screen pixels correspond to one scene unit.
    pub fn screen_pixels_per_scene_pixel(&self) -> f32 {
        if self.canvas_size.x.abs() < f32::EPSILON {
            return 1.0;
        }
        self.display_size.x / self.canvas_size.x
    }

    /// Convert a screen-space distance to a scene-space distance.
    ///
    /// Used for handle hit radii: e.g., "8 screen pixels" → scene units.
    /// Accounts for current zoom level.
    pub fn screen_distance_to_scene_distance(&self, screen_pixels: f32) -> f32 {
        screen_pixels * self.scene_pixels_per_screen_pixel() / self.zoom
    }

    /// Check whether a screen-space point falls within the display rect.
    pub fn is_point_in_display_rect(&self, screen_pos: Vec2) -> bool {
        let relative: Vec2 = screen_pos - self.display_origin;
        relative.x >= 0.0
            && relative.y >= 0.0
            && relative.x <= self.display_size.x
            && relative.y <= self.display_size.y
    }

    /// Build the full scene→screen affine matrix (mirrors Olive's GenerateDisplayTransform).
    ///
    /// Composes four steps into a single `Mat3`:
    ///
    /// 1. `translate(pan)` — apply pan offset
    /// 2. `zoom around canvas center` — translate(-center) → scale(zoom) → translate(center)
    /// 3. `normalize` — scale(1/canvas_w, 1/canvas_h) to get 0..1
    /// 4. `to screen` — scale(display_w, display_h) + translate(display_origin)
    ///
    /// The resulting Mat3 transforms any scene-space point to screen pixels in one
    /// matrix multiply — matching Olive's `GenerateDisplayTransform()`.
    ///
    /// This is cached once per frame and used by `render_clip_quad()` for vertex
    /// transforms (instead of per-point `scene_to_screen()` calls) and by gizmo
    /// hit testing for coordinate conversion.
    pub fn scene_to_screen_matrix(&self) -> glam::Mat3 {
        use glam::Mat3;

        let center: Vec2 = self.canvas_size * 0.5;

        // Step 1: Apply pan
        let pan_matrix: Mat3 = Mat3::from_translation(self.pan);

        // Step 2: Zoom around canvas center
        //   translate(-center) → scale(zoom) → translate(center)
        let to_origin: Mat3 = Mat3::from_translation(-center);
        let zoom_scale: Mat3 = Mat3::from_scale(Vec2::splat(self.zoom));
        let from_origin: Mat3 = Mat3::from_translation(center);
        let zoom_matrix: Mat3 = from_origin * zoom_scale * to_origin;

        // Step 3: Normalize to 0..1 range
        let normalize: Mat3 = Mat3::from_scale(Vec2::new(
            1.0 / self.canvas_size.x.max(f32::EPSILON),
            1.0 / self.canvas_size.y.max(f32::EPSILON),
        ));

        // Step 4: Scale to screen pixels + translate to display origin
        let to_screen: Mat3 =
            Mat3::from_translation(self.display_origin) * Mat3::from_scale(self.display_size);

        // Compose: read right-to-left (pan first, screen last)
        to_screen * normalize * zoom_matrix * pan_matrix
    }

    /// Inverse: screen→scene matrix (mirrors Olive's `gizmo_last_draw_transform_inverted_`).
    ///
    /// Transforms screen pixel coordinates back to scene-space coordinates.
    /// Used for hit-testing when the user clicks at a screen position and we need
    /// the corresponding scene-space point.
    pub fn screen_to_scene_matrix(&self) -> glam::Mat3 {
        self.scene_to_screen_matrix().inverse()
    }

    /// Return the display rect as an `(origin, size)` tuple.
    ///
    /// Convenience accessor used by callers that need to pass the display rect
    /// as a clipping boundary (e.g., `render_clip_quad()`).
    pub fn display_rect(&self) -> (Vec2, Vec2) {
        (self.display_origin, self.display_size)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a standard 1920×1080 canvas displayed in an 800×450 viewport
    /// at 1x zoom with no pan. This gives a perfect 1:1 aspect ratio fit.
    fn standard_transform() -> CoordinateTransform {
        CoordinateTransform::new(
            Vec2::new(0.0, 0.0),       // display origin
            Vec2::new(800.0, 450.0),   // display size (matches 16:9)
            Vec2::new(1920.0, 1080.0), // canvas size
            1.0,                       // zoom
            Vec2::ZERO,                // pan
        )
    }

    #[test]
    fn test_screen_to_scene_identity() {
        let ct: CoordinateTransform = standard_transform();

        // Center of display → center of canvas
        let center_screen: Vec2 = Vec2::new(400.0, 225.0);
        let center_scene: Vec2 = ct.screen_to_scene(center_screen);
        assert!((center_scene.x - 960.0).abs() < 0.1, "x={}", center_scene.x);
        assert!((center_scene.y - 540.0).abs() < 0.1, "y={}", center_scene.y);
    }

    #[test]
    fn test_screen_to_scene_origin() {
        let ct: CoordinateTransform = standard_transform();

        // Top-left of display → top-left of canvas
        let origin_scene: Vec2 = ct.screen_to_scene(Vec2::ZERO);
        assert!((origin_scene.x).abs() < 0.1, "x={}", origin_scene.x);
        assert!((origin_scene.y).abs() < 0.1, "y={}", origin_scene.y);
    }

    #[test]
    fn test_screen_to_scene_bottom_right() {
        let ct: CoordinateTransform = standard_transform();

        // Bottom-right of display → bottom-right of canvas
        let br_scene: Vec2 = ct.screen_to_scene(Vec2::new(800.0, 450.0));
        assert!((br_scene.x - 1920.0).abs() < 0.1, "x={}", br_scene.x);
        assert!((br_scene.y - 1080.0).abs() < 0.1, "y={}", br_scene.y);
    }

    #[test]
    fn test_screen_to_scene_zoomed() {
        // 2x zoom → screen center still maps to canvas center,
        // but edges map to half the canvas range
        let ct: CoordinateTransform = CoordinateTransform::new(
            Vec2::ZERO,
            Vec2::new(800.0, 450.0),
            Vec2::new(1920.0, 1080.0),
            2.0,
            Vec2::ZERO,
        );

        // Center should still be center
        let center: Vec2 = ct.screen_to_scene(Vec2::new(400.0, 225.0));
        assert!((center.x - 960.0).abs() < 0.1, "x={}", center.x);
        assert!((center.y - 540.0).abs() < 0.1, "y={}", center.y);

        // Top-left at 2x zoom → scene (480, 270) (quarter from center)
        let tl: Vec2 = ct.screen_to_scene(Vec2::ZERO);
        assert!((tl.x - 480.0).abs() < 0.5, "tl.x={}", tl.x);
        assert!((tl.y - 270.0).abs() < 0.5, "tl.y={}", tl.y);
    }

    #[test]
    fn test_screen_to_scene_panned() {
        // Pan right by 100 scene units → everything shifts left in scene space
        let ct: CoordinateTransform = CoordinateTransform::new(
            Vec2::ZERO,
            Vec2::new(800.0, 450.0),
            Vec2::new(1920.0, 1080.0),
            1.0,
            Vec2::new(100.0, 50.0),
        );

        let center: Vec2 = ct.screen_to_scene(Vec2::new(400.0, 225.0));
        assert!((center.x - 860.0).abs() < 0.5, "x={}", center.x);
        assert!((center.y - 490.0).abs() < 0.5, "y={}", center.y);
    }

    #[test]
    fn test_roundtrip_screen_scene_screen() {
        let ct: CoordinateTransform = CoordinateTransform::new(
            Vec2::new(50.0, 30.0), // offset display
            Vec2::new(800.0, 450.0),
            Vec2::new(1920.0, 1080.0),
            1.5,                     // zoomed in
            Vec2::new(200.0, 100.0), // panned
        );

        let original_screen: Vec2 = Vec2::new(350.0, 200.0);
        let scene: Vec2 = ct.screen_to_scene(original_screen);
        let back_to_screen: Vec2 = ct.scene_to_screen(scene);

        assert!(
            (original_screen.x - back_to_screen.x).abs() < 0.01,
            "x: {} vs {}",
            original_screen.x,
            back_to_screen.x
        );
        assert!(
            (original_screen.y - back_to_screen.y).abs() < 0.01,
            "y: {} vs {}",
            original_screen.y,
            back_to_screen.y
        );
    }

    #[test]
    fn test_roundtrip_scene_screen_scene() {
        let ct: CoordinateTransform = CoordinateTransform::new(
            Vec2::new(10.0, 20.0),
            Vec2::new(640.0, 360.0),
            Vec2::new(3840.0, 2160.0), // 4K canvas
            0.75,
            Vec2::new(-50.0, 30.0),
        );

        let original_scene: Vec2 = Vec2::new(1000.0, 500.0);
        let screen: Vec2 = ct.scene_to_screen(original_scene);
        let back_to_scene: Vec2 = ct.screen_to_scene(screen);

        assert!(
            (original_scene.x - back_to_scene.x).abs() < 0.1,
            "x: {} vs {}",
            original_scene.x,
            back_to_scene.x
        );
        assert!(
            (original_scene.y - back_to_scene.y).abs() < 0.1,
            "y: {} vs {}",
            original_scene.y,
            back_to_scene.y
        );
    }

    #[test]
    fn test_screen_delta_to_scene_delta() {
        let ct: CoordinateTransform = standard_transform();

        // At 1x zoom, 800 screen px = 1920 scene units → ratio = 2.4
        let delta: Vec2 = ct.screen_delta_to_scene_delta(Vec2::new(10.0, 10.0));
        assert!((delta.x - 24.0).abs() < 0.1, "dx={}", delta.x);
        assert!((delta.y - 24.0).abs() < 0.1, "dy={}", delta.y);
    }

    #[test]
    fn test_screen_distance_to_scene_distance() {
        let ct: CoordinateTransform = standard_transform();

        // 8 screen pixels should convert to scene units
        let scene_dist: f32 = ct.screen_distance_to_scene_distance(8.0);
        let expected: f32 = 8.0 * (1920.0 / 800.0);
        assert!((scene_dist - expected).abs() < 0.1, "dist={}", scene_dist);
    }

    #[test]
    fn test_display_offset() {
        // Display rect starts at (100, 50) — simulates letterboxing
        let ct: CoordinateTransform = CoordinateTransform::new(
            Vec2::new(100.0, 50.0),
            Vec2::new(800.0, 450.0),
            Vec2::new(1920.0, 1080.0),
            1.0,
            Vec2::ZERO,
        );

        // Screen (100, 50) → scene (0, 0) (display top-left = canvas top-left)
        let scene: Vec2 = ct.screen_to_scene(Vec2::new(100.0, 50.0));
        assert!((scene.x).abs() < 0.1, "x={}", scene.x);
        assert!((scene.y).abs() < 0.1, "y={}", scene.y);
    }

    #[test]
    fn test_is_point_in_display_rect() {
        let ct: CoordinateTransform = CoordinateTransform::new(
            Vec2::new(100.0, 50.0),
            Vec2::new(800.0, 450.0),
            Vec2::new(1920.0, 1080.0),
            1.0,
            Vec2::ZERO,
        );

        assert!(ct.is_point_in_display_rect(Vec2::new(100.0, 50.0))); // top-left
        assert!(ct.is_point_in_display_rect(Vec2::new(500.0, 275.0))); // center
        assert!(ct.is_point_in_display_rect(Vec2::new(900.0, 500.0))); // bottom-right
        assert!(!ct.is_point_in_display_rect(Vec2::new(99.0, 50.0))); // just outside left
        assert!(!ct.is_point_in_display_rect(Vec2::new(500.0, 49.0))); // just outside top
    }

    /// Verify that `scene_to_screen_matrix()` produces the same result as the
    /// per-point `scene_to_screen()` method across various configurations.
    #[test]
    fn test_matrix_matches_per_point_scene_to_screen() {
        let configs: Vec<CoordinateTransform> = vec![
            standard_transform(),
            CoordinateTransform::new(
                Vec2::new(50.0, 30.0),
                Vec2::new(800.0, 450.0),
                Vec2::new(1920.0, 1080.0),
                1.5,
                Vec2::new(200.0, 100.0),
            ),
            CoordinateTransform::new(
                Vec2::new(100.0, 50.0),
                Vec2::new(640.0, 360.0),
                Vec2::new(3840.0, 2160.0),
                0.75,
                Vec2::new(-50.0, 30.0),
            ),
        ];

        let test_points: Vec<Vec2> = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(960.0, 540.0),
            Vec2::new(1920.0, 1080.0),
            Vec2::new(500.0, 300.0),
        ];

        for ct in &configs {
            let mat: glam::Mat3 = ct.scene_to_screen_matrix();
            for &scene_pos in &test_points {
                let expected: Vec2 = ct.scene_to_screen(scene_pos);
                let from_matrix: Vec2 = mat.transform_point2(scene_pos);
                assert!(
                    (expected.x - from_matrix.x).abs() < 0.1,
                    "scene_to_screen mismatch x: {} vs {} for {:?}",
                    expected.x,
                    from_matrix.x,
                    scene_pos
                );
                assert!(
                    (expected.y - from_matrix.y).abs() < 0.1,
                    "scene_to_screen mismatch y: {} vs {} for {:?}",
                    expected.y,
                    from_matrix.y,
                    scene_pos
                );
            }
        }
    }

    /// Verify that `screen_to_scene_matrix()` produces the same result as the
    /// per-point `screen_to_scene()` method.
    #[test]
    fn test_matrix_matches_per_point_screen_to_scene() {
        let ct: CoordinateTransform = CoordinateTransform::new(
            Vec2::new(50.0, 30.0),
            Vec2::new(800.0, 450.0),
            Vec2::new(1920.0, 1080.0),
            1.5,
            Vec2::new(200.0, 100.0),
        );

        let test_points: Vec<Vec2> = vec![
            Vec2::new(50.0, 30.0),
            Vec2::new(450.0, 255.0),
            Vec2::new(850.0, 480.0),
        ];

        let inv_mat: glam::Mat3 = ct.screen_to_scene_matrix();
        for &screen_pos in &test_points {
            let expected: Vec2 = ct.screen_to_scene(screen_pos);
            let from_matrix: Vec2 = inv_mat.transform_point2(screen_pos);
            assert!(
                (expected.x - from_matrix.x).abs() < 0.5,
                "screen_to_scene mismatch x: {} vs {} for {:?}",
                expected.x,
                from_matrix.x,
                screen_pos
            );
            assert!(
                (expected.y - from_matrix.y).abs() < 0.5,
                "screen_to_scene mismatch y: {} vs {} for {:?}",
                expected.y,
                from_matrix.y,
                screen_pos
            );
        }
    }

    /// Verify matrix roundtrip: scene → screen → scene via matrices.
    #[test]
    fn test_matrix_roundtrip() {
        let ct: CoordinateTransform = CoordinateTransform::new(
            Vec2::new(50.0, 30.0),
            Vec2::new(800.0, 450.0),
            Vec2::new(1920.0, 1080.0),
            2.0,
            Vec2::new(100.0, -50.0),
        );

        let scene_pos: Vec2 = Vec2::new(800.0, 400.0);
        let fwd: glam::Mat3 = ct.scene_to_screen_matrix();
        let inv: glam::Mat3 = ct.screen_to_scene_matrix();

        let screen: Vec2 = fwd.transform_point2(scene_pos);
        let back: Vec2 = inv.transform_point2(screen);

        assert!(
            (scene_pos.x - back.x).abs() < 0.01,
            "roundtrip x: {} vs {}",
            scene_pos.x,
            back.x
        );
        assert!(
            (scene_pos.y - back.y).abs() < 0.01,
            "roundtrip y: {} vs {}",
            scene_pos.y,
            back.y
        );
    }
}
