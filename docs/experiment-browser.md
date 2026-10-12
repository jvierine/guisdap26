# Experiment calendar and processing queue

Open `/guisdap/catalog.html`. The default list shows all inventoried experiment-days,
newest first, with 25/50/100 rows per page and oldest-first ordering available.
Search dates, experiment names or radars. All / Processed / Unprocessed filters
apply to both the list and Calendar view. Processed means at least one completed
product; partial-only entries remain in Unprocessed. Select an experiment from
the list, or switch to Calendar and select a UTC day. Ion-line products
are the default tab; Plasma-line is the other tab. Processing queue shows jobs,
the current stage and interval count, core allocation and calibration revision.
Existing products remain readable while new results are produced.

The Rust `catalog-store` service scans configured directories on avaruus. It groups
`YYYYMMDD_HH` directories beneath experiment names containing `@` by experiment
and day, combining duplicate archive locations. It stops at hour directories and
does not decompress recordings during inventory. Dates are directory metadata,
not a validation of the recording contents. Read errors and depth-limited paths
are reported; the inventory is partial while scanning. Symlinks are not followed.
Raw entries without a processing recipe remain selectable but cannot be processed.

Sources, the depth bound, the core budget and validated recipes are configured in
`config/catalog-avaruus.json`. The initial recipe is restricted to the studied
2022-11-02 BEATA 2.2 UHF recording: it must not be reused for another experiment
without verifying its layout, experiment response, calibration and atmosphere
inputs. Other archive experiments are inventory entries, not claims of decoder
support. Published RUISDAP products are associated through their recipes; this
version does not import arbitrary legacy GUISDAP analysis files into the viewer.

## Starting processing

In Compute settings, enter the server processing key, then select Process missing
products. The key is in `/home/j/data/guisdap/catalog-state/processing-key`, mode
0600. It is not included in static exports, Git, URLs or browser storage. Static
mirrors can display the catalogue and queue snapshots without the compute API.

The API binds to loopback port 8092, proxied by `scripts/apache-catalog.conf`.
`POST /guisdap/api/catalog/jobs/<experiment-id>` requires Bearer authentication.
The client supplies only the experiment identifier; sources, commands and output
paths come from the server's validated recipe. Duplicate queued/running requests
return the existing job. Completed products are skipped. One job runs at a time;
the BEATA recipe uses 128 workers and processes coarse gates before native gates,
60 s before 15 s. Plasma products already complete are reused.

Jobs and stage plans are saved under the private catalogue state directory.
The systemd user service `ruisdap-catalog.service` survives browser closure.
On service restart, a running job becomes interrupted rather than silently
successful; queued jobs remain queued. The service's control group contains the
workers, so stopping it stops its workers. Failed/interrupted jobs can be retried
through the same experiment action. An operator should inspect the job log first.

## Calibration

The recipe's `calibration_binding` points to a private file containing the saved
notebook path. Included points are fitted with the same Rust `fit_gain` used by
the plasma-line calibrator. Each queued job freezes the notebook bytes, SHA-256,
revision, included/total counts, absolute magic constant and its conditional
standard error. Configurations and outputs use a calibration-specific suffix;
previous results are retained. Later notebook edits do not modify the queued job.

Ion fitting uses the estimated absolute constant, not a multiplier of 0.7.
Both ion-line measurements and the raw-Ne proxy are normalized with this constant.
The fit configuration records calibration provenance in its product metadata;
the queue reports the snapshot. Calibration uncertainty is recorded separately
and is not silently added to the fitter's conditional parameter error bars.
Existing plasma-line spectra are reused; their amplitudes are diagnostic spectral
units. The plasma browser's old raw-Ne product is not rewritten by an ion rerun.

Each calibrated resolution becomes the default ion-viewer selection only after
its index exists. Original products remain available with `?original=1`.
The calendar links calibrated products through per-experiment configuration.

For a point with inferred constant C and old constant C0, the density correction
is C/C0. C>C0 means the old density is underestimated; C<C0 means overestimated.
This is stated directly in the calibrator. Refit densities may also change through
Debye-dependent model shape, so C/C0 is the calibration correction, not a guarantee
that every refitted density changes by exactly that ratio.
