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

# Independently expand each numerical lag-profile group; do not use Rust geometry
# or normalization constants here. Verify both partial and fully decoded gates.
short_groups=np.flatnonzero((m['lpg_ra']>=480)&(m['lpg_ra']<911))
rr=[];nd=[];ll=[];ww=[]
for group in short_groups:
 assert m['lpg_bac'][group]==0 and np.isclose(m['lpg_lag'][group]*spacing,10e-6,rtol=1e-12)
 count=int(m['lpg_nt'][group])
 rr.extend((m['lpg_h'][group]+np.arange(count)*m['lpg_dt'][group])*scale)
 nd.extend([m['lpg_ND'][group]]*count)
 ll.extend([m['lpg_wom'][group].real.max()/m['lpg_ND'][group]]*count)
 ww.extend([m['lpg_w'][group]*scale/1000]*count)
rr=np.array(rr);nd=np.array(nd);ll=np.array(ll)
assert len(rr)==431
for path in sys.argv[1:]:
 with h5py.File(path) as h:
  meta=json.loads(bytes(h['metadata_json'][:]))
  d=loadmat(io.BytesIO(bz2.open(meta['source']).read()))
  raw=d['d_data'].ravel().astype(complex);par=d['d_parbl'].ravel()
  slices=raw[25531:31565].reshape(14,431)
  np.testing.assert_allclose(slices.sum(axis=0),raw[480:911],rtol=2e-6,atol=2)
  coeff=single*volume/(1.380649e-23*ap/spacing)*(r0/rr)**2*par[7]/.7
  b=raw[:16].real.mean()/32;cal=raw[16:32].real.mean()/32
  factor=2*par[20]/(coeff*ll*nd)
  ne=factor*raw[480:911].real/(cal-b)
  rho=m['vcg_Ap'][np.array([25,50,75]),0]/ap
  nref=par[63]/2*16/(1+2*np.sum((1-np.arange(1,4)/16)*rho**2))
  vargain=(b*b+cal*cal)/(nref+1)
  vsum=14*slices.real.var(axis=0,ddof=1)
  se=factor*np.sqrt(vsum/(cal-b)**2+raw[480:911].real**2*vargain/(cal-b)**4)
  np.testing.assert_allclose(h['ion_short_raw_ne'][:],ne,rtol=4e-5,atol=1e3)
  np.testing.assert_allclose(h['ion_short_raw_ne_sigma'][:],se,rtol=4e-5,atol=1e3)
  np.testing.assert_allclose(h['ion_short_range_km'][:],rr/1000,rtol=1e-12)
  np.testing.assert_allclose(h['ion_short_response_width_km'][:],ww,rtol=1e-12)
  np.testing.assert_allclose(h['ion_short_acf'][:,0],raw[480:911].real/par[21],rtol=1e-6)
  np.testing.assert_allclose(h['ion_short_slices'][:,:,1],slices.imag/par[21],rtol=1e-6)
  heights=h['ion_short_altitude_km'][:]
  assert heights[0]<48 and heights[0]>47
  assert np.any(ne[:60]<0), 'low-gate negatives must survive'
  print('PASS all 431 short-lag gates, sum identity, model normalization/range, signed Ne and sample uncertainty:',meta['id'], 'first altitude',heights[0])
