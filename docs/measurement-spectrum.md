# Measured ACF spectrum

The ion-line browser evaluates a real matrix DFT in Rust/WASM from the selected
profile's measured complex ACF. It does not rerun or alter the server fit.
BEATA's 10 microsecond lag grid is explicit and validated; unsupported lag grids
fail rather than silently changing spectral units.

For R(tau) = integral S(f) exp(+i 2 pi f tau) df, the estimate is

    S_mask(f) = dt R(0) + 2 dt sum_measured_positive_lags
                       [Re R(tau) cos(2 pi f tau) + Im R(tau) sin(2 pi f tau)].

The zero-lag term is included only when measured. Repeated response groups at a
lag are averaged with equal weights. The matrix contains columns only for actual
measurements, with those averaging weights included. No lag interpolation,
SVD, positivity clipping, fitted baseline insertion, or missing-lag amplitude
renormalization is performed. This operator is mathematically equivalent to a
masked, finite-lag Hermitian DFT; it does not reconstruct unmeasured lags.

The grey curve retains the experiment response and is in ACF-units times seconds
(m^-3 Hz^-1 for these normalized ion-line products). It is not the intrinsic ISR
spectrum: receiver/range ambiguity and the lag sampling window remain. Missing
zero lag means its absolute spectral baseline is undetermined. The gold curve
uses the *same matrix* on the individually predicted response groups, so it has
the same missing-lag effects. The separate analytical spectrum is intrinsic.

Grey shading is pointwise +/- one standard error at 30% opacity, computed as
sqrt(diag(A diag(sigma^2) A^T)). The exported product does not contain cross-lag,
real/imaginary, or cross-group covariance. Consequently this is a diagonal noise
approximation, not a full self-noise uncertainty estimate. Frequency bins of the
plotted spectrum are also correlated; 100 Hz plot sampling does not mean 100 Hz
spectral resolution. Changes to altitude, time or resolution recompute from the
selected full-detail ACF, including failed fits when measurements are available.

Tests cover the complete-grid Hermitian DFT limit, frequency sign, missing zero
and intermediate lags, duplicate-group normalization, diagonal noise propagation,
and invalid inputs.
