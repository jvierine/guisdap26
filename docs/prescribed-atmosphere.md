# Prescribed atmosphere and composition

Production BEATA fits prescribe collision frequency; it is absent from the optimizer vector, covariance and fitted uncertainty. All four `ion-fit-msis-iri-*` configurations use 128 threads. `scripts/refit-msis-iri.sh` runs coarse 60 s, coarse 15 s, native 60 s, native 15 s. New products use `msis-iri-v5`; original products remain available. Never resume products generated with different physics.

## MSIS

Pure Rust [jvierine/rmsis](https://github.com/jvierine/rmsis), pinned revision fc707387617885de749dcc516c306a74227cf1b5, default NRLMSISE-00 backend. The optional Fortran MSIS 2.1 backend is not used. SI neutral densities and local temperature are computed at each integration midpoint and gate-center altitude, at transmitter latitude 69.6 N, longitude 19.2 E (the legacy IRI site coordinates). This currently neglects the small horizontal displacement along the tilted beam.

Archived IRI apf107.dat inputs for 2022-11-02: previous-day F10.7=126.1, centered 81-day F10.7=129.5, daily Ap=13. F10.7 values in this archive are adjusted to 1 AU. The Rust model converts the daily and 81-day drivers to observed flux using the same Earth–Sun distance approximation as IRI-2020; both converted values and distance are stored per gate. The IRI 365-day composition driver stays adjusted. Daily Ap mode is explicit; there is no automatic network lookup or quiet-day fallback. Unsupported dates fail. Original file SHA256: 9f269007a62542d4930dd8ea5601a0322ec84f3c160f52bb95eea00a63a938d5.

The prescribed effective molecular-ion momentum-transfer rate follows the existing IRI/GUISDAP prescription:

`nu = 1.63e-16 n(O) + 2.99e-16 n(N2) + 4.28e-16 n(O2)` s^-1, for number densities in m^-3.

No 1/s floor is imposed. The current two-ion spectral model shares this effective rate between species. This is an approximation, not a species-resolved O+ resonant charge-exchange treatment. Atmospheric/model uncertainty is not included in the conditional fitted-parameter covariance.

## IRI molecular fraction

Rust implementation of the selectable **IRI DS-1995** composition option: Danilov and Smirnova, *Improving the 75 to 300 km ion composition model of the IRI*, Advances in Space Research 15(2), 171–177 (1995). Coefficient tables and reference calculation are IRI-2020 IONCO2/APROK. This is a port of that composition component, not a claim to implement all IRI outputs or the newer RBV/FLIP chemistry option.

Inputs are seasonal hemisphere-adjusted month, solar zenith and archived 365-day mean F10.7=141.4. Solar zenith uses a fractional-year declination/equation-of-time approximation and DS-1995's native 20–90 degree clamp. The percent rounding in IRI is retained. The Rust implementation is compared to 36,432 original double-precision Fortran reference cases across all months, 75–300 km, solar zenith 10–110 degrees and solar flux 60–160. The original Fortran is used only to generate test fixtures; production analysis is Rust.

The molecular fraction is NO+ + O2+ + cluster fraction, represented by the existing effective mass 30.5 amu. The remaining fraction is represented as O+, retaining the two-ion ISR approximation; H+, He+ and N+ are not separately fitted. Above 300 km IRI's DS/DY path sets molecular abundance to zero. Below 75 km the boundary composition is held and explicitly flagged as outside the model domain; cluster-ion masses are not represented. DS-1995 is a climatological empirical option and does not solve auroral production chemistry.

Every gate stores neutral densities, neutral temperature, modeled collision rate, molecular fractions, model identities, timestamp and altitude. The index records all external drivers and their provenance. Validation checks identical measurement grids, the expected degree-of-freedom change, fixed collision rates and composition consistency. Residuals may increase relative to the former free-collision/logistic-composition fit because its physics and number of free parameters differ.

IRI source SHA256: e72b7f5adf0ea7f37ddf1c6d063ad783981733e22406ead196ee70a4e9f1119b. Reference fixture SHA256: 13a2e1bbb2c68efba62bbbd1b1b4c0e4d17c43782eb147da5529915edf315eef. Fixture generator uses the original IONCO2/APROK with gfortran `-fdefault-real-8 -ffixed-line-length-none`.
