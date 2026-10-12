# GUISDAP26

New Rust compute + Rust/WebAssembly/WebGL scientific GUI. Internal products are HDF5.
Legacy references, raw recordings, unpublished papers and products stay out of Git.
Do not add the ignored legacy directory as a submodule or vendor its sources.
First study: UHF BEATA 2.2, 2022-11-02, around 17:05 UTC.
Compute runs on avaruus. The GUI must run on a static web server with only exported products.
Preserve signed spectra and power; do not conceal negative estimates by clipping stored data.
Document frequency sign, range response, units, processing provenance and scientific limitations.
Use UTC. Never treat slant range as altitude. Fail on unsupported experiment layouts.
Test meaningful numerical and I/O invariants before publishing changes.
