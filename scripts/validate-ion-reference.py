"""Independent ion power calibration QA, using the legacy numerical response model.
Run from repository root: python scripts/validate-ion-reference.py frames/*.h5
This is a check, not the production processor. Legacy model stays outside Git.
"""
import bz2, hashlib, io, json, sys
import h5py
import numpy as np
from scipy.io import loadmat
model_path='legacy/guisdap9/exps/beata/beataT_2init.mat'
assert hashlib.sha256(open(model_path,'rb').read()).hexdigest() == '27180ca657512d2bca042824431f688e8b3fa66980bd0916c77aab9c9391f4ec'
m=loadmat(model_path,simplify_cells=True)
lpg=int(np.flatnonzero(m['lpg_ra']==48)[0])
spacing=float(m['p_dtau'])*1e-6
c0=299792458.; scale=spacing*c0/2
ap=m['vcg_Ap'][0,0]
leff=m['lpg_wom'][lpg].real.max()/m['lpg_ND'][lpg]
r=(m['lpg_h'][lpg]+np.arange(432)*m['lpg_dt'][lpg])*scale
for path in sys.argv[1:]:
 with h5py.File(path) as h:
  meta=json.loads(bytes(h['metadata_json'][:]))
  d=loadmat(io.BytesIO(bz2.open(meta['source']).read()))
  raw=d['d_data'].ravel().real.astype(float);par=d['d_parbl'].ravel()
  # Independently reproduce radar_eq's monostatic reference-volume form.
  r0=m['p_R0']*scale; gain=m['ch_gain'][0]; wavelength=c0/m['ch_fradar'][0]
  volume=.460*.66*4*np.pi*r0*r0/gain*scale
  single=4*np.pi*(2.8179403262e-15)**2*(gain/(4*np.pi*r0*r0))**2*wavelength**2/(4*np.pi)
  coeff=single*volume/(1.380649e-23*ap/spacing)*(r0/r)**2*par[7]/.7
  b=raw[:16].mean()/32;cal=raw[16:32].mean()/32
  temp=(raw[48:480]/64-b)/(cal-b)*par[20]
  ne=2*temp/(coeff*leff)
  np.testing.assert_allclose(h['ion_raw_ne'][:],ne,rtol=3e-5,atol=1e3)
  np.testing.assert_allclose(h['ion_range_km'][:],r/1000,rtol=1e-12)
  assert np.all(h['ion_raw_ne_sigma'][:]>0)
  assert np.any(h['ion_raw_ne'][:]<0), 'negative estimates must survive export'
  print('PASS ion matched-channel raw Ne, signed values and model centroid:',meta['id'])
