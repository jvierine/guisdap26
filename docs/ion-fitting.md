# Ion-line fitting

The `guisdap-ion` crate provides a Rust-only spectrum model and nonlinear fit.
Its API passes `SpectrumModel`, `PlasmaState`, `FitSettings`, and `FitProblem`
explicitly. A result owns its parameters, covariance, predictions, residuals,
and status. There is no mutable global state, interpreter, external fitting
process, or dependence on the order in which fits run.

## First BEATA analysis

Run on avaruus from the repository directory:

```sh
cargo run --release -p guisdap-worker -- fit-ion --config config/ion-fit-avaruus.json
```

The configuration selects the existing HDF5 frames and their raw-source hashes,
the local numerical experiment-response model, output paths, integration length,
altitude bins, composition, calibration, and worker count. The native Rust MAT
reader verifies each original recording against its product hash. The Rust
`matfile` reader imports the numerical response arrays; no legacy analysis code
is executed or redistributed. Unsupported model hashes and incomplete/overlapping
address maps fail explicitly.

The default run covers 17:00–17:15 UTC on 2022-11-02 in sixty independent
15-second windows, each containing three native five-second records. This is
four times the original cadence. The original fifteen 60-second products remain
available; `config/ion-fit-avaruus-60s.json` reproduces that integration.
The 15-second product is stored separately as `2022-11-02-ionfit-15s-v1`.
Altitude bins begin at 47 km. No low-altitude or low-SNR data are silently removed.
The grid uses 10 km bins below 197 km and 20 km bins above, ending at 697 km.
Every available decoded ion lag (10–410 microseconds, addresses 480–25530)
is included at its original address. Fractional-code groups can contain several
responses for a nominal lag; those responses remain distinct. The 14 short-lag
slices are repetitions of the first aggregate lag and are not double-counted.

## Forward model and calibration

For each singly charged Maxwellian species, evaluate the unmagnetized free-particle
response using the Faddeeva function. Number-conserving BGK collisions use
`G = G0/(1 - nu G0)` and susceptibility proportional to `1 + i omega G`.
The dielectric spectrum includes electron shielding, ion composition, finite
Debye length, electron and ion temperatures, and a common drift. Spectra are
per Hz per electron; both frequency signs are included. Collision rates are
in inverse seconds, not angular frequencies or cycles per second.

The prediction is the integral of this spectrum against the experiment's
**complex spectral response**, using all 121 native frequency nodes and
trapezoidal quadrature. The imported response is divided by its accumulation
factor and its maximum real response; the identical normalization is applied
to the measurements. No ideal-ACF approximation, missing-lag interpolation,
lag rejection, FFT fitting, or truncated SVD is involved.

Signal calibration uses channel 1 background and injected calibration samples,
not the differently tuned gain channel. The decoded nonzero lags have no
background subtraction, as specified by the experiment model. Each individual
sample receives its own range-squared correction and recorded transmitter power
before range or time averaging. Antenna efficiency 0.66 and calibration multiplier
0.7 retain the event's existing calibration assumption. The response model uses
930 MHz. These constants are not independently remeasured by this fit.

Within an altitude bin the plasma is assumed homogeneous. The exact **reduced
spectral** response is used; this is not a two-dimensional range–lag inversion.
The maximum equivalent response width is retained with each bin, and range
inhomogeneity can bias the parameters. Broad zero-lag power has an approximately
96 km response and is **not** inserted into these narrow bins. Combining that
measurement with the decoded lags requires a joint range model and remains
outstanding. Existing signed raw-power products are preserved.

## Explicit first-pass assumptions

The ion mixture is O+ (16 amu) and an effective molecular species (30.5 amu).
The oxygen fraction is fixed to `1/(1+exp(-(h-180)/20))`, with altitude in km.
This is a configurable starting composition assumption, not a composition
measurement or an IRI calculation. Composition uncertainty is not in the bars.

Below 120 km, Te is tied to Ti. Below 150 km, the effective ion BGK rate is fitted;
above that altitude it is fixed to zero. The electron collision rate is zero.
The low-altitude model is a first approximation, not a validated full magnetized,
ion-neutral collision calculation. Changing those assumptions can change the
inferred temperatures and density. The lowest bins often cannot identify all
parameters and must be judged from their flags and residuals.

The velocity convention is positive model spectral frequency shift. Physical
toward/away sign remains to be verified from receiver conventions. It is labelled
spectral velocity in the GUI.

## Optimization and uncertainties

Bounded Levenberg–Marquardt uses logarithms of positive parameters and Cholesky
linear solves. Two starting temperatures are tried; the lower weighted residual
sum is retained. No priors or cross-altitude smoothing are applied. Bounds are
Ne 1e7–1e14 m^-3, Ti 80–10000 K, Te 80–20000 K, velocity +/-3000 m/s, and fitted
BGK rate 0.1–1e6 s^-1. A parameter at a bound is flagged, not presented as a
successful unconstrained measurement.

Each five-second record is calibrated and averaged within each response group
separately. Temporal variance of these group averages provides the weighting
variance of the integration mean. The final parameter covariance is a temporal
cluster sandwich estimate: correlations between lags and range averages within
a record are included. Its diagonal is increased to the conditional diagonal
variance when the latter is larger. Both error estimates are saved. The three
records per 15-second window (12 for a 60-second window) are assumed independent; their temporal correlation, instrument systematics,
and composition uncertainty remain unaccounted for. Actual time variation within
an integration increases the empirical errors and can invalidate a stationary model.
With only three records, the estimated variances and covariance are less stable;
quality flags and uncertainty bands remain essential. The current estimator
requires at least three records, so 15 seconds is its shortest supported
integration; five-second fits require a separate within-record noise model.

The diagonal weighted residual sum divided by nominal degrees of freedom is
saved as `reduced_chi_square`, but is displayed as **weighted residual score**.
Correlated observations mean it is not a calibrated chi-square significance test.
Numerical convergence, parameter boundaries, singularity, and weak information
are recorded. A first-lag mean below three empirical standard errors flags
`weak_signal`; a score above 9 flags `model_mismatch`. These are inspection flags,
not data rejection or detection significance statements. Signed measurements
and every fitted result remain in HDF5 even when the fit is flagged.

## Products and GUI

`/home/j/data/guisdap/2022-11-02-ionfit-v1/ion-fits.h5` contains parameters,
uncertainties, covariance, observed/predicted complex lags, standardized residuals,
source addresses, integration bounds, model hash, configuration, and source hashes.
Atomic file replacement prevents partial products being published.

The browser loads a static JSON export at `guisdap/ion.html`. All plotting and
interaction run in Rust/WASM, with only the wasm-bindgen loading glue in JavaScript.
The parameter axes are fixed across time. The selected altitude shows all measured
responses and their fitted predictions, including real and imaginary components.
Boundary and weak fits use open markers. Server-local source paths are excluded
from the public JSON. The GUI does not require a compute server or raw files.

## Validation

Rust tests compare the plasma response against independent time-domain Simpson
integration, verify spectral symmetry and drift sign, check the equilibrium
ion-line integral, recover collisionless and collisional synthetic parameters,
reject invalid inputs, retain signed measurements, and check fit-order independence.
A cluster-covariance regression duplicates perfectly correlated lag observations
and requires the inferred density uncertainty to remain unchanged. The HDF5
inspection example checks native arrays against the serialized diagnostics.

These tests validate numerical implementation; they do not replace an independent
full GUISDAP comparison or establish the composition and low-altitude collision
model for this event. The plasma-line tuning ambiguity is separate and unresolved.


The gold lag-model line averages predicted responses over the groups at each
measured lag. Individual measured groups remain visible and enter the fit
separately. Its 30% opacity band shows pointwise ±1σ uncertainty of that fitted
mean. The response Jacobians are averaged before applying the full fitted
parameter covariance (`g C gᵀ`), including off-diagonal terms and tied-temperature
constraints. Predictions sharing fitted parameters are not independent samples.
This linearized band excludes measurement noise and fixed composition/calibration
uncertainty; boundary and unconverged fits remain provisional. Unavailable
covariance produces a line without a band. The display does not create lag data
or affect the fit. HDF5 `gate_NNN/model_curve` stores lag seconds, real/imaginary
means and their standard errors; unavailable errors are NaN (null in JSON).


The ion GUI includes time–altitude colour meshes of Ne, Ti, Te and spectral
velocity with fixed colour limits and logarithmic Ne. Flagged fits are hatched;
singular/missing fits are blank. Separate controls choose the diagnostic altitude
and the lower/upper displayed altitude bounds. Display limits never filter or
refit the stored measurements.


## Whole-dataset streaming products

`run-study` prepares lightweight ion inputs, then fits all UTC-aligned windows at
15 s and 60 s using 128 configurable workers. The browser overview carries only
parameters, uncertainties, flags and raw Ne. Native lag measurements, covariance
and model curves are loaded from immutable per-interval JSON on selection; the
complete equivalent remains in per-interval HDF5. Cache keys include source
metadata and numerical configuration, excluding publication paths and thread count.

Five of the 958 15 s windows contain two native records: their fits are explicitly
`insufficient_records`, while raw power remains available. All 240 one-minute
windows meet the record-count requirement. A completed interval can still contain
nonconverged, boundary or weak-signal fits; the browser preserves those flags.

The extra raw-Ne row is the signed 10 µs short-lag proxy averaged into the fit
altitude bins, not fitted Ne or a newly inferred zero-lag estimate. Its uncertainty
conservatively treats range samples as correlated and temporal samples as
independent, with empirical temporal variance as a lower bound. Negative estimates
remain in HDF5/JSON and are blank on logarithmic colour scales.


## Interactive display controls

The gear panel controls UTC/altitude bounds, a maximum weighted residual score
(default 10; logarithmic slider 0.1–1000), optional NaN display masking, hatching,
and each row's colour limits. Rejection follows the score rather than treating
every non-converged status as a hatch. Original status remains visible in hover
and diagnostics; a low score does not establish convergence or scientific validity.
Missing/singular fits remain unavailable. Masking leaves HDF5 and measured ACFs
unchanged. Raw Ne is not filtered by fit quality. Settings survive resolution
changes during the session. Te defaults to 0–4000 K; Ti to 0–2000 K. Velocity uses
a seismic blue–white–red scale centred at zero, including asymmetric limits.
Dragging previews the shared time selection across all rows; leaving the plot
cancels the drag. Powers of ten use locally served MathJax 3.2.2 SVG rendering.


Display screening now rejects optimizer failures, bound-limited solutions and
singular/low-information fits independently of residual score. Adjustable checks
use the exported final standard errors: default σNe/Ne ≤ 50%, σTi/Ti and σTe/Te
≤ 100%, σv ≤ 300 m/s, and first-lag mean / temporal standard error ≥ 3.
These are pragmatic display defaults, not a calibrated probability of correctness.
NaN masking defaults on; raw measurements and diagnostics stay inspectable.
The residual cutoff is optional and defaults off because correlated lags and
few-record variance estimates invalidate a simple chi-square interpretation.
Colour settings update on input and also have explicit Apply buttons with applied
limits shown. The selected-time parameter axes use the same configured limits.

### Instrument diagnostics

The ion viewer loads an optional `instruments_url` from `ion-config.json`. The
native-record traces share the parameter-map UTC window and selected-time cursor;
the selected interval readout uses overlap-duration weighted means. Missing
records/calibration remain gaps. This series is independent of fit resolution and
quality masking, allowing instrument changes to be compared with rejected fits.

`cargo run --release -p guisdap-worker --example export_instruments -- INPUT_FRAMES OUTPUT.h5 OUTPUT.json`
exports the recorded transmitter power (`d_parbl[7]`, watts), elevation (`[8]`) and
azimuth (`[9]`), plus ion-channel system noise temperature in kelvin. Zero-based
background addresses 0..16 and calibration-on addresses 16..32 have the same
1/512 normalization used by the fitter. With the recorded injected calibration
temperature `d_parbl[20]`, `Tsys = B * Tcal / (C - B)`. Nonpositive calibration
excess, negative background, or invalid injection temperature gives a missing
estimate. This uses means, matching the current fitter (legacy `scale_data.m`
uses medians). It estimates receiver-plus-sky noise, not electron temperature.
Absolute calibration uncertainty is not estimated. Internal data is retained in
HDF5; the static browser consumes the atomic JSON export. Export once per source
collection, and refresh when records are added.
