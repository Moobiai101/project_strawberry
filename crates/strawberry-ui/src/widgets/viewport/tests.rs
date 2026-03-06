//! Tests for the Viewport widget module

use super::*;

#[test]
fn test_viewport_creation() {
    let viewport = ViewportWidget::new()
        .with_resolution(1920, 1080)
        .with_zoom(1.5);

    assert_eq!(viewport.source_resolution.x, 1920);
    assert_eq!(viewport.zoom, 1.5);
}

#[test]
fn test_aspect_ratio_calculation() {
    let viewport = ViewportWidget::new().with_resolution(1920, 1080);

    // Test with wider container
    let wide_container = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 300.0));
    let display: Rect = viewport.calculate_display_rect(wide_container);

    // Should fit to height (container is wider than 16:9)
    assert!((display.height() - 300.0).abs() < 0.1);

    // Test with taller container
    let tall_container = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 600.0));
    let display_tall: Rect = ViewportWidget::new()
        .with_resolution(1920, 1080)
        .calculate_display_rect(tall_container);

    // Should fit to width (container is taller than 16:9)
    assert!((display_tall.width() - 400.0).abs() < 0.1);
}
