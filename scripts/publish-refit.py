"""Publish fit product pointers only; all scientific computation is Rust."""
import json, os, sys
from pathlib import Path
kind, cadence, dataset, phase = sys.argv[1:]
path = Path("web/ion-config.json")
cfg = json.loads(path.read_text())
url = f"data/{dataset}/index.json"
label = ("1 minute" if cadence == "60" else "15 seconds")
if kind == "native":
    label += " · native gates below 250 km"
existing = next((d for d in cfg["datasets"] if d["data_url"] == url), None)
if phase == "running":
    if existing is None:
        cfg["datasets"].append(dict(label=label + " · refitting", integration_s=int(cadence), data_url=url))
else:
    cfg["datasets"] = [d for d in cfg["datasets"] if d["data_url"] != url]
    for d in cfg["datasets"]:
        if d["label"] == label:
            d["data_url"] = url
for out in [path, Path("/srv/guisdap/ion-config.json")]:
    temp = out.with_suffix(".json.tmp")
    temp.write_text(json.dumps(cfg, indent=2)); os.replace(temp, out)
if phase == "complete" and kind == "full" and cadence == "60":
    path = Path("web/calibration-config.json")
    cfg = json.loads(path.read_text()); cfg["ion_index_url"] = url
    for out in [path, Path("/srv/guisdap/calibration-config.json")]:
        temp = out.with_suffix(".json.tmp")
        temp.write_text(json.dumps(cfg, indent=2)); os.replace(temp, out)
