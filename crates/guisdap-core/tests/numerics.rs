use guisdap_core::{decoded_weight, process, C, NFFT, STRIDE};
#[test]
fn decoded_tone_lands_in_the_expected_band_gate_and_frequency() {
    let mut d = vec![C::new(0.0, 0.0); 196493];
    for offset in [0, 32029, 32029 + STRIDE] {
        for x in &mut d[offset..offset + 16] {
            x.re = 100.;
        }
    }
    for band in 0..2 {
        let gate = 23 + band * 10;
        let tone = if band == 0 { 100. } else { -170. };
        for lag in 1..801 {
            d[36611 + band * STRIDE + (lag - 1) * 90 + gate] = C::from_polar(
                decoded_weight(lag),
                2. * std::f64::consts::PI * tone * lag as f64 / NFFT as f64,
            );
        }
    }
    let p = process(&d, 1.).unwrap();
    for band in 0..2 {
        let gate = 23 + band * 10;
        let row = &p.decoded[(band * 90 + gate) * NFFT..(band * 90 + gate + 1) * NFFT];
        let peak = row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        assert_eq!(peak, if band == 0 { 900 } else { 630 });
        assert!(row[peak] > 1e-4);
    }
    assert!(p.ion_excess.iter().all(|x| *x == -1.)); // Negative power estimates are retained.
}
