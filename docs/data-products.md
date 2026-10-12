# Reading RUISDAP data products

Internal products are HDF5. Numerical analysis runs in float64; storage precision
is a separate choice. New ion-profile files store measurement arrays, fitted
parameters, standard errors and model curves as **float32**. Arrays of at least
256 values use shuffle and DEFLATE level 4; smaller arrays stay contiguous to
avoid compression metadata larger than the data. Plasma signal arrays already use compressed float32.

Float32 has about seven significant decimal digits (normal-value relative
rounding error at most about 6e-8). Signed values and NaNs are preserved. Values
that would overflow or underflow float32 cause an error. Absolute UTC Unix
seconds, coordinate arrays and parameter covariance remain float64. Do not cast
Unix seconds to float32: that loses the timing of these integrations.

The full-precision JSON restart record is retained inside HDF5 and compressed
losslessly. This preserves exact fit restart/provenance information even when
scientific arrays are stored at reduced precision. Existing browser JSON exports
remain readable JSON; they have no binary float32 dtype and are not changed by
HDF5 repacking. Do not assume all old files have the new storage dtype.

## Ion-line layout

A resolution directory contains `index.h5` and `profiles/*.h5`. Each profile is
one integration. `index.h5/manifest_json` is UTF-8 JSON stored as uint8 bytes:
`config` includes integration duration, atmosphere, calibration constant
(`calibration_multiplier` is its historical field name), and calibration snapshot
provenance when available. `profiles` contains compact time/altitude summaries.

Each profile file contains:

| Dataset | Shape / dtype | Meaning |
| --- | --- | --- |
| `profile/time_bounds_unix_s` | `[2]`, float64 | Start/end UTC Unix seconds |
| `profile/altitude_km` | `[gate]`, float64 | Altitude, not slant range |
| `profile/parameters_ne_ti_te_v_nu` | `[gate,5]`, float32 | Ne [m^-3], Ti [K], Te [K], spectral velocity [m/s], prescribed collision rate [s^-1] |
| `profile/standard_error` | `[gate,5]`, float32 | Conditional 1-sigma errors, same column order; NaN when unavailable |
| `profile/raw_ne_and_sigma` | `[raw_gate,2]`, float32 | Signed 10-us-lag equivalent raw Ne and sigma [m^-3]; see profile JSON for context |
| `profile/gate_NNN/lag_us` | `[measurement]`, float64 | Measured lag coordinates; repeats are distinct response groups |
| `profile/gate_NNN/observed`, `predicted`, `sigma` | `[measurement,2]`, float32 | Real, imaginary components [m^-3], response-normalized scattering strength |
| `profile/gate_NNN/standardized_residual` | `[measurement,2]`, float32 | Dimensionless residuals |
| `profile/gate_NNN/model_curve` | `[sample,5]`, float32 | lag [s], real, imaginary, sigma-real, sigma-imag [m^-3] |
| `profile/gate_NNN/parameter_covariance` | `[free,free]`, float64 | Conditional covariance; parameter order given by `free_parameter_indices` |
| `profile/gate_NNN/altitude_bin_km` | `[2]`, float64 | Lower/upper altitude bounds |
| `profile/gate_NNN/status_utf8` | bytes | Fit status; inspect before interpreting parameters |
| `profile/gate_NNN/source_address_offsets`, `source_addresses_zero_based` | uint32 | Ragged source-address mapping, offsets delimit each row |
| `profile_json` | compressed UTF-8 bytes | Full-precision profile, source records, model/fit diagnostics |
| `cache_key` | UTF-8 bytes | Processing identity used for resumability |
| `model_frequency_hz` | float64 | Model frequency coordinates |

Absent fit-specific datasets mean no corresponding fit result. NaN is missing,
not zero. Negative measured powers are retained. A converged status alone does
not guarantee a scientifically acceptable fit. Calibration and fixed-composition
systematics are not included in the conditional errors. The raw-Ne proxy is not
a fitted density or a direct narrow-gate zero-lag measurement.

## Minimal reader

Run the Rust example from the repository:

```sh
cargo run -p guisdap-worker --example read_ion_product -- /path/to/profiles/interval.h5
```

For other tooling, no RUISDAP runtime is needed; any HDF5 reader works. For example:

```python
import h5py, json
with h5py.File("interval.h5", "r") as f:
    time_bounds = f["profile/time_bounds_unix_s"][:]
    altitude_km = f["profile/altitude_km"][:]
    ne, ti, te, velocity, collision = f["profile/parameters_ne_ti_te_v_nu"][:].T
    ne_sigma = f["profile/standard_error"][:, 0]
    diagnostics = json.loads(f["profile_json"][:].tobytes())
```

## Compacting completed ion products

```sh
guisdap-worker compact-ion --input /path/to/completed-resolution
```

This requires `index.h5`, writes replacement profiles, verifies unchanged
full-precision restart records and timestamps, then replaces each file atomically.
It does not refit or change calibration, and does not rewrite browser exports.
Run it on completed, idle resolution directories, not while fitting into them.
