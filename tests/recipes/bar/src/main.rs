// A mounting bar: a row of through holes, then a top fillet only when the
// holes left enough material (a decision the host's measurement drives).
use kit::{Session, ops};

fn main() {
    let mut s = Session::start();
    let width = s.param_f64("width", 60.0);
    let holes = s.param_u32("holes", 4);
    let part = s.new_box([width, 20.0, 4.0]);
    for i in 0..holes {
        let x = width * (i as f64 + 0.5) / holes as f64;
        s.apply(&part, &ops::hole_down([x, 10.0, 4.0], 1.6, None));
    }
    let v = s.volume(&part);
    if v > 0.8 * width * 20.0 * 4.0 {
        s.apply(&part, &ops::fillet_top(1.0));
    }
    s.done(&part);
}
