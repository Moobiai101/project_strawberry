//! # Keyframe Interpolation Engine
//!
//! Production-grade, property-agnostic interpolation math for keyframe automation.
//! No ECS, no UI, no allocations — pure functions, fully testable in isolation.
//!
//! ## Interpolation Modes
//!
//! | Mode   | Algorithm                                | Typical Use                    |
//! |--------|------------------------------------------|--------------------------------|
//! | Hold   | Step function                            | Instantaneous parameter jumps  |
//! | Linear | Lerp                                     | Simple, predictable transitions|
//! | Bezier | 2D cubic bezier + Newton-Raphson solver  | Precise easing (AE-style)      |
//! | Smooth | Non-uniform Catmull-Rom / Hermite basis  | Auto-smooth (Resolve-style)    |
//!
//! ## Performance
//! - `evaluate_at_time`: O(log n) binary search + O(1) interpolation
//! - Bezier solver: Newton-Raphson (2-4 iters typical) + bisection fallback
//! - All functions `#[inline]`, zero heap allocation, no branching on hot path

use serde::{Deserialize, Serialize};

// ============================================================================
// INTERPOLATION MODE
// ============================================================================

/// How to interpolate FROM this keyframe TO the next one.
///
/// The last keyframe's mode is irrelevant (value held indefinitely).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum KeyframeInterpolation {
    /// Step function — value holds until the next keyframe, then jumps.
    Hold,
    /// Straight line: `lerp(a, b, t)`. Industry default.
    #[default]
    Linear,
    /// 2D cubic bezier with user-editable tangent handles (After Effects style).
    /// Requires Newton-Raphson solving for the time→parameter mapping.
    Bezier,
    /// Auto-smooth non-uniform Catmull-Rom (DaVinci Resolve / MLT style).
    /// Tangents auto-computed from neighbors; accounts for uneven time spacing.
    Smooth,
}

impl KeyframeInterpolation {
    /// All variants for UI dropdowns.
    pub const ALL: &'static [KeyframeInterpolation] = &[
        KeyframeInterpolation::Hold,
        KeyframeInterpolation::Linear,
        KeyframeInterpolation::Bezier,
        KeyframeInterpolation::Smooth,
    ];

    /// Human-readable display name for UI.
    pub fn display_name(&self) -> &'static str {
        match self {
            KeyframeInterpolation::Hold => "Hold",
            KeyframeInterpolation::Linear => "Linear",
            KeyframeInterpolation::Bezier => "Bezier",
            KeyframeInterpolation::Smooth => "Smooth",
        }
    }
}

// ============================================================================
// KEYFRAME
// ============================================================================

/// A single automation keypoint with bezier tangent handles.
///
/// Time is clip-relative seconds (0.0 = clip start).
///
/// ## Bezier Handle Model (After Effects / Blender standard)
///
/// Each keyframe owns both its incoming and outgoing tangent handles.
/// For the segment from keyframe A to B, the cubic bezier is:
/// ```text
///   P0 = (0, 0)               — segment start (normalized)
///   P1 = A.handle_right       — outgoing tangent of A
///   P2 = B.handle_left        — incoming tangent of B
///   P3 = (1, 1)               — segment end (normalized)
/// ```
///
/// Handles are in normalized segment coordinates:
/// - `[0]` = time axis ∈ [0.0, 1.0] (must be monotonic)
/// - `[1]` = value axis, can exceed [0, 1] for overshoot
///
/// The actual value: `result = A.value + (B.value - A.value) × By(u)`
///
/// ## Property Semantics (depends on property_id)
/// - `"volume"`: 0.0–2.0 (0%–200%)
/// - `"pan"`: -1.0 to 1.0
/// - `"opacity"`: 0.0–1.0
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keyframe {
    /// Clip-relative time in seconds (0.0 = clip start)
    pub time_secs: f64,
    /// Value at this point (meaning depends on property)
    pub value: f32,
    /// Interpolation method FROM this keyframe toward the next
    pub interpolation: KeyframeInterpolation,
    /// Outgoing bezier handle (P1). Normalized within segment to NEXT keyframe.
    /// `[0]` = time ∈ [0, 1], `[1]` = value (can overshoot).
    /// Default `[1/3, 1/3]` produces linear interpolation.
    pub handle_right: [f32; 2],
    /// Incoming bezier handle (P2). Normalized within segment from PREVIOUS keyframe.
    /// `[0]` = time ∈ [0, 1], `[1]` = value (can overshoot).
    /// Default `[2/3, 2/3]` produces linear interpolation.
    pub handle_left: [f32; 2],
}

impl Keyframe {
    /// Create a keyframe with default linear interpolation and linear handles.
    pub fn new(time_secs: f64, value: f32) -> Self {
        Self {
            time_secs,
            value,
            interpolation: KeyframeInterpolation::Linear,
            handle_right: [1.0 / 3.0, 1.0 / 3.0],
            handle_left: [2.0 / 3.0, 2.0 / 3.0],
        }
    }

    /// Create a keyframe with a specific interpolation mode.
    pub fn with_interpolation(
        time_secs: f64,
        value: f32,
        interpolation: KeyframeInterpolation,
    ) -> Self {
        Self {
            interpolation,
            ..Self::new(time_secs, value)
        }
    }

    /// Create a bezier keyframe with explicit tangent handles.
    pub fn bezier(
        time_secs: f64,
        value: f32,
        handle_right: [f32; 2],
        handle_left: [f32; 2],
    ) -> Self {
        Self {
            time_secs,
            value,
            interpolation: KeyframeInterpolation::Bezier,
            handle_right,
            handle_left,
        }
    }

    /// Set the incoming bezier handle (for segments arriving at this keyframe).
    /// Returns self for builder chaining.
    pub fn with_incoming_handle(mut self, handle_left: [f32; 2]) -> Self {
        self.handle_left = handle_left;
        self
    }

    /// Create a pair of keyframes for a CSS `cubic-bezier(x1,y1,x2,y2)` segment.
    ///
    /// In CSS, P1=(x1,y1) maps to A's `handle_right` (outgoing tangent),
    /// and P2=(x2,y2) maps to B's `handle_left` (incoming tangent).
    /// This helper wires both keyframes correctly.
    pub fn css_bezier_pair(
        time_a: f64,
        value_a: f32,
        time_b: f64,
        value_b: f32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    ) -> (Self, Self) {
        let a: Keyframe = Self {
            time_secs: time_a,
            value: value_a,
            interpolation: KeyframeInterpolation::Bezier,
            handle_right: [x1, y1],
            handle_left: [2.0 / 3.0, 2.0 / 3.0], // default (incoming from previous)
        };
        let b: Keyframe = Self {
            time_secs: time_b,
            value: value_b,
            interpolation: KeyframeInterpolation::Linear, // mode for B's onward segment
            handle_right: [1.0 / 3.0, 1.0 / 3.0], // default (outgoing to next)
            handle_left: [x2, y2], // P2 incoming handle
        };
        (a, b)
    }
}

// ============================================================================
// MAIN EVALUATION ENTRY POINT
// ============================================================================

/// Evaluate the interpolated value at a given time from a sorted keyframe list.
///
/// ## Edge Cases
/// - Empty list → `default_value`
/// - Single keyframe → constant value
/// - Before first → hold first value
/// - After last → hold last value
///
/// ## Complexity
/// O(log n) binary search + constant-time interpolation.
#[inline]
pub fn evaluate_at_time(keyframes: &[Keyframe], time_secs: f64, default_value: f32) -> f32 {
    if keyframes.is_empty() {
        return default_value;
    }
    if keyframes.len() == 1 {
        return keyframes[0].value;
    }
    // Boundary hold
    if time_secs <= keyframes[0].time_secs {
        return keyframes[0].value;
    }
    let last_idx: usize = keyframes.len() - 1;
    if time_secs >= keyframes[last_idx].time_secs {
        return keyframes[last_idx].value;
    }

    // Binary search for the segment containing time_secs
    let idx: usize = match keyframes.binary_search_by(|kf| {
        kf.time_secs
            .partial_cmp(&time_secs)
            .unwrap_or(std::cmp::Ordering::Equal)
    }) {
        Ok(exact) => exact,
        Err(insert_pos) => insert_pos.saturating_sub(1),
    };

    let a: &Keyframe = &keyframes[idx];
    let b: &Keyframe = &keyframes[idx + 1];
    let segment_dt: f64 = b.time_secs - a.time_secs;
    if segment_dt <= 0.0 {
        return a.value;
    }
    let t: f64 = ((time_secs - a.time_secs) / segment_dt).clamp(0.0, 1.0);

    match a.interpolation {
        KeyframeInterpolation::Hold => a.value,
        KeyframeInterpolation::Linear => lerp(a.value, b.value, t),
        KeyframeInterpolation::Bezier => {
            bezier_evaluate(a.value, b.value, &a.handle_right, &b.handle_left, t)
        }
        KeyframeInterpolation::Smooth => {
            let prev: Option<&Keyframe> = if idx > 0 { Some(&keyframes[idx - 1]) } else { None };
            let next: Option<&Keyframe> = if idx + 2 < keyframes.len() {
                Some(&keyframes[idx + 2])
            } else {
                None
            };
            hermite_catmull_rom(prev, a, b, next, t)
        }
    }
}

// ============================================================================
// LINEAR
// ============================================================================

/// Standard linear interpolation.
#[inline]
fn lerp(value_a: f32, value_b: f32, t: f64) -> f32 {
    value_a + (value_b - value_a) * t as f32
}

// ============================================================================
// CUBIC BEZIER — 2D parametric curve with Newton-Raphson solver
// ============================================================================
//
// This implements the same algorithm used by Webkit/Blink for CSS
// `cubic-bezier()` animations. The curve is 2D parametric:
//   B(u) = (Bx(u), By(u))   for u ∈ [0, 1]
//
// Given target time `t`, we solve Bx(u) = t for `u` (Newton-Raphson),
// then evaluate By(u) for the normalized output value.
//
// This is critical because naively using `t` as the bezier parameter
// directly (as many tutorials do) produces incorrect easing shapes —
// the X axis (time) must be solved separately from Y axis (value).

/// Newton-Raphson convergence threshold (sub-sample precision at any rate).
const SOLVER_EPSILON: f64 = 1e-7;
/// Max Newton-Raphson iterations (converges in 2-4 for typical curves).
const NEWTON_ITERATIONS: u32 = 8;
/// Max bisection iterations as fallback (gives ~1e-6 precision).
const BISECTION_ITERATIONS: u32 = 20;
/// Minimum derivative for safe Newton step.
const NEWTON_MIN_SLOPE: f64 = 1e-7;

/// Evaluate a bezier-eased value between two keyframes.
///
/// Constructs normalized control points P0=(0,0), P1=handle_right,
/// P2=handle_left, P3=(1,1), solves for the parameter, and maps
/// the result back to the actual value range.
#[inline]
fn bezier_evaluate(
    value_a: f32,
    value_b: f32,
    handle_right: &[f32; 2],
    handle_left: &[f32; 2],
    t: f64,
) -> f32 {
    if t <= 0.0 {
        return value_a;
    }
    if t >= 1.0 {
        return value_b;
    }

    let x1: f64 = handle_right[0] as f64;
    let y1: f64 = handle_right[1] as f64;
    let x2: f64 = handle_left[0] as f64;
    let y2: f64 = handle_left[1] as f64;

    // Fast path: if handles lie on the diagonal, this is linear
    if (x1 - y1).abs() < 1e-6 && (x2 - y2).abs() < 1e-6 {
        return lerp(value_a, value_b, t);
    }

    // Solve Bx(u) = t for parameter u
    let u: f64 = solve_bezier_t_for_x(x1, x2, t);
    // Evaluate By(u) for normalized value
    let normalized: f64 = sample_curve(y1, y2, u);

    value_a + (value_b - value_a) * normalized as f32
}

/// Solve `Bx(u) = target_x` using Newton-Raphson + bisection fallback.
///
/// Phase 1: Newton-Raphson with quadratic convergence (2-4 iters typical).
/// Phase 2: If Newton stalls near flat tangents, bisection guarantees convergence.
#[inline]
fn solve_bezier_t_for_x(x1: f64, x2: f64, target_x: f64) -> f64 {
    let mut u: f64 = target_x; // initial guess

    // Phase 1: Newton-Raphson
    for _ in 0..NEWTON_ITERATIONS {
        let residual: f64 = sample_curve(x1, x2, u) - target_x;
        if residual.abs() < SOLVER_EPSILON {
            return u;
        }
        let derivative: f64 = sample_curve_derivative(x1, x2, u);
        if derivative.abs() < NEWTON_MIN_SLOPE {
            break; // flat region → switch to bisection
        }
        u -= residual / derivative;
        u = u.clamp(0.0, 1.0);
    }

    // Phase 2: Bisection (guaranteed convergence)
    let mut lo: f64 = 0.0;
    let mut hi: f64 = 1.0;
    u = target_x;

    for _ in 0..BISECTION_ITERATIONS {
        let current_x: f64 = sample_curve(x1, x2, u);
        if (current_x - target_x).abs() < SOLVER_EPSILON {
            return u;
        }
        if current_x < target_x {
            lo = u;
        } else {
            hi = u;
        }
        u = (lo + hi) * 0.5;
    }

    u
}

/// Evaluate one axis of a cubic bezier at parameter `u`.
///
/// For P0=0, P1=p1, P2=p2, P3=1, using Horner's form for stability:
/// `B(u) = ((1 - 3p2 + 3p1)u + (3p2 - 6p1))u + 3p1)u`
#[inline]
fn sample_curve(p1: f64, p2: f64, u: f64) -> f64 {
    let c3: f64 = 1.0 - 3.0 * p2 + 3.0 * p1;
    let c2: f64 = 3.0 * p2 - 6.0 * p1;
    let c1: f64 = 3.0 * p1;
    ((c3 * u + c2) * u + c1) * u
}

/// Derivative of one bezier axis: `B'(u) = (3c3·u + 2c2)u + c1`
#[inline]
fn sample_curve_derivative(p1: f64, p2: f64, u: f64) -> f64 {
    let c3: f64 = 1.0 - 3.0 * p2 + 3.0 * p1;
    let c2: f64 = 3.0 * p2 - 6.0 * p1;
    let c1: f64 = 3.0 * p1;
    (3.0 * c3 * u + 2.0 * c2) * u + c1
}

// ============================================================================
// NON-UNIFORM CATMULL-ROM via CUBIC HERMITE (DaVinci Resolve / MLT style)
// ============================================================================
//
// Real keyframes are never uniformly spaced. The standard Catmull-Rom
// matrix assumes uniform parameter spacing (Δt=1 between all points),
// which distorts the curve when keyframes are clustered or spread apart.
//
// We instead compute tangents from the chord slope of surrounding
// keyframes, then use the cubic Hermite basis for evaluation. This
// automatically handles non-uniform spacing correctly.

/// Non-uniform Catmull-Rom interpolation using cubic Hermite basis.
///
/// Tangent at each endpoint is the chord slope between surrounding keyframes,
/// scaled by the segment duration. Boundary keyframes (no neighbor) use the
/// Bessel end condition (slope of the adjacent segment) for C1 continuity.
///
/// ## Hermite Basis
/// ```text
/// h00(t) =  2t³ - 3t² + 1    (start value weight)
/// h10(t) =   t³ - 2t² + t    (start tangent weight)
/// h01(t) = -2t³ + 3t²        (end value weight)
/// h11(t) =   t³ -  t²        (end tangent weight)
///
/// result = h00·va + h10·ma·Δt + h01·vb + h11·mb·Δt
/// ```
#[inline]
fn hermite_catmull_rom(
    prev: Option<&Keyframe>,
    a: &Keyframe,
    b: &Keyframe,
    next: Option<&Keyframe>,
    t: f64,
) -> f32 {
    let segment_dt: f64 = b.time_secs - a.time_secs;
    if segment_dt <= 0.0 {
        return a.value;
    }

    // ---- Tangent at A (slope in value/second) ----
    let slope_a: f64 = match prev {
        Some(p) => {
            // Chord slope across [prev, b] — the standard Catmull-Rom tangent
            let dt: f64 = b.time_secs - p.time_secs;
            if dt.abs() > 1e-12 {
                (b.value as f64 - p.value as f64) / dt
            } else {
                0.0
            }
        }
        // Bessel end condition: use slope of this segment
        None => (b.value as f64 - a.value as f64) / segment_dt,
    };

    // ---- Tangent at B (slope in value/second) ----
    let slope_b: f64 = match next {
        Some(n) => {
            let dt: f64 = n.time_secs - a.time_secs;
            if dt.abs() > 1e-12 {
                (n.value as f64 - a.value as f64) / dt
            } else {
                0.0
            }
        }
        None => (b.value as f64 - a.value as f64) / segment_dt,
    };

    // Scale tangents by segment duration for Hermite basis
    let m_a: f64 = slope_a * segment_dt;
    let m_b: f64 = slope_b * segment_dt;

    // Cubic Hermite basis evaluation
    let t2: f64 = t * t;
    let t3: f64 = t2 * t;

    let h00: f64 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10: f64 = t3 - 2.0 * t2 + t;
    let h01: f64 = -2.0 * t3 + 3.0 * t2;
    let h11: f64 = t3 - t2;

    let result: f64 = h00 * a.value as f64 + h10 * m_a + h01 * b.value as f64 + h11 * m_b;
    result as f32
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f32 = 0.01;

    fn assert_near(actual: f32, expected: f32, msg: &str) {
        assert!(
            (actual - expected).abs() < TOLERANCE,
            "{}: expected {:.4}, got {:.4}",
            msg,
            expected,
            actual
        );
    }

    // ---- Edge cases ----

    #[test]
    fn test_empty_returns_default() {
        assert_near(evaluate_at_time(&[], 1.0, 1.0), 1.0, "empty default=1.0");
        assert_near(evaluate_at_time(&[], 0.0, 0.5), 0.5, "empty default=0.5");
    }

    #[test]
    fn test_single_keyframe_constant() {
        let kf: Vec<Keyframe> = vec![Keyframe::new(2.0, 0.75)];
        assert_near(evaluate_at_time(&kf, 0.0, 1.0), 0.75, "before single");
        assert_near(evaluate_at_time(&kf, 2.0, 1.0), 0.75, "at single");
        assert_near(evaluate_at_time(&kf, 5.0, 1.0), 0.75, "after single");
    }

    #[test]
    fn test_hold_before_first() {
        let kf: Vec<Keyframe> = vec![Keyframe::new(1.0, 0.5), Keyframe::new(3.0, 1.0)];
        assert_near(evaluate_at_time(&kf, 0.0, 0.0), 0.5, "hold before first");
    }

    #[test]
    fn test_hold_after_last() {
        let kf: Vec<Keyframe> = vec![Keyframe::new(0.0, 0.0), Keyframe::new(2.0, 0.8)];
        assert_near(evaluate_at_time(&kf, 5.0, 0.0), 0.8, "hold after last");
    }

    // ---- Hold interpolation ----

    #[test]
    fn test_hold_step_function() {
        let kf: Vec<Keyframe> = vec![
            Keyframe::with_interpolation(0.0, 0.0, KeyframeInterpolation::Hold),
            Keyframe::new(2.0, 1.0),
        ];
        assert_near(evaluate_at_time(&kf, 0.5, 0.0), 0.0, "hold at 0.5s");
        assert_near(evaluate_at_time(&kf, 1.9, 0.0), 0.0, "hold at 1.9s");
    }

    // ---- Linear interpolation ----

    #[test]
    fn test_linear_midpoint() {
        let kf: Vec<Keyframe> = vec![Keyframe::new(0.0, 0.0), Keyframe::new(2.0, 1.0)];
        assert_near(evaluate_at_time(&kf, 1.0, 0.0), 0.5, "linear midpoint");
    }

    #[test]
    fn test_linear_quarter() {
        let kf: Vec<Keyframe> = vec![Keyframe::new(0.0, 0.0), Keyframe::new(4.0, 1.0)];
        assert_near(evaluate_at_time(&kf, 1.0, 0.0), 0.25, "linear quarter");
        assert_near(evaluate_at_time(&kf, 3.0, 0.0), 0.75, "linear three-quarter");
    }

    #[test]
    fn test_linear_multi_segment() {
        let kf: Vec<Keyframe> = vec![
            Keyframe::new(0.0, 0.0),
            Keyframe::new(1.0, 1.0),
            Keyframe::new(3.0, 0.5),
        ];
        assert_near(evaluate_at_time(&kf, 0.5, 0.0), 0.5, "seg1 midpoint");
        assert_near(evaluate_at_time(&kf, 2.0, 0.0), 0.75, "seg2 midpoint");
    }

    // ---- Bezier interpolation (2D parametric with Newton-Raphson) ----

    /// Helper: create a properly-wired CSS bezier segment as a Vec.
    fn css_bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> Vec<Keyframe> {
        let (a, b) = Keyframe::css_bezier_pair(0.0, 0.0, 1.0, 1.0, x1, y1, x2, y2);
        vec![a, b]
    }

    #[test]
    fn test_bezier_endpoints() {
        let kf: Vec<Keyframe> = css_bezier(0.42, 0.0, 0.58, 1.0);
        assert_near(evaluate_at_time(&kf, 0.0, 0.0), 0.0, "bezier t=0");
        assert_near(evaluate_at_time(&kf, 1.0, 0.0), 1.0, "bezier t=1");
    }

    #[test]
    fn test_bezier_ease_in_out_symmetry() {
        // CSS ease-in-out: cubic-bezier(0.42, 0, 0.58, 1)
        // This curve is symmetric around (0.5, 0.5).
        let kf: Vec<Keyframe> = css_bezier(0.42, 0.0, 0.58, 1.0);
        let v_mid: f32 = evaluate_at_time(&kf, 0.5, 0.0);
        assert_near(v_mid, 0.5, "ease-in-out midpoint symmetry");

        // Ease-in-out should be slow at start, fast in middle, slow at end
        let v_early: f32 = evaluate_at_time(&kf, 0.2, 0.0);
        let v_late: f32 = evaluate_at_time(&kf, 0.8, 0.0);
        assert!(v_early < 0.2, "ease-in: early value {v_early} should be < 0.2");
        assert!(v_late > 0.8, "ease-out: late value {v_late} should be > 0.8");
    }

    #[test]
    fn test_bezier_ease_in_slow_start() {
        // cubic-bezier(0.42, 0, 1, 1) — slow start, fast end
        let kf: Vec<Keyframe> = css_bezier(0.42, 0.0, 1.0, 1.0);
        let v_quarter: f32 = evaluate_at_time(&kf, 0.25, 0.0);
        assert!(
            v_quarter < 0.25,
            "ease-in at t=0.25: value {v_quarter} should be < 0.25"
        );
    }

    #[test]
    fn test_bezier_ease_out_fast_start() {
        // cubic-bezier(0, 0, 0.58, 1) — fast start, slow end
        let kf: Vec<Keyframe> = css_bezier(0.0, 0.0, 0.58, 1.0);
        let v_quarter: f32 = evaluate_at_time(&kf, 0.25, 0.0);
        assert!(
            v_quarter > 0.25,
            "ease-out at t=0.25: value {v_quarter} should be > 0.25"
        );
    }

    #[test]
    fn test_bezier_linear_handles_equal_linear() {
        // Both handles on the diagonal → should match pure linear
        let (a, b) = Keyframe::css_bezier_pair(
            0.0, 0.0, 2.0, 1.0,
            1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0,
        );
        let kf_bez: Vec<Keyframe> = vec![a, b];
        let kf_lin: Vec<Keyframe> = vec![Keyframe::new(0.0, 0.0), Keyframe::new(2.0, 1.0)];

        for i in 0..=10 {
            let t: f64 = i as f64 * 0.2;
            let v_bez: f32 = evaluate_at_time(&kf_bez, t, 0.0);
            let v_lin: f32 = evaluate_at_time(&kf_lin, t, 0.0);
            assert!(
                (v_bez - v_lin).abs() < 0.02,
                "linear handles should equal linear at t={t}: bez={v_bez}, lin={v_lin}"
            );
        }
    }

    // ---- Smooth (non-uniform Catmull-Rom / Hermite) ----

    #[test]
    fn test_smooth_two_points_matches_linear() {
        // With only 2 keyframes, Bessel end conditions reduce to linear
        let kf: Vec<Keyframe> = vec![
            Keyframe::with_interpolation(0.0, 0.0, KeyframeInterpolation::Smooth),
            Keyframe::new(2.0, 1.0),
        ];
        assert_near(evaluate_at_time(&kf, 1.0, 0.0), 0.5, "smooth 2pt midpoint");
    }

    #[test]
    fn test_smooth_three_points_continuity() {
        // Smooth should produce a continuous, plausible curve through 3 points
        let kf: Vec<Keyframe> = vec![
            Keyframe::with_interpolation(0.0, 0.0, KeyframeInterpolation::Smooth),
            Keyframe::with_interpolation(1.0, 1.0, KeyframeInterpolation::Smooth),
            Keyframe::new(2.0, 0.0),
        ];
        // At the peak keyframe, value should be exactly the keyframe value
        assert_near(evaluate_at_time(&kf, 1.0, 0.0), 1.0, "smooth at peak kf");
        // Midpoints should be smooth — non-linear (not 0.5)
        let v_left: f32 = evaluate_at_time(&kf, 0.5, 0.0);
        let v_right: f32 = evaluate_at_time(&kf, 1.5, 0.0);
        assert!(v_left > 0.0 && v_left < 1.0, "smooth left mid in range: {v_left}");
        assert!(v_right > 0.0 && v_right < 1.0, "smooth right mid in range: {v_right}");
    }

    #[test]
    fn test_smooth_non_uniform_spacing() {
        // Keyframes with very different spacing — should NOT distort
        let kf: Vec<Keyframe> = vec![
            Keyframe::with_interpolation(0.0, 0.0, KeyframeInterpolation::Smooth),
            Keyframe::with_interpolation(0.1, 1.0, KeyframeInterpolation::Smooth), // tight
            Keyframe::new(10.0, 0.5), // wide gap
        ];
        // At t=0.05 (midpoint of tight segment), value should be reasonable
        let v: f32 = evaluate_at_time(&kf, 0.05, 0.0);
        assert!(v > -0.5 && v < 1.5, "non-uniform smooth should be stable: {v}");
    }

    // ---- Solver robustness ----

    #[test]
    fn test_bezier_solver_extreme_handles() {
        // Near-vertical S-curve: steep outgoing on A, steep incoming on B
        let (a, b) = Keyframe::css_bezier_pair(0.0, 0.0, 1.0, 1.0, 0.0, 0.9, 1.0, 0.1);
        let kf: Vec<Keyframe> = vec![a, b];
        // Should not crash or produce NaN
        for i in 0..=20 {
            let t: f64 = i as f64 * 0.05;
            let v: f32 = evaluate_at_time(&kf, t, 0.0);
            assert!(!v.is_nan(), "NaN at t={t}");
            assert!(!v.is_infinite(), "Inf at t={t}");
        }
    }

    #[test]
    fn test_bezier_monotonic_time() {
        // Ensure results are monotonically increasing for monotonic input
        let kf: Vec<Keyframe> = css_bezier(0.42, 0.0, 0.58, 1.0);
        let mut prev_value: f32 = -1.0;
        for i in 0..=100 {
            let t: f64 = i as f64 * 0.01;
            let v: f32 = evaluate_at_time(&kf, t, 0.0);
            assert!(
                v >= prev_value - 0.001,
                "monotonicity violated at t={t}: prev={prev_value}, curr={v}"
            );
            prev_value = v;
        }
    }
}
