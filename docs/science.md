# BEATA 2.2 processing contract

## Observation and scope

The files identify `beata_cp1_2.2u_AA`, UHF site code 4, experiment version 2.2.
`d_data` contains 196493 complex samples; `d_parbl` contains 67 parameters.
The selected interval is 2022-11-02 17:00–17:15 UTC, selected by integration
midpoint. Products preserve actual start/end times including fractional seconds.
The observed integrations are five seconds. Pointing is read for each integration.

This is a relative spectral-contrast quick look using a direct FFT of recorded lags. It is not the full GUISDAP
range-ambiguity inverse problem or a fit of ISR plasma parameters. No density,
thermal correction, uncertainty, significance or physical Doppler direction is
inferred. Narrow features require further discrimination from interference.

## Memory layout

Addresses below are zero-based in the MATLAB column-major vector.

| Quantity | Offset | Shape / ordering |
|---|---:|---|
| Ion-channel power | 48 | 432 range samples |
| Ion-channel noise | 31565 | 16 background samples |
| Plasma noise | 32029 + b×82232 | 16 samples |
| Decoded short-lag block | 32061 + b×82232 | 50 lags × 91 gates |
| Decoded long-lag block | 36611 + b×82232 | lags 1…800 × 90 gates |
| Undecoded plasma ACF | 108611 + b×82232 | 50 lags × 4 broad gates |
| Plasma-channel power | 108811 + b×82232 | 5450 samples |

Here `b` is 0 or 1. Gates vary fastest. The two receiver offsets come from
`d_parbl(47:48)` in MATLAB indexing: −2.5 and −4.9 MHz. Lag spacing is 0.4 μs.
The sample metadata are validated; older three-band layouts are not accepted.
Accumulated values are divided by `d_parbl(22)` (1 in the inspected records).

## Undecoded overview

The four approximate slant-range supports are 47–221, 128–302, 209–383 and
290–464 km, following the BEATA definition. They overlap strongly.
For each 50-element ACF form a 100-element Hermitian vector with a zero at the
unmeasured terminal lag, then take the real FFT and fftshift. No window or dt
scaling is applied, matching the raw-spectrum convention in `plasma_summary`.
Frequency is receiver offset plus signed baseband FFT frequency; spacing is 25 kHz.

The web export contains both raw spectra and their per-frequency temporal median
residual over the exported interval. This simple background subtraction differs
from GUISDAP's more elaborate spectral-shape/interference removal. It can suppress
persistent lines and is not a statistical detection test.

## Direct FFT of the native long-lag block

The acquisition already decoded these ACF accumulators. All 800 long lags are
retained at their measured indices 1…800; no lag selection, spline filling,
truncated SVD, or lag-zero extrapolation is performed. All 50 × 91 complex
short-block estimates are retained separately, without treating their distinct
range response as interchangeable with the long block. Both blocks are stored
as normalized float32 real/imaginary pairs with explicit lag-index datasets.
The raw source SHA-256 and normalization retain processing provenance.

For each range gate use the 1600-point circular Hermitian ACF implied by
`plwin.c`. Recorded R(1)…R(799) have conjugate partners; the recorded R(800)
is the self-conjugate midpoint and contributes once. Its tiny recorded imaginary
residual is retained in `native_long_acf`; the real-spectrum transform uses its
real component. Only R(0) is absent, represented by zero contribution, never an
interpolated measurement. The signed FFT therefore has an unknown constant power
offset. The previous 1602-point construction incorrectly doubled the midpoint
and introduced an unnecessary endpoint; schema v4 corrects this.

The optional positive weight is `sqrt((640 − 0.4 (l−1))/640)`; uniform weighting
is selectable. Juha's historical `ipypl/beatapl.py` supplied the diagnostic
weighting, but its lag-origin convention is not used here. No measured lag is
rejected and no spline or SVD is used.

The four uncoded broad-gate ACFs are saved independently as
`native_uncoded_acf` (2×50×4×2). A regression test changes both short blocks
while requiring the altitude-spectrum output to remain exactly unchanged.

The browser computes the FFT in Rust/WASM. Optional boxcar frequency averaging
uses all measured lags and averages only available spectral bins at the edges.
Default detection display: absolute signed contrast, four-bin averaging, then
division by the median across 90 gates separately for each receiver/frequency.
This removes sign information only in that selected display; signed spectra
and native complex lags remain available. It is not a calibrated power or
statistical significance estimator and may suppress range-filling features.
The default magnitude color limit is fixed at six times the range median;
signed mode locks its initial percentile limit until the processing controls
change. No frame-by-frame color autoscaling is used during playback.

Receiver spectra are mapped onto an increasing common frequency grid. Linear
interpolation here resamples **frequency bins only**, never missing ACF lags;
the approximately 100 kHz receiver overlap is averaged equally. Receiver
frequency grids align at 1562.5 Hz spacing for this 1600-point transform. Their gains are not
cross-calibrated, so raw signed amplitudes can show a receiver seam.
The broad overview grids align exactly at 25 kHz spacing. Both histories have
identical time columns and plot widths; spectrum and ion power commit to a new
integration together after loading. Playback loops without skipping frames.

The decoded range coordinate follows the observed RTG timing:
`r = c/2 × (955 − 240 − 20.4 + 20 g) μs`, g = 0…89.
The lower 12 gates contain partial-code tails; the remaining 78 have the fully
illuminated code geometry. Range-gate spacing is approximately 3 km. This RTG
coordinate is a nominal quick-look coordinate, not a fitted GUISDAP ambiguity
centroid. No claim of calibrated absolute power is made.

## Zero-lag echo power in raw Ne

Use the ion receiver's signal addresses 48…479, its own background 0…15,
and calibration 16…31. The previous quick look incorrectly paired signal
channel 1 with channel 2's background at 31565. Channel 2 has different tuning
and is not substituted for the ion-line receiver. `beata-u.fil`,
`beataT_2_LP.m` and `scale_data.m` establish the signal/background accumulation
ratio 64/32. Normalize background and calibration to signal accumulation:
B=2 mean(background), C=2 mean(calibration). The signed temperature estimate is
`T = Tcal (P−B)/(C−B)`, with Tcal from parameter 21 (MATLAB indexing).
Use means and retain negative values. No positivity constraint, absolute value,
threshold rejection, or smoothing is applied to the stored estimate.

The numerical receiver response comes from `beataT_2init.mat`, SHA-256
`27180ca657512d2bca042824431f688e8b3fa66980bd0916c77aab9c9391f4ec`:
Ap(0)=0.18370696050680618, effective length=7851.068411222712 in p_dtau units,
p_dtau=0.4 μs, antenna gain=10^4.81, frequency=930 MHz. Follow GUISDAP's
monostatic `radar_eq.m` and `power_prof.m`: beam factor 0.460, antenna efficiency
0.66, measured transmitter power, and Magic_const=0.7 as recorded in the existing
analysis of this event. Raw Ne is the conventional 2×scattering-strength estimate,
assuming Te/Ti=1 and omitting the Debye correction; it is not a fitted true density.
The conversion contains **r²**, undoing geometric 1/r² spreading. It is a fixed
linear conversion conditional on the instrument calibration. Uncertain calibration,
finite-sample gain ratios, filtering, and the raw-Ne temperature convention prevent
a claim of universally unbiased physical electron density. No positivity bias is
introduced by the new estimator.

Range uses the response centroid from the numerical model:
`r = (1548×0.4 + 10 i) μs × c/2`, i=0…431, not the lower response edge.
Its equivalent slant-range width is 95.90584644 km. Each point is a broad
response-weighted echo-power estimate. It must not be interpreted as independently
resolved electron density every 1.5 km. The separate short-lag products can reach
lower altitudes, but converting those nonzero lags into zero-lag power requires
a validated spectral response model.

`ion_raw_ne_sigma` is an approximate statistical standard error. Following the
approach in GUISDAP `var_prof.m`, estimate signal variance from six neighbouring
range-normalized samples. Correct finite-sample degrees of freedom and receiver
covariance using |Ap(10,20,30 μs)/Ap(0)|². Real range gradients can inflate this
local-scatter estimate; it is not a full covariance inversion. Background and
calibration errors use the 16 reference samples and half of the pulse count each,
including receiver covariance. Propagate all three through the calibration ratio,
including B appearing in both numerator and denominator. Systematic antenna,
transmitter and Magic_const errors and uncertainty of the range response are not
included in these bars. Ne and sigma are stored in m⁻³.

The displayed short-lag profile uses fixed logarithmic bounds 10⁸–10¹³ m⁻³ and horizontal ±1σ bars.
Nonpositive estimates are retained and explicitly counted/marked at the axis edge;
they are never replaced by a positive floor in the products. The temporal image
uses the same fixed logarithmic limits and the same integration as the plasma animation; nonpositive estimates are dark, with no change to the stored values.

Altitude uses a spherical Earth radius 6370 km, site altitude 0.030 km, and
measured elevation. This is an explicit spherical approximation, not WGS84 ray tracing.

## Validation and limitations

Unit tests check tone frequency sign and the zero-lag/FFT integral relation,
geometry, overlap weights and corrupt/truncated MAT inputs. An independent
NumPy/SciPy calculation on an actual recording checks the raw broad spectra and
the native circular-FFT spectra against the Rust output (float32 storage tolerance).
Synthetic and numerical tests do not establish physical calibration. End-to-end
comparison with a complete GUISDAP run remains future work.

The source documentation is recorded in [legacy.md](legacy.md). No unpublished
manuscript or legacy source is redistributed with this repository.

## Partial-pulse lower boundary

Schema v5 displays the **measured 10 µs ion lag**, addresses 480…910,
rather than the broad zero-lag profile. All 431 gates are retained. The numerical
`beataT_2init.mat` response model above gives, for zero-based gate i:

- centroid `(810.5×0.4 + 10 i) µs × c/2`, first slant range 48.596 km;
- first altitude about **47.5 km** at the measured elevation;
- equivalent response width 2.1724 km in slant range;
- accumulation normalization ND = `64 × min(1+floor(i/2),31)`;
- peak real spectral response divided by ND = 156.249999713498.

The first 60 samples are partial-code tails; there is no altitude cutoff at the
fully decoded boundary. The model maps every group to calibration 16…31 and
**no background subtraction** (`lpg_bac=0`) for these decoded nonzero lags.
The raw-Ne conversion follows `power_prof.m`: divide by ND, calibrate in Kelvin,
apply the monostatic radar coefficient including r², divide by the peak spectral
response and multiply by two. It is a signed linear estimate conditional on the
calibration. No absolute value, positive fit, lag interpolation or data-dependent
weighting is used. Finite-lag decorrelation and Doppler can differ from zero-lag
power; the GUI calls this a short-lag raw-Ne proxy, not an exact zero-lag estimate.

Addresses 25531…31564 hold fourteen subintegrations of this same 431-gate lag.
Their sum matches the five-second aggregate (checked against recordings).
Estimate variance of the sum as `14 × sample_variance(slices)` and propagate
calibration-gain uncertainty through the same conversion. This avoids treating
spatial gradients as random noise, but is **approximate**: the online clutter
filter can correlate subintegrations, and its missing parameter file prevents
an exact covariance correction. Bars exclude that covariance and absolute
calibration, spectral-model and response errors. No claim of fully unbiased
physical density or exact uncertainty is made.

HDF5 preserves complex aggregate and slice lags, signed `ion_short_raw_ne`, its
sigma, centroid coordinates, response width, and the original broad zero-lag
products separately. GUI time histories and profiles now use these short-lag
products; fixed log limits are 10⁸…10¹³ m⁻³. Python validation independently
expands every lag-profile group in the original numerical model and checks
geometry, normalization, calibration, signs, slice sums and standard errors.

## Unresolved plasma-line validation

The layout matches the archived `.fil` and RTG definitions. The installed
revontuli and cloned `plwin.c` are byte-identical. A delayed coded tone replayed
through that C decoder recovers the expected range gate and frequency with the
1600-point transform. All 64 transmitted code patterns in the 17:05:30 pulse
monitor agree with the legacy experiment model (up to each pulse's overall phase).
These checks do **not** yet explain the noisy real-data altitude–frequency panel.
Uncoded spectra contain clear evolving features, while the decoded FFT has not
passed a corresponding real-data feature-recovery check. Treat the decoded panel
as an unvalidated diagnostic, not a plasma-line measurement. The original
`beata_up.par` is still missing from the archived experiment information.

## Plot conventions

Both receivers are merged in each of the four undecoded history panels. Panels
are ordered from highest to lowest altitude, using the measured elevation to
convert the approximate slant-range supports. These are overlapping supports,
not four sharply resolved altitude bins. A sequential plasma palette is shared
across all panels. Numerical color limits are common to the four gates and remain
fixed during playback. Signed residuals retain their sign and are displayed on
a monotonic asinh scale; changing a palette never changes the stored estimator.
