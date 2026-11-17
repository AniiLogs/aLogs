//! `paste` without a mask copies whole rows. It must give what the pixel-by-pixel paste gives (here: the masked paste
//! with an all-white mask), wherever the pasted picture lies, and convert the other picture's mode first.

use alogs_pil::{Image, Mode};

fn noise(mode: Mode, w: usize, h: usize, seed: u32) -> Image {
    let n = w * h * mode.bands();
    let mut v = seed;
    let data = (0..n)
        .map(|_| {
            v = v.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (v >> 24) as u8
        })
        .collect();
    Image::from_raw(mode, w, h, data)
}

#[test]
fn rows_copied_are_the_pixels_pasted() {
    let white = |im: &Image| Image::new(Mode::L, im.width, im.height, &[255]);
    for mode in [Mode::Rgb, Mode::Rgba, Mode::L] {
        let base = noise(mode, 40, 30, 1);
        let other = noise(mode, 17, 11, 2);
        // inside, over each edge, wholly outside, and bigger than the picture
        for (x, y) in [(5, 4), (-6, 3), (30, 25), (-3, -4), (38, -9), (60, 5), (5, 60), (-40, 0), (0, -40)] {
            let (mut fast, mut slow) = (base.clone(), base.clone());
            fast.paste(&other, x, y);
            slow.paste_mask(&other, x, y, Some(&white(&other)));
            assert_eq!(fast.data, slow.data, "{mode:?} at ({x}, {y})");
        }
        let big = noise(mode, 90, 70, 3);
        let (mut fast, mut slow) = (base.clone(), base.clone());
        fast.paste(&big, -20, -10);
        slow.paste_mask(&big, -20, -10, Some(&white(&big)));
        assert_eq!(fast.data, slow.data, "{mode:?}, a picture bigger than this one");
    }
    // another mode is converted to this one first, as before
    let base = noise(Mode::Rgb, 40, 30, 4);
    let grey = noise(Mode::L, 12, 9, 5);
    let (mut fast, mut slow) = (base.clone(), base.clone());
    fast.paste(&grey, 7, 6);
    slow.paste_mask(&grey, 7, 6, Some(&Image::new(Mode::L, 12, 9, &[255])));
    assert_eq!(fast.data, slow.data, "a grey picture into a colour one");
}

#[test]
fn a_picture_of_one_value_is_that_value_everywhere() {
    assert!(Image::new(Mode::Rgb, 7, 5, &[0, 0, 0]).data.iter().all(|v| *v == 0));
    assert!(Image::new(Mode::L, 7, 5, &[255]).data.iter().all(|v| *v == 255));
    let im = Image::new(Mode::Rgb, 3, 2, &[10, 20, 30]);
    assert_eq!(im.data, [10, 20, 30].repeat(6));
    assert_eq!(Image::new(Mode::Rgba, 2, 1, &[0, 0, 0, 0]).data, vec![0; 8]);
}

/// By hand: what a look between fights does with its picture, the top part of a 4K frame put into a whole black one
/// (`cargo test -p alogs-pil --release --test paste -- --ignored --nocapture`). 172 ms before rows were copied.
#[test]
#[ignore = "a timing, by hand"]
fn the_top_part_of_a_4k_frame_into_a_whole_one() {
    let top = noise(Mode::Rgb, 3840, 648, 6);
    let t = std::time::Instant::now();
    let mut ms = vec![];
    for _ in 0..20 {
        let t0 = std::time::Instant::now();
        let mut canvas = Image::new(Mode::Rgb, 3840, 2160, &[0, 0, 0]);
        canvas.paste(&top, 0, 0);
        ms.push(t0.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(canvas.data[..9], top.data[..9]);
    }
    ms.sort_by(f64::total_cmp);
    println!("a black 4K picture with the top part put in: typical {:.1} ms, slowest {:.1} ms ({} rounds in {:.0} ms)", ms[10], ms[19], ms.len(), t.elapsed().as_secs_f64() * 1000.0);
}
