# GUISDAP26

Rust compute and a Rust/WebAssembly/WebGL2 viewer for incoherent-scatter radar data.
The first vertical slice processes Tromsø UHF **BEATA 2.2, 2022-11-02,
17:00–17:15 UTC**: two plasma-line bands and ion-line short-lag power versus altitude.

This is a research quick-look implementation, not yet a replacement for full
GUISDAP inference. It displays signed 10 µs lag raw-Ne equivalents down to about 47.5 km, with approximate statistical error bars;
A separate Rust ion-line fitter now estimates density, temperatures, spectral velocity,
and an effective collision rate, with explicit composition assumptions and fit flags.
See [ion fitting](docs/ion-fitting.md) and the live [fit inspector](http://avaruus.uit.no/guisdap/ion.html).
The equations, range responses and limitations are in [docs/science.md](docs/science.md).

## Working environment

Development and compute run directly on `avaruus.uit.no:~/src/guisdap26`.
The private legacy reference checkout lives under `legacy/guisdap9`, ignored by Git.
New source lives in this repository; raw recordings, papers, HDF5 outputs and compiled
web assets do not. See [docs/legacy.md](docs/legacy.md) for the pinned upstream.

## Build and analyze

Requires Rust, a C compiler, pkg-config, and HDF5 development headers.
On Debian/Ubuntu the native packages are `build-essential pkg-config libhdf5-dev`.
The maintained `hdf5-metno` Rust binding supports avaruus's HDF5 1.14 installation.

```sh
cargo build --release -p guisdap-worker
cargo test --workspace
target/release/guisdap-worker analyze --config config/avaruus.json
target/release/guisdap-worker export \
  --input ~/data/guisdap/2022-11-02-v5 \
  --output web/data/2022-11-02-v5
```

`inspect <file.mat.bz2>` prints the observed identity, timing, tuning and dimensions.
Input support currently covers the little-endian MATLAB level-4 layout actually
observed in this BEATA 2.2 recording. Unknown layouts fail explicitly.

For multiple disks, list each experiment/hour directory in `roots`. Avoid recursively
scanning the whole archive for one event. Exact duplicate recordings are deduplicated
by decompressed source SHA-256. Different recordings at one timestamp are rejected
by the exporter rather than silently combined.

For multiple nodes, run the same command with `--node-index I --node-count N` on
each node, with `I` from 0 through N−1 and the same root inventory visible to all nodes.
Each node uses its configured worker threads. Sharding is stable by filename, and
HDF5 publication uses atomic no-clobber files. Use a shared output filesystem, or
merge the per-node `frames/` directories before exporting. All workers must finish
successfully before export. This is batch partitioning; there is no scheduler,
automatic node discovery, retry coordinator or live job API yet.

## Browser

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
scripts/build-web.sh
target/release/guisdap-worker serve --root web --bind 127.0.0.1:8088
```

Open `http://127.0.0.1:8088/guisdap/`. A trailing slash matters for relative assets.
The browser needs WebGL2. The only handwritten JavaScript is module startup;
selection, fetching, scaling, profile generation and WebGL rendering are Rust.
No TypeScript or frontend package manager is required.

The viewer includes raw/temporal-background-subtracted broad-gate spectrograms,
a time selector and animation controls, a merged altitude–frequency view, and a
short-lag raw-Ne time–altitude view with a synchronized signed profile (fixed 10⁸–10¹³ m⁻³ limits).
Browser color limits are diagnostic stretches, not calibration; the animation
uses a fixed scale. All 800 measured long lags enter the direct FFT.

For static hosting at `juha.no/guisdap`, copy `web/` assets and put the exported
products at `/mnt/shovel/guisdap/YYYY-MM-DD`. Only change `web/config.json` and the
web-server aliases. No HDF5 library, Rust runtime or compute service is needed there.
See [docs/deployment.md](docs/deployment.md).

## Layout

- `crates/guisdap-ion`: stateless Maxwellian/BGK physics, nonlinear fitting, covariance.
- `crates/guisdap-core`: numerical transforms and geometry, independent of I/O.
- `crates/guisdap-worker`: native MAT reader, batch workers, HDF5 and static export.
- `crates/guisdap-web`: Rust WASM application and WebGL2 heatmaps.
- `web`: HTML/CSS shell and dataset location configuration.
- `config`: first-event configuration and multiple-root example.
- `scripts`: builds and deployment templates.

Internal products retain source hashes, original parameter blocks, UTC integration
bounds, antenna pointing, tuning, signed spectra, signed lag-zero estimates, raw
power, noise references and explicit slant-range/altitude coordinates. Public web
exports omit raw recording paths and contain only selected derived products.

## Next scientific work

Validate the quick-look frequency/range response against a complete GUISDAP analysis;
add calibration and uncertainty propagation; assess interference and transmitter
artifacts; then implement plasma-line detection and fitting with explicit quality
flags. The raw zero-lag pulse response is approximately 96 km in slant range and must
not be mistaken for the 3 km decoded range response. The selected plasma spectra have five-second integrations. The recording also
contains fourteen ion short-lag slices per integration; v5 preserves these in HDF5 and uses their scatter for the approximate uncertainty of the five-second profile.


The current browser merges both receivers, animates integrations, and aligns
plasma and ion-line histories on a shared UTC axis. Direct FFT, optional positive
lag weighting and frequency averaging run in Rust/WASM. Native lag blocks and
indices remain in HDF5; missing lags are never spline filled. See
[the scientific contract](docs/science.md) for missing-zero-lag and partial-pulse
limitations. The historical plasma-line reference was found in
[Juha's ipypl](https://github.com/jvierine/ipypl), `beatapl.py`.

On avaruus, Apache serves the deployed viewer at http://avaruus.uit.no/guisdap/.

The optional GitHub Actions check definition is provided as
`scripts/github-actions-check.yml`. It is a template, not an enabled workflow;
the initial publishing credential lacks the GitHub workflow scope. The Rust tests,
Clippy checks and WASM build are run on avaruus.

For ion fitting, run `target/release/guisdap-worker fit-ion --config config/ion-fit-avaruus.json`.
The fit inspector is `ion.html`; its static-product location is in `ion-config.json`.
All new fitting and validation code is Rust. HDF5 preserves the full numerical
results and provenance, and a static JSON export supplies the Rust/WASM viewer.


## Full-study processing and live browsing

Run both ion-line fitting and plasma-line processing on avaruus:

```sh
cargo build --release -p guisdap-worker
target/release/guisdap-worker run-study --config config/study-avaruus.json
```

Edit `data_sources` (one or more mounted directories), `worker_threads`, UTC
`start`/`end`, `work_root`, `publication_root`, and `integrations_s` in that config.
The current study uses 128 compute threads and produces both 15 s and 60 s products.
Stages share this capacity sequentially; parallel fit windows and gates use one
Rayon pool. Restarting reuses validated input and interval caches.

The full 2 November 2022 input contains 2,869 native records, approximately
15:00:35–19:00 UTC: 958 nominal 15 s intervals and 240 one-minute intervals.
Five 15 s intervals have only two records and are flagged insufficient for fitting.
Partial plasma averages retain their actual source count. Processing completion
is distinct from fit convergence or validation of plasma-line decoding.

`ion.html` provides resolution switching, shared time zoom across five stacked
parameter/raw-Ne rows, altitude limits, and on-demand fit diagnostics. Its
Processing status tab shows both cadences and the plasma-line stages. Completed
ion intervals publish atomically during fitting; the viewer polls for updates
without resetting the selected time or zoom. Plasma products publish after each
cadence completes, with immutable overview files. Existing products remain usable
while analysis runs. Both viewers work on a static server with exported products;
no compute service is required by the browser.
