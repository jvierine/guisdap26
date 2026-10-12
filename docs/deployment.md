# Compute and static hosting

Authoritative development checkout: `avaruus.uit.no:/home/j/src/guisdap26`.
Compute products: `/home/j/data/guisdap/2022-11-02-v4/frames/*.h5`.
Static export: `/home/j/src/guisdap26/web/data/2022-11-02-v4/`.

The event copy currently used is under
`/nfs/revontuli/data/oliver/02112022/beata_cp1_2.2u_AA@uhf/20221102_17`.
The archive root supplied for future discovery is `/urdr/data/beata` on revontuli;
on avaruus the same urdr mount is `/nfs/urdr/data/beata`. That archive does not
contain this event at the exact `beata_cp1_2.2u_AA@uhf/20221102_17` path on the
inspection date. Do not substitute another experiment version silently.

## Avaruus

Apache serves `http://avaruus.uit.no/guisdap/` directly from `/srv/guisdap/`.
It is enabled at boot. Only GUI assets and exported products are deployed there;
there is no compute endpoint. `scripts/apache-guisdap.conf` defines the alias,
disables directory listing, and sets the WebAssembly MIME type. The legacy
user service on port 8088 remains useful for development previews.

```sh
# One-time administrator setup after installing apache2:
sudo install -d -o j -g j -m 755 /srv/guisdap
sudo install -m 644 scripts/apache-guisdap.conf /etc/apache2/conf-available/guisdap.conf
sudo a2enmod headers
sudo a2enconf guisdap
sudo apache2ctl configtest
sudo systemctl reload apache2
# If UFW is active with default-deny incoming:
sudo ufw allow 80/tcp comment "GUISDAP Apache HTTP"
# Build and publish from the avaruus checkout:
scripts/build-web.sh
scripts/deploy-web.sh
```

Development preview over SSH:
`ssh -N -L 18088:127.0.0.1:8088 avaruus.uit.no`, then open
`http://127.0.0.1:18088/guisdap/`. Production Apache does not depend on the tunnel
or on the Rust preview server. TLS is not configured by this initial setup.

## Thin host, e.g. juha.no

Copy only `index.html`, `style.css`, `config.json` and generated `pkg/` into the
GUI document directory, e.g. `/var/www/guisdap/`.
Export processed data into `/mnt/shovel/guisdap/YYYY-MM-DD/` and serve that root
through a read-only static alias. Set `config.json` to:

```json
{"data_root":"/guisdap/data/2022-11-02-v4/"}
```

For Apache, adapt `scripts/apache-guisdap.conf` and add a more specific `/guisdap/data/` Alias to `/mnt/shovel/guisdap/` before the GUI alias, with a matching read-only Directory block. An nginx alternative is `scripts/nginx-thin.conf`.
No compute connection is required. If GUI and data use different origins, the
product server must allow the GUI origin through CORS. Avoid public directory
listing. HDF5 files remain the internal canonical products; the static export is
little-endian float32 arrays and a JSON manifest.

Export into a staging day directory and switch it into place after completion.
Within an export the JSON index is written last. Existing date exports should not
be rewritten while clients are reading them; publish a new versioned directory
when reprocessing, then change `config.json` atomically.
