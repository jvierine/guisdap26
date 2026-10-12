//! Explicit display screening; never changes fitted measurements or parameters.
#[derive(Clone, Copy)]
pub struct Limits {
    pub density_relative_error: f64,
    pub temperature_relative_error: f64,
    pub velocity_error_m_s: f64,
    pub minimum_signal_significance: f64,
    pub maximum_residual: Option<f64>,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            density_relative_error: 0.5,
            temperature_relative_error: 1.,
            velocity_error_m_s: 300.,
            minimum_signal_significance: 3.,
            maximum_residual: None,
        }
    }
}
pub fn reasons(
    status: &str,
    parameters: [f64; 4],
    errors: [Option<f64>; 4],
    significance: Option<f64>,
    residual: f64,
    limits: Limits,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    match status {
        "converged" | "model_mismatch" => (),
        "parameter_boundary" => reasons.push("parameter at bound"),
        "not_converged" => reasons.push("optimizer did not converge"),
        "low_information" => reasons.push("poorly constrained fit"),
        "weak_signal" => reasons.push("weak signal"),
        "singular" => reasons.push("singular covariance"),
        _ => reasons.push("fit unavailable or invalid"),
    }
    if significance.is_none_or(|s| !s.is_finite() || s < limits.minimum_signal_significance) {
        reasons.push("low or missing signal significance");
    }
    let labels = [
        "Ne uncertainty",
        "Ti uncertainty",
        "Te uncertainty",
        "velocity uncertainty",
    ];
    for i in 0..4 {
        let cap = match i {
            0 => parameters[0] * limits.density_relative_error,
            1 | 2 => parameters[i] * limits.temperature_relative_error,
            _ => limits.velocity_error_m_s,
        };
        if !parameters[i].is_finite()
            || (i < 3 && parameters[i] <= 0.)
            || errors[i].is_none_or(|e| !e.is_finite() || e < 0. || e > cap)
        {
            reasons.push(labels[i]);
        }
    }
    if !residual.is_finite() || limits.maximum_residual.is_some_and(|max| residual > max) {
        reasons.push("residual mismatch");
    }
    reasons
}
#[cfg(test)]
mod tests {
    use super::*;
    fn check(
        status: &str,
        errors: [Option<f64>; 4],
        signal: Option<f64>,
        limits: Limits,
    ) -> Vec<&'static str> {
        reasons(
            status,
            [1e11, 1000., 2000., 0.],
            errors,
            signal,
            0.2,
            limits,
        )
    }
    const GOOD: [Option<f64>; 4] = [Some(1e10), Some(100.), Some(200.), Some(50.)];
    #[test]
    fn low_residual_does_not_rescue_failed_optimizer() {
        for status in [
            "parameter_boundary",
            "not_converged",
            "singular",
            "low_information",
        ] {
            assert!(!check(status, GOOD, Some(10.), Limits::default()).is_empty());
        }
    }
    #[test]
    fn noisy_topside_is_rejected_even_with_good_residual() {
        let mut e = GOOD;
        e[0] = Some(2e11);
        assert!(check("converged", e, Some(10.), Limits::default()).contains(&"Ne uncertainty"));
        assert!(!check("converged", GOOD, Some(1.), Limits::default()).is_empty());
    }
    #[test]
    fn missing_uncertainty_is_not_zero_uncertainty() {
        let mut e = GOOD;
        e[2] = None;
        assert!(check("converged", e, Some(10.), Limits::default()).contains(&"Te uncertainty"));
    }
    #[test]
    fn constrained_zero_velocity_is_valid() {
        assert!(check("converged", GOOD, Some(10.), Limits::default()).is_empty());
    }
    #[test]
    fn adjustable_error_limit_does_not_disable_status_checks() {
        let mut e = GOOD;
        e[0] = Some(7e10);
        let l = Limits {
            density_relative_error: 1.,
            ..Limits::default()
        };
        assert!(check("converged", e, Some(10.), l).is_empty());
        assert!(!check("not_converged", e, Some(10.), l).is_empty());
    }
}
