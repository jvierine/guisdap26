"""Independent NumPy reference for actual BEATA records; QA only, not compute.
Usage: python scripts/validate-reference.py /path/to/frames/*.h5
Requires numpy, scipy, h5py. Source paths come from the private HDF5 provenance.
"""
import bz2
import hashlib
import json
import sys
import h5py
import numpy as np
from scipy.io import loadmat

def weights():
    f, bits, ml = 50, 32, 800
    w = np.r_[1/(6*f), np.arange(1, f)/(f-1/3), 1]
    wave = np.tile(np.r_[w[1:], w[-2:0:-1], 0], 8)
    tri = np.r_[np.arange(1, f), np.arange(f, -1, -1)]
    a = np.outer(tri, np.arange(bits-1, -1, -2)).ravel(order='F')
    b = np.outer(tri, np.arange(bits-2, -2, -2)).ravel(order='F')
    a = np.r_[(bits-1)*np.arange(2*f, f-1, -1), a[f:]]
    b = np.r_[np.zeros(f+1), b]
    return (np.r_[w[0], wave][:ml+1]*a[:ml+1]
            + np.r_[np.zeros(f+1), wave][:ml+1]*b[:ml+1])

def fft(acf, dt=1.0):
    full = np.concatenate([acf, np.zeros((1, acf.shape[1])), np.conj(acf[:0:-1])])
    return (np.fft.fftshift(np.fft.fft(full, axis=0), axes=0).real*dt).T

for path in sys.argv[1:]:
    with h5py.File(path) as h:
        meta = json.loads(bytes(h['metadata_json'][:]))
        opener = bz2.open if meta['source'].endswith('.bz2') else open
        with opener(meta['source'], 'rb') as f:
            data = f.read()
        assert hashlib.sha256(data).hexdigest() == meta['source_sha256']
        import io
        m = loadmat(io.BytesIO(data))
        d = m['d_data'].ravel().astype(np.complex128)
        norm = m['d_parbl'].ravel()[21]
        for band in range(2):
            offset = band*82232
            broad = d[108611+offset:108811+offset].reshape(50, 4)/norm
            uncoded=h['native_uncoded_acf'][band]
            np.testing.assert_allclose(uncoded[...,0]+1j*uncoded[...,1],broad,rtol=1e-7)
            np.testing.assert_array_equal(h['fft_unmeasured_lag_samples'][:],[0])
            np.testing.assert_allclose(h['broad_spectrum'][band], fft(broad), rtol=2e-6, atol=.02)
            native=d[36611+offset:108611+offset].reshape(800,90)/norm
            short=d[32061+offset:36611+offset].reshape(50,91)/norm
            stored=h['native_long_acf'][band]
            np.testing.assert_allclose(stored[...,0]+1j*stored[...,1],native,rtol=1e-7)
            stored_short=h['native_short_acf'][band]
            np.testing.assert_allclose(stored_short[...,0]+1j*stored_short[...,1],short,rtol=1e-7)
            np.testing.assert_array_equal(h['native_long_lag_samples'][:],np.arange(1,801))
            np.testing.assert_array_equal(h['native_short_lag_samples'][:],np.arange(50))
            lag0=(short[0,:-1]+short[0,1:]).real/(31/3)
            np.testing.assert_allclose(h['decoded_lag0'][band],lag0,rtol=2e-6,atol=1e-5)
            a=np.zeros((801,90),complex)
            a[1:]=(stored[...,0].astype(float)+1j*stored[...,1].astype(float))*np.sqrt((640-.4*np.arange(800))/640)[:,None]
            np.testing.assert_allclose(h['decoded_spectrum'][band],(np.fft.fftshift(np.fft.hfft(a, n=1600, axis=0),axes=0)*.4e-6).T,rtol=2e-6,atol=2e-5)
        expected = d[48:480].real/norm
        background = 2*d[0:16].real.mean()/norm
        np.testing.assert_allclose(h['ion_power'][:], expected, rtol=1e-7)
        np.testing.assert_allclose(h['ion_excess'][:], expected/background-1, rtol=2e-5, atol=1e-7)
        assert h['decoded_spectrum'].shape == (2, 90, 1600)
        assert np.all(np.diff(h['ion_altitude_km'][:]) > 0)
        assert np.all(h['ion_altitude_km'][:] < h['ion_range_km'][:])
        print('PASS', meta['id'], 'both bands, signed lag zero, power, hash, geometry')
