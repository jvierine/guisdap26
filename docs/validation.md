# Initial BEATA validation

Validated on avaruus, 12 October 2026, Rust 1.99 and HDF5 1.14.

- 180 five-second integrations spanning 17:00–17:15 UTC on 2 November 2022;
  zero processing errors and no gaps in the selected sequence.
- Nine Rust tests pass: MAT complex layout/truncation, tone frequency sign,
  lag index and final-complex-lag preservation, band merging and overlap,
  geometry, unsupported layout, HDF5 round trip and conflict detection.
- Native and WASM builds succeed. Clippy passes with warnings denied.
- Independent NumPy/SciPy/HDF5 checks pass for the first, breakup-area and last
  integration: both stored complex lag blocks, explicit indices, signed FFT,
  broad spectra, signed zero-lag diagnostic, power, source hash and geometry.
- Browser checked against Apache over HTTP from the development computer:
  page, module, WASM, manifest, power history and per-frame lags load; playback
  advances; frame stepping keeps the spectrum and power timestamps identical;
  signed/unweighted controls work; no browser console warnings/errors observed.

This verifies implementation consistency, not physical calibration or a plasma-
line detection claim. Independent range-response and interference validation,
uncertainties, and a resolved low-altitude partial-pulse inversion remain open.

## Schema v4 checks

- Corrected the circular FFT length to 1600, retained all 800 measured lags, and
  tested that the real midpoint contributes exactly once.
- Tested strict separation of both short-lag blocks from the altitude spectrum.
- Synthetic delayed tone through the legacy C decoder and the actual 64-code
  pattern peaked at the expected gate 32 and +250 kHz (bin 960).
- Independently checked matched-channel ion calibration, r² conversion, signed
  raw Ne and range centroids against the numerical GUISDAP response model.
- The real-data altitude–frequency plasma result remains unvalidated; numerical
  implementation agreement alone is insufficient to call that problem solved.
